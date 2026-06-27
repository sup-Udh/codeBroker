use super::collector::RelationshipCollector;
use super::relationship::{Relationship, RelationshipKind};
use super::visitor::LanguageVisitor;
use graph::{SemanticBinding, SemanticBindingKind};
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct PythonVisitor;

impl LanguageVisitor for PythonVisitor {
    fn visit(&self, tree: &Tree, source_code: &str, collector: &mut RelationshipCollector) {
        let language = tree_sitter_python::LANGUAGE.into();
        emit_imports(tree, source_code, &language, collector);
        emit_calls(tree, source_code, &language, collector);
        emit_inheritance(tree, source_code, &language, collector);
        emit_type_refs(tree, source_code, &language, collector);
        emit_decorators(tree, source_code, &language, collector);
        emit_global_refs(tree, source_code, &language, collector);
    }

    fn visit_semantic(&self, tree: &Tree, source_code: &str) -> Vec<SemanticBinding> {
        let language = tree_sitter_python::LANGUAGE.into();
        emit_semantic_bindings(tree, source_code, &language)
    }
}

fn emit_imports(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "
        (import_statement name: (_) @import)
        (import_from_statement module_name: (_) @source name: (_) @import)
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut source = String::new();
        let mut line = 0usize;

        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let text = text.trim().to_string();
                match *cn {
                    "import" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                    }
                    "source" => source = text,
                    _ => {}
                }
            }
        }

        if !name.is_empty() && name != "*" {
            let rel = if source.is_empty() {
                Relationship::new(name, RelationshipKind::Import, line)
            } else {
                Relationship::new(name, RelationshipKind::Import, line).with_source(source)
            };
            collector.emit(rel);
        }
    }
}

fn emit_calls(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // ---- Method calls with identifier receiver (emitted first for dedup priority) ----
    let q_meth_recv = "(call function: (attribute object: (identifier) @receiver attribute: (identifier) @method))";
    if let Ok(query) = Query::new(language, q_meth_recv) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut receiver = String::new();
            let mut method = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "receiver" => receiver = t,
                        "method" => {
                            method = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !method.is_empty() && !crate::utils::is_noisy_call_name(&method) {
                let rel = Relationship::new(method, RelationshipKind::MethodCall, line);
                let rel = if !receiver.is_empty() { rel.with_source(receiver) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- Constructor bindings: x = SomeClass() where x is a simple identifier ----
    // Emitted first so dedup keeps version with variable name.
    let q_ctor = "(assignment left: (identifier) @var_name right: (call function: (identifier) @constructor))";
    if let Ok(query) = Query::new(language, q_ctor) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut var_name = String::new();
            let mut constructor = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "var_name" => var_name = t,
                        "constructor" => {
                            constructor = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !constructor.is_empty() && !crate::utils::is_noisy_call_name(&constructor) {
                let rel = Relationship::new(constructor, RelationshipKind::Instantiates, line);
                let rel = if !var_name.is_empty() { rel.with_source(var_name) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- self.field.method() — two-level attribute chain from `self` ----
    // Source set to "self.<field>" to distinguish from a local var receiver.
    let q_self_meth = "(call function: (attribute object: (attribute object: (identifier) @self_obj attribute: (identifier) @field_name) attribute: (identifier) @method_name))";
    if let Ok(query) = Query::new(language, q_self_meth) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut self_obj = String::new();
            let mut field_name = String::new();
            let mut method_name = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "self_obj" => self_obj = t,
                        "field_name" => field_name = t,
                        "method_name" => {
                            method_name = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if (self_obj == "self" || self_obj == "cls")
                && !method_name.is_empty()
                && !field_name.is_empty()
                && !crate::utils::is_noisy_call_name(&method_name)
            {
                let source = format!("self.{}", field_name);
                let rel = Relationship::new(method_name, RelationshipKind::MethodCall, line)
                    .with_source(source);
                collector.emit(rel);
            }
        }
    }

    // ---- Fallback queries (deduplicated against receiver-aware results above) ----
    let query_str = "
        (call function: (identifier) @call_name)
        (call function: (attribute attribute: (identifier) @method_call))
        (attribute attribute: (identifier) @member_access)
        (assignment right: (call function: (identifier) @instantiates))
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if crate::utils::is_noisy_call_name(&name) || name.is_empty() {
                    continue;
                }
                let kind = match *cn {
                    "call_name" => RelationshipKind::Call,
                    "method_call" => RelationshipKind::MethodCall,
                    "member_access" => RelationshipKind::MemberAccess,
                    "instantiates" => RelationshipKind::Instantiates,
                    _ => continue,
                };
                collector.emit(Relationship::new(name, kind, line));
            }
        }
    }
}

fn emit_inheritance(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "
        (class_definition superclasses: (argument_list (identifier) @inherits))
        (class_definition superclasses: (argument_list (attribute attribute: (identifier) @inherits)))
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() {
                    collector.emit(Relationship::new(name, RelationshipKind::Inherits, line));
                }
            }
        }
    }
}

fn emit_type_refs(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // Function parameter type annotations and return type annotations
    let query_str = "
        (typed_parameter type: (type (identifier) @type_ref))
        (typed_default_parameter type: (type (identifier) @type_ref))
        (typed_parameter type: (type (subscript value: (identifier) @type_ref)))
        (typed_default_parameter type: (type (subscript value: (identifier) @type_ref)))
        (function_definition return_type: (type (identifier) @type_ref))
        (function_definition return_type: (type (subscript value: (identifier) @type_ref)))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !crate::utils::is_noisy_call_name(&name) && !is_py_builtin(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::TypeRef, line));
                }
            }
        }
    }
}

