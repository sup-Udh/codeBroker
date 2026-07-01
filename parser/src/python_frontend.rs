use crate::frontend::LanguageFrontend;
use graph::{RelationshipNode, SemanticBinding, SymbolNode};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct PythonFrontend;

impl LanguageFrontend for PythonFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".py")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        _path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<RelationshipNode>,
        Vec<SemanticBinding>,
    )> {
        let language = tree_sitter_python::LANGUAGE.into();
        let tree = crate::pool::with_parser("python", &language, |parser| {
            parser.parse(source_code, None)
        })?;

        let symbols = extract_py_symbols(&tree, source_code, language);

        let mut collector = crate::discovery::RelationshipCollector::new();
        crate::discovery::LanguageVisitor::visit(
            &crate::discovery::python::PythonVisitor,
            &tree,
            source_code,
            &mut collector,
        );
        let relationships = collector.into_relationship_nodes();
        let semantic = crate::discovery::LanguageVisitor::visit_semantic(
            &crate::discovery::python::PythonVisitor,
            &tree,
            source_code,
        );

        Some((graph::models::FileMetadata::default(), symbols, relationships, semantic))
    }
}

fn extract_py_symbols(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<SymbolNode> {
    let query_str = "
        (class_definition name: (identifier) @name) @class
        (function_definition name: (identifier) @name) @function
        (expression_statement (assignment left: (identifier) @name)) @variable
    ";

    crate::pool::with_query("python_symbols", &language, query_str, |query| {
    let mut symbols = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut kind = String::new();
        let mut def_node = None;

        for capture in m.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            if *capture_name == "name" {
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    name = text.to_string();
                }
            } else if *capture_name == "class" {
                kind = "class".to_string();
                def_node = Some(capture.node);
            } else if *capture_name == "function" {
                kind = "function".to_string();
                def_node = Some(capture.node);
            } else if *capture_name == "variable" {
                kind = "variable".to_string();
                def_node = Some(capture.node);
            }
        }

        if let Some(node) = def_node {
            let mut signature_parts = Vec::new();
            let short_name = name.clone();
            let mut attributes = Vec::new();

            // 1. Check for decorators
            if let Some(parent) = node.parent() {
                if parent.kind() == "decorated_definition" {
                    let mut wcursor = parent.walk();
                    for child in parent.children(&mut wcursor) {
                        if child.kind() == "decorator" {
                            if let Ok(text) = child.utf8_text(source_code.as_bytes()) {
                                let trimmed = text.trim();
                                signature_parts.push(format!("[{}]", trimmed));
                                attributes.push(trimmed.to_string());
                            }
                        }
                    }
                }
            }

            // 2. If it's a function, check if it's a method
            if kind == "function" {
                let mut current = node.parent();
                while let Some(p) = current {
                    if p.kind() == "class_definition" {
                        if p.child_by_field_name("name").is_some() {
                            kind = "method".to_string();
                        }
                        break;
                    }
                    if p.kind() == "decorated_definition" {
                        current = p.parent();
                        continue;
                    }
                    current = p.parent();
                }

                // 3. Extract parameters for signature
                let mut head = format!("def {}", short_name);
                if let Some(params_node) = node.child_by_field_name("parameters") {
                    if let Ok(params_text) = params_node.utf8_text(source_code.as_bytes()) {
                        let clean_params = params_text.replace("\n", "").replace("  ", " ");
                        head.push_str(&clean_params);
                    }
                }
                if let Some(return_node) = node.child_by_field_name("return_type") {
                    if let Ok(return_text) = return_node.utf8_text(source_code.as_bytes()) {
                        head.push_str(" -> ");
                        head.push_str(return_text.trim());
                    }
                }
                signature_parts.push(head);
            } else if kind == "class" {
                let mut head = format!("class {}", short_name);
                if let Some(bases_node) = node.child_by_field_name("superclasses") {
                    if let Ok(bases_text) = bases_node.utf8_text(source_code.as_bytes()) {
                        head.push_str(bases_text.trim());
                    }
                }
                signature_parts.push(head);
            }

            let signature = if signature_parts.is_empty() {
                None
            } else {
                Some(signature_parts.join(" "))
            };

            let end_line = node.end_position().row + 1;
            symbols.push(SymbolNode {
                name,
                kind,
                start_line: node.start_position().row + 1,
                end_line,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                signature,
                attributes,
                metadata: None,
            });
        }
    }

    symbols.sort_by_key(|s| s.start_byte);
    symbols.dedup_by_key(|s| s.start_byte);

    symbols
    })
}

