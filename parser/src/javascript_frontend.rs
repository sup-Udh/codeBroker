use crate::frontend::LanguageFrontend;
use graph::{RelationshipNode, SemanticBinding, SymbolNode};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct JavaScriptFrontend;

impl LanguageFrontend for JavaScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".js") || path.ends_with(".jsx")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<RelationshipNode>,
        Vec<SemanticBinding>,
    )> {
        let language = tree_sitter_javascript::LANGUAGE.into();
        let tree = crate::pool::with_parser("javascript", &language, |parser| {
            parser.parse(source_code, None)
        })?;

        let metadata = graph::models::FileMetadata { metadata: None };
        let symbols = extract_js_symbols(&tree, source_code, language, path);

        let mut collector = crate::discovery::RelationshipCollector::new();
        crate::discovery::LanguageVisitor::visit(
            &crate::discovery::javascript::JavaScriptVisitor,
            &tree,
            source_code,
            &mut collector,
        );
        let relationships = collector.into_relationship_nodes();

        Some((metadata, symbols, relationships, Vec::new()))
    }
}

fn extract_js_symbols(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
    path: &str,
) -> Vec<SymbolNode> {
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_jsx = path.ends_with(".jsx");
    let query_str = "
        (class_declaration name: (identifier) @type)
        (class_declaration
          body: (class_body
            (method_definition name: (property_identifier) @method)))
        (class_declaration
          body: (class_body
            (field_definition property: (property_identifier) @method value: (arrow_function))))
        (function_declaration name: (identifier) @function)
        (return_statement (jsx_element) @jsx_render)
        (return_statement (parenthesized_expression (jsx_element) @jsx_render))
        (lexical_declaration
            (variable_declarator
                name: (identifier) @function
                value: (arrow_function)
            )
        )
        (lexical_declaration
            (variable_declarator
                name: (identifier) @function
                value: (call_expression)
            )
        )
        (lexical_declaration
            (variable_declarator
                name: (identifier) @data_const
                value: (_)
            )
        )
    ";

    crate::pool::with_query("javascript_symbols", &language, query_str, |query| {
    let mut symbols = Vec::new();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let mut name_str = name.to_string();
                let mut kind = capture_kind.to_string();

                if kind == "jsx_render" {
                    name_str = "render".to_string();
                    kind = "jsx_element".to_string();
                }

                let mut parent = node.parent().unwrap_or(node);

                // The node that actually owns a "body" field (function/class/arrow),
                // captured before `parent` gets widened to its lexical_declaration/export_statement.
                // field_definition (an arrow-function class property, e.g.
                // `handleClick = () => {...}`) has a "value" field the same
                // as variable_declarator, so the signature can be built the
                // same way.
                let decl_node = if parent.kind() == "variable_declarator" || parent.kind() == "field_definition" {
                    parent.child_by_field_name("value")
                } else {
                    Some(parent)
                };

                let mut is_call_expr_assignment = false;
                if parent.kind() == "variable_declarator" {
                    if let Some(value_node) = parent.child_by_field_name("value") {
                        if value_node.kind() == "call_expression" {
                            is_call_expr_assignment = true;
                        }
                    }
                }

                let mut is_exported = false;
                let mut current = parent;
                while let Some(p) = current.parent() {
                    if p.kind() == "export_statement" || p.kind() == "export_clause" {
                        is_exported = true;
                        break;
                    }
                    // A statement_block belongs to an enclosing function/method body.
                    // Anything nested inside one is a local, never a module-level export,
                    // no matter what wraps the enclosing function.
                    if p.kind() == "program" || p.kind() == "statement_block" {
                        break;
                    }
                    current = p;
                }

                if is_call_expr_assignment && kind == "function" {
                    if !is_exported {
                        continue; // Skip indexing this local generic variable
                    }
                    kind = "variable".to_string();
                }

                if kind == "data_const" {
                    if !is_exported {
                        continue;
                    }
                    kind = "constant".to_string();
                }

                if parent.kind() == "variable_declarator" {
                    if let Some(lex) = parent.parent() {
                        if lex.kind() == "lexical_declaration" {
                            parent = lex;
                        }
                    }
                }
                if let Some(exp) = parent.parent() {
                    if exp.kind() == "export_statement" {
                        parent = exp;
                    }
                }

                let end_line = parent.end_position().row + 1;

                let signature = decl_node
                    .and_then(|d| d.child_by_field_name("body"))
                    .and_then(|body| {
                        source_code
                            .get(parent.start_byte()..body.start_byte())
                            .map(|s| s.trim_end().to_string())
                    })
                    .filter(|s| !s.is_empty());

                symbols.push(SymbolNode {
                    name: name_str,
                    kind,
                    start_line: parent.start_position().row + 1,
                    end_line,
                    start_byte: parent.start_byte(),
                    end_byte: parent.end_byte(),
                    signature,
                    attributes: Vec::new(),
                    metadata: None,
                });
            }
        }
    }
    symbols
    })
}