fn emit_decorators(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // Python decorators: @app.get("/"), @property, @staticmethod, @classmethod,
    // @login_required etc. We emit the outermost name without arguments.
    let query_str = "
        (decorator (identifier) @decorator_name)
        (decorator (call function: (identifier) @decorator_name))
        (decorator (call function: (attribute attribute: (identifier) @decorator_name)))
        (decorator (attribute attribute: (identifier) @decorator_name))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !is_py_builtin_decorator(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::Annotation, line));
                }
            }
        }
    }
}

fn emit_global_refs(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "(global_statement (identifier) @global_ref)";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !crate::utils::is_noisy_call_name(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::GlobalRef, line));
                }
            }
        }
    }
}

fn emit_semantic_bindings(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
) -> Vec<SemanticBinding> {
    let mut bindings = Vec::new();

    // ── Variable type annotations: x: Type = ... (annotated_assignment) ─────
    let q_var_type = "(annotated_assignment left: (identifier) @var_name annotation: (identifier) @type_name)";
    if let Ok(query) = Query::new(language, q_var_type) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut var_name = String::new();
            let mut type_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "var_name" => var_name = text.trim().to_string(),
                        "type_name" => type_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !var_name.is_empty() && !type_name.is_empty() && !is_py_builtin(&type_name) {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::VarType,
                    name: var_name,
                    type_name,
                    context: None,
                });
            }
        }
    }

    // ── Function return type annotations: def f() -> Type ───────────────────
    let q_ret = "(function_definition name: (identifier) @func_name return_type: (type (identifier) @return_type))";
    if let Ok(query) = Query::new(language, q_ret) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut func_name = String::new();
            let mut return_type = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "func_name" => func_name = text.trim().to_string(),
                        "return_type" => return_type = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !func_name.is_empty() && !return_type.is_empty() && !is_py_builtin(&return_type) {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::ReturnType,
                    name: func_name,
                    type_name: return_type,
                    context: None,
                });
            }
        }
    }

    // ── Class field annotations: class C: \n  db: Type ──────────────────────
    let q_field = "(class_definition name: (identifier) @class_name body: (block (expression_statement (annotated_assignment left: (identifier) @field_name annotation: (identifier) @type_name))))";
    if let Ok(query) = Query::new(language, q_field) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut class_name = String::new();
            let mut field_name = String::new();
            let mut type_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "class_name" => class_name = text.trim().to_string(),
                        "field_name" => field_name = text.trim().to_string(),
                        "type_name" => type_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !field_name.is_empty() && !type_name.is_empty() && !is_py_builtin(&type_name) {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::FieldType,
                    name: field_name,
                    type_name,
                    context: if class_name.is_empty() { None } else { Some(class_name) },
                });
            }
        }
    }

    // ── Alias assignments: x = y (bare identifier RHS) ──────────────────────
    let q_alias = "(assignment left: (identifier) @alias_name right: (identifier) @source_name)";
    if let Ok(query) = Query::new(language, q_alias) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut alias_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "alias_name" => alias_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !alias_name.is_empty() && !source_name.is_empty() && alias_name != source_name {
                bindings.push(SemanticBinding {
                    kind: SemanticBindingKind::Alias,
                    name: alias_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    bindings
}

fn is_py_builtin(name: &str) -> bool {
    matches!(
        name,
        "int" | "str" | "float" | "bool" | "bytes" | "list" | "dict" | "set"
        | "tuple" | "None" | "True" | "False" | "object" | "type"
        | "Optional" | "Union" | "List" | "Dict" | "Set" | "Tuple" | "Any"
        | "Callable" | "Iterator" | "Generator" | "Awaitable" | "Coroutine"
        | "T" | "K" | "V" | "R"
    )
}

fn is_py_builtin_decorator(name: &str) -> bool {
    matches!(
        name,
        "property" | "staticmethod" | "classmethod" | "abstractmethod"
        | "overload" | "dataclass" | "cached_property" | "final"
    )
}

#[cfg(test)]
mod python_visitor_tests {
    use super::*;
    use crate::discovery::collector::RelationshipCollector;

    fn parse_and_collect(src: &str) -> Vec<graph::RelationshipNode> {
        let language = tree_sitter_python::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut collector = RelationshipCollector::new();
        PythonVisitor.visit(&tree, src, &mut collector);
        collector.into_relationship_nodes()
    }

    #[test]
    fn from_import_produces_import_edge() {
        let src = "from auth import login";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "login" && r.kind.as_deref() == Some("imports")));
    }

    #[test]
    fn class_inheritance_produces_inherits_edge() {
        let src = "class UserService(BaseService): pass";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "BaseService" && r.kind.as_deref() == Some("inherits")));
    }

    #[test]
    fn type_annotation_produces_type_ref() {
        let src = "def simulate(topology: Topology): pass";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Topology" && r.kind.as_deref() == Some("type_ref")));
    }

    #[test]
    fn decorator_produces_annotation_edge() {
        let src = "@login_required\ndef view(request): pass";
        let rels = parse_and_collect(src);
        assert!(
            rels.iter().any(|r| r.name == "login_required" && r.kind.as_deref() == Some("annotation")),
            "decorator should produce annotation; got {rels:?}"
        );
    }

    #[test]
    fn global_ref_produces_global_ref_edge() {
        let src = "def f():\n    global counter\n    counter += 1";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "counter" && r.kind.as_deref() == Some("global_ref")));
    }

    #[test]
    fn wildcard_import_produces_no_edge() {
        let src = "from os import *";
        let rels = parse_and_collect(src);
        assert!(rels.iter().all(|r| r.name != "*"), "wildcard import must not produce edge");
    }
}