#[cfg(test)]
mod type_ref_tests {
    use super::*;
    use crate::frontend::LanguageFrontend;

    // Regression for the impact-analysis inversion bug: a function whose
    // parameter is annotated with a class (`topology: Topology`) must produce a
    // real reference edge to that class, while a same-named parameter with a
    // different annotation (`topology: dict`) must NOT — the dependency is the
    // resolved type, not the parameter identifier.
    #[test]
    fn typed_parameter_emits_type_ref_for_the_annotation_only() {
        let src = "\
class Topology:
    pass

def simulate(topology: Topology):
    return topology

def analyze(topology: dict):
    return topology
";
        let (_m, _syms, imports, _) = PythonFrontend.parse_and_extract(src, "main.py").unwrap();

        let type_refs: Vec<&str> = imports
            .iter()
            .filter(|i| i.kind.as_deref() == Some("type_ref"))
            .map(|i| i.name.as_str())
            .collect();

        assert!(
            type_refs.contains(&"Topology"),
            "`simulate(topology: Topology)` must reference the Topology type, got {type_refs:?}"
        );
        // The plain-`dict` parameter must not masquerade as a Topology reference.
        assert!(
            !type_refs.contains(&"topology"),
            "the parameter identifier must never be treated as a type reference"
        );
    }

    #[test]
    fn return_type_annotation_emits_type_ref() {
        let src = "\
class Result:
    pass

def run() -> Result:
    return Result()
";
        let (_m, _syms, imports, _) = PythonFrontend.parse_and_extract(src, "main.py").unwrap();
        assert!(
            imports
                .iter()
                .any(|i| i.kind.as_deref() == Some("type_ref") && i.name == "Result"),
            "return-type annotation should produce a type_ref to Result"
        );
    }
}

#[allow(dead_code)]
fn extract_py_imports(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<RelationshipNode> {
    let mut imports = Vec::new();
    // Grab any individual identifier inside any import statement
    let query_str = "
        (import_statement name: (_) @import)
        (import_from_statement module_name: (_) @source name: (_) @import)
        (class_definition superclasses: (argument_list (identifier) @inherits))
        (assignment right: (call function: (identifier) @instantiates))
        (call function: (identifier) @call_name)
        (call function: (attribute attribute: (identifier) @call_name))
        (attribute attribute: (identifier) @member_access)
        (typed_parameter type: (type (identifier) @type_ref))
        (typed_default_parameter type: (type (identifier) @type_ref))
        (typed_parameter type: (type (subscript (identifier) @type_ref)))
        (typed_default_parameter type: (type (subscript (identifier) @type_ref)))
        (function_definition return_type: (type (identifier) @type_ref))
        (function_definition return_type: (type (subscript (identifier) @type_ref)))
        (global_statement (identifier) @global_ref)
    ";

    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut import_name = String::new();
        let mut import_source = String::new();
        let mut import_kind = None;
        let mut line_number = 0;

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "import" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "source" {
                    import_source = text.trim().to_string();
                } else if *capture_kind == "inherits" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("inherits".to_string());
                } else if *capture_kind == "instantiates" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("instantiates".to_string());
                } else if *capture_kind == "call_name" {
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    line_number = node.start_position().row + 1;
                    import_kind = Some("calls".to_string());
                } else if *capture_kind == "member_access" {
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        line_number = node.start_position().row + 1;
                        import_kind = Some("MEMBER_ACCESS".to_string());
                    }
                } else if *capture_kind == "type_ref" {
                    // A type annotation on a parameter or return value
                    // (`def simulate(topology: Topology)`). This is a genuine
                    // dependency on the annotated type — the edge the old
                    // parser missed entirely, leaving impact-analysis to rely
                    // on phantom name-fragment matches instead.
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        line_number = node.start_position().row + 1;
                        import_kind = Some("type_ref".to_string());
                    }
                } else if *capture_kind == "global_ref" {
                    // `global x` inside a function body explicitly declares that
                    // the function reads/writes the module-level variable `x`.
                    // This is real coupling that impact_analysis must count —
                    // functions sharing globals are coupled even with no call edges.
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name;
                        line_number = node.start_position().row + 1;
                        import_kind = Some("global_ref".to_string());
                    }
                }
            }
        }

        // Wildcard imports (`from x import *`) can't resolve to a specific
        // symbol, so they produce no edge.
        if !import_name.is_empty() && import_name != "*" {
            imports.push(RelationshipNode {
                name: import_name,
                source: if import_source.is_empty() {
                    None
                } else {
                    Some(import_source)
                },
                line_number,
                kind: import_kind,
            });
        }
    }
    imports
}