#[allow(dead_code)]
fn extract_js_imports(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
) -> Vec<RelationshipNode> {
    let mut imports = Vec::new();
    let query_str = "
        (import_statement
            (import_clause (named_imports (import_specifier name: (identifier) @import)))
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (identifier) @import)
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (namespace_import (identifier) @ns_import))
            source: (string (string_fragment) @source)
        )
        (export_statement
            (export_clause (export_specifier name: (identifier) @re_export))
            source: (string (string_fragment) @source)
        )
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @method_call))
        (member_expression property: (property_identifier) @member_access)
        (new_expression constructor: (identifier) @new_call)
        (class_declaration (class_heritage (identifier) @extends_class))
    ";

    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut import_name = String::new();
        let mut import_source = String::new();
        let mut line_number = 0;

        let mut import_kind = "imports".to_string();

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "import" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "source" {
                    import_source = text.trim().to_string();
                } else if *capture_kind == "call_name" {
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    import_kind = "calls".to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "method_call" {
                    // Member-access invocation (obj.foo()). Tracked under a
                    // distinct kind so the linker never resolves it against a
                    // same-named top-level symbol — that bare-name matching is
                    // what produced phantom edges like `query.delete()` ->
                    // exported `DELETE`.
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    import_kind = "method_call".to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "member_access" {
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        import_kind = "MEMBER_ACCESS".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "new_call" {
                    let name = text.trim().to_string();
                    if !crate::utils::is_noisy_call_name(&name) {
                        import_name = name.clone();
                        import_kind = "new_call".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "extends_class" {
                    let name = text.trim().to_string();
                    if !name.is_empty() {
                        import_name = name.clone();
                        import_kind = "extends".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "re_export" {
                    let name = text.trim().to_string();
                    if !name.is_empty() {
                        import_name = name.clone();
                        import_kind = "re_export".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "ns_import" {
                    let name = text.trim().to_string();
                    if !name.is_empty() {
                        import_name = name.clone();
                        import_kind = "imports".to_string();
                        line_number = node.start_position().row + 1;
                    }
                }
            }
        }

        if !import_name.is_empty() {
            imports.push(RelationshipNode {
                name: import_name,
                source: if import_source.is_empty() {
                    None
                } else {
                    Some(import_source)
                },
                line_number,
                kind: Some(import_kind),
            });
        }
    }
    imports
}

#[cfg(test)]
mod call_resolution_tests {
    use super::*;
    use crate::frontend::LanguageFrontend;

    // #2 — call_resolution_fixture: a member-access call (obj.foo()) must be
    // tagged "method_call" so the linker never resolves it to a same-named
    // top-level symbol, while a free call (foo()) stays "calls".
    #[test]
    fn member_calls_tagged_method_call_free_calls_tagged_calls() {
        let src = r#"
            export function GET() {
                helper();
                query.deleteRoom();
            }
            function helper() {}
        "#;
        let (_meta, _symbols, imports, _) = JavaScriptFrontend
            .parse_and_extract(src, "route.js")
            .expect("parse should succeed");

        let kind_of = |name: &str| {
            imports
                .iter()
                .find(|i| i.name == name)
                .and_then(|i| i.kind.clone())
        };

        assert_eq!(
            kind_of("helper").as_deref(),
            Some("calls"),
            "free call helper() should be a 'calls' edge"
        );
        assert_eq!(
            kind_of("deleteRoom").as_deref(),
            Some("method_call"),
            "member call query.deleteRoom() should be a 'method_call', not 'calls'"
        );
    }
}
