use super::collector::RelationshipCollector;
use super::relationship::{Relationship, RelationshipKind};
use super::visitor::LanguageVisitor;
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct JavaScriptVisitor;

impl LanguageVisitor for JavaScriptVisitor {
    fn visit(&self, tree: &Tree, source_code: &str, collector: &mut RelationshipCollector) {
        let language = tree_sitter_javascript::LANGUAGE.into();
        emit_imports(tree, source_code, &language, collector);
        emit_calls(tree, source_code, &language, collector);
        emit_inheritance(tree, source_code, &language, collector);
    }

    fn visit_semantic(&self, tree: &Tree, source_code: &str) -> Vec<graph::SemanticBinding> {
        let language = tree_sitter_javascript::LANGUAGE.into();
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
        (variable_declarator
            name: (identifier) @cjs_import
            value: (call_expression
                function: (identifier) @require_fn
                arguments: (arguments (string (string_fragment) @source))
            )
        )
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut source = String::new();
        let mut kind = RelationshipKind::Import;
        let mut line = 0usize;
        // Only set for the CommonJS `require(...)` pattern; used to confirm
        // the called function is actually named "require" (tree-sitter has
        // no #eq? predicate support wired up in this codebase, so the check
        // happens here instead of in the query).
        let mut require_fn_text: Option<String> = None;

        for capture in m.captures {
            let cn = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let text = text.trim().to_string();
                match *cn {
                    "import" | "ns_import" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                        kind = RelationshipKind::Import;
                    }
                    "re_export" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                        kind = RelationshipKind::ReExport;
                    }
                    "cjs_import" => {
                        name = text;
                        line = capture.node.start_position().row + 1;
                        kind = RelationshipKind::Import;
                    }
                    "require_fn" => require_fn_text = Some(text),
                    "source" => source = text,
                    _ => {}
                }
            }
        }

        if require_fn_text.is_some_and(|f| f != "require") {
            continue;
        }

        if !name.is_empty() {
            let rel = if source.is_empty() {
                Relationship::new(name, kind, line)
            } else {
                Relationship::new(name, kind, line).with_source(source)
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
    // ---- Receiver-aware method calls: obj.method() where obj is an identifier ----
    // Emitted first so dedup keeps the version with receiver info. Without this,
    // every `foo.method()` call loses its receiver entirely (`source: None`),
    // which forces MemberResolverStage to fail immediately and fall through to
    // LexicalGenerationStage's low-confidence global name lookup even when the
    // receiver is a perfectly resolvable local import — see typescript.rs's
    // emit_calls, which this mirrors.
    let q_meth_recv = "(call_expression function: (member_expression object: (identifier) @receiver property: (property_identifier) @method))";
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

    // ---- Bare this.method() — direct call on the enclosing instance, as
    // opposed to this.field.method() (q_this_meth below) or obj.method() on
    // some other variable (q_meth_recv above). Source is the literal
    // sentinel "this" so MemberResolverStage resolves it against the
    // enclosing class instead of trying to look up a field's type.
    let q_this_bare = "(call_expression function: (member_expression object: (this) property: (property_identifier) @method))";
    if let Ok(query) = Query::new(language, q_this_bare) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if *cn != "method" {
                    continue;
                }
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let method = text.trim().to_string();
                    let line = capture.node.start_position().row + 1;
                    if !method.is_empty() && !crate::utils::is_noisy_call_name(&method) {
                        let rel = Relationship::new(method, RelationshipKind::MethodCall, line)
                            .with_source("this".to_string());
                        collector.emit(rel);
                    }
                }
            }
        }
    }

    // ---- New expressions with variable binding: const x = new Foo() ----
    // Emitted first so dedup keeps the version with variable name.
    let q_new_var = "(lexical_declaration (variable_declarator name: (identifier) @var_name value: (new_expression constructor: (identifier) @constructor)))";
    if let Ok(query) = Query::new(language, q_new_var) {
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
                let rel = Relationship::new(constructor, RelationshipKind::NewCall, line);
                let rel = if !var_name.is_empty() { rel.with_source(var_name) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- Constructor bindings on this: this.field = new SomeClass() ----
    // The standard JS dependency-injection idiom (assigned in a
    // constructor), but q_new_var above only matches a plain variable
    // declarator target, never a `this.field` assignment target. Source is
    // the bare field name (not "this.<field>") to match the convention
    // FieldType semantic bindings already use — MemberResolverStage strips
    // "this."/"self." down to before calling flow_engine.get_var, so the two
    // must agree on the same bare key.
    let q_this_ctor = "(assignment_expression left: (member_expression object: (this) property: (property_identifier) @field_name) right: (new_expression constructor: (identifier) @constructor))";
    if let Ok(query) = Query::new(language, q_this_ctor) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut field_name = String::new();
            let mut constructor = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "field_name" => field_name = t,
                        "constructor" => {
                            constructor = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !field_name.is_empty() && !constructor.is_empty() && !crate::utils::is_noisy_call_name(&constructor) {
                let rel = Relationship::new(constructor, RelationshipKind::NewCall, line)
                    .with_source(field_name);
                collector.emit(rel);
            }
        }
    }

    // ---- this.field.method() — two-level member chain from `this` ----
    // Emitted before fallback so dedup keeps this version (with field context in source).
    let q_this_meth = "(call_expression function: (member_expression object: (member_expression object: (this) property: (property_identifier) @field_name) property: (property_identifier) @method_name))";
    if let Ok(query) = Query::new(language, q_this_meth) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut field_name = String::new();
            let mut method_name = String::new();
            let mut line = 0usize;
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let t = text.trim().to_string();
                    match *cn {
                        "field_name" => field_name = t,
                        "method_name" => {
                            method_name = t;
                            line = capture.node.start_position().row + 1;
                        }
                        _ => {}
                    }
                }
            }
            if !method_name.is_empty() && !field_name.is_empty()
                && !crate::utils::is_noisy_call_name(&method_name)
            {
                let source = format!("this.{}", field_name);
                let rel = Relationship::new(method_name, RelationshipKind::MethodCall, line)
                    .with_source(source);
                collector.emit(rel);
            }
        }
    }

    // ---- Fallback queries (deduplicated against receiver-aware results above) ----
    let query_str = "
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @method_call))
        (member_expression property: (property_identifier) @member_access)
        (new_expression constructor: (identifier) @new_call)
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
                    "new_call" => RelationshipKind::NewCall,
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
        (class_declaration (class_heritage (identifier) @extends_class))
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
                    collector.emit(Relationship::new(name, RelationshipKind::Extends, line));
                }
            }
        }
    }
}

fn emit_semantic_bindings(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
) -> Vec<graph::SemanticBinding> {
    let mut bindings = Vec::new();

    // ── Variable assignments from constructors: const x = new Foo() ────────
    let q_var_type = "
        (lexical_declaration (variable_declarator name: (identifier) @var_name value: (new_expression constructor: (identifier) @type_name)))
        (variable_declaration (variable_declarator name: (identifier) @var_name value: (new_expression constructor: (identifier) @type_name)))
        (assignment_expression left: (identifier) @var_name right: (new_expression constructor: (identifier) @type_name))
        (assignment_expression left: (member_expression object: (this) property: (property_identifier) @var_name) right: (new_expression constructor: (identifier) @type_name))
    ";
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
            if !var_name.is_empty() && !type_name.is_empty() {
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::VarType,
                    name: var_name,
                    type_name,
                    context: None,
                });
            }
        }
    }

    // ── Alias assignments: const x = y (bare identifier RHS) ────────────────
    let q_alias = "
        (lexical_declaration (variable_declarator name: (identifier) @alias_name value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (identifier) @alias_name value: (identifier) @source_name))
        (assignment_expression left: (identifier) @alias_name right: (identifier) @source_name)
    ";
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
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::Alias,
                    name: alias_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Assignments from function calls: const x = foo() ────────────────────
    let q_assign_call = "
        (lexical_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (identifier) @source_name)))
        (variable_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (identifier) @source_name)))
        (assignment_expression left: (identifier) @assign_name right: (call_expression function: (identifier) @source_name))
        (lexical_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (member_expression property: (property_identifier) @source_name))))
        (variable_declaration (variable_declarator name: (identifier) @assign_name value: (call_expression function: (member_expression property: (property_identifier) @source_name))))
        (assignment_expression left: (identifier) @assign_name right: (call_expression function: (member_expression property: (property_identifier) @source_name)))
    ";
    if let Ok(query) = Query::new(language, q_assign_call) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut assign_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "assign_name" => assign_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !assign_name.is_empty() && !source_name.is_empty() {
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::Assignment,
                    name: assign_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Destructuring: const { login } = auth ──────────────────────────────
    let q_destructuring = "
        (lexical_declaration (variable_declarator name: (object_pattern (shorthand_property_identifier_pattern) @destruct_name) value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (object_pattern (shorthand_property_identifier_pattern) @destruct_name) value: (identifier) @source_name))
        (lexical_declaration (variable_declarator name: (object_pattern (pair_pattern value: (identifier) @destruct_name)) value: (identifier) @source_name))
        (variable_declaration (variable_declarator name: (object_pattern (pair_pattern value: (identifier) @destruct_name)) value: (identifier) @source_name))
    ";
    if let Ok(query) = Query::new(language, q_destructuring) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut destruct_name = String::new();
            let mut source_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "destruct_name" => destruct_name = text.trim().to_string(),
                        "source_name" => source_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !destruct_name.is_empty() && !source_name.is_empty() {
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::Destructuring,
                    name: destruct_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    // ── Object literal: const api = { login(){} } ──────────────────────────
    let q_obj_literal = "
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (pair key: (property_identifier) @prop_name))))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (pair key: (property_identifier) @prop_name))))
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (shorthand_property_identifier) @prop_name)))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (shorthand_property_identifier) @prop_name)))
        (lexical_declaration (variable_declarator name: (identifier) @obj_name value: (object (method_definition name: (property_identifier) @prop_name))))
        (variable_declaration (variable_declarator name: (identifier) @obj_name value: (object (method_definition name: (property_identifier) @prop_name))))
    ";
    if let Ok(query) = Query::new(language, q_obj_literal) {
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
        while let Some(m) = matches.next() {
            let mut obj_name = String::new();
            let mut prop_name = String::new();
            for capture in m.captures {
                let cn = &query.capture_names()[capture.index as usize];
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    match *cn {
                        "obj_name" => obj_name = text.trim().to_string(),
                        "prop_name" => prop_name = text.trim().to_string(),
                        _ => {}
                    }
                }
            }
            if !obj_name.is_empty() && !prop_name.is_empty() {
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::ObjectLiteral,
                    name: prop_name,
                    type_name: obj_name,
                    context: None,
                });
            }
        }
    }

    // ── Import alias: import { Foo as Bar } ────────────────────────────────
    let q_import_alias = "
        (import_statement (import_clause (named_imports (import_specifier name: (identifier) @source_name alias: (identifier) @alias_name))))
    ";
    if let Ok(query) = Query::new(language, q_import_alias) {
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
            if !alias_name.is_empty() && !source_name.is_empty() {
                bindings.push(graph::SemanticBinding {
                    kind: graph::SemanticBindingKind::ImportAlias,
                    name: alias_name,
                    type_name: source_name,
                    context: None,
                });
            }
        }
    }

    bindings
}

#[cfg(test)]
mod js_visitor_tests {
    use super::*;
    use crate::discovery::collector::RelationshipCollector;

    fn parse_and_collect(src: &str) -> Vec<graph::RelationshipNode> {
        let language = tree_sitter_javascript::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut collector = RelationshipCollector::new();
        JavaScriptVisitor.visit(&tree, src, &mut collector);
        collector.into_relationship_nodes()
    }

    #[test]
    fn named_import_produces_import_edge() {
        let src = r#"import { helper } from "./utils";"#;
        let rels = parse_and_collect(src);
        let edge = rels.iter().find(|r| r.name == "helper" && r.kind.as_deref() == Some("imports"));
        assert!(edge.is_some(), "import edge expected; got {rels:?}");
        assert_eq!(edge.unwrap().source.as_deref(), Some("./utils"));
    }

    #[test]
    fn free_call_produces_calls_edge() {
        let src = "helper();";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "helper" && r.kind.as_deref() == Some("calls")));
    }

    #[test]
    fn member_call_produces_method_call_edge() {
        let src = "query.deleteRoom();";
        let rels = parse_and_collect(src);
        assert!(
            rels.iter().any(|r| r.name == "deleteRoom" && r.kind.as_deref() == Some("method_call")),
            "member call should produce method_call; got {rels:?}"
        );
    }

    #[test]
    fn extends_produces_extends_edge() {
        let src = "class Child extends Parent {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Parent" && r.kind.as_deref() == Some("extends")));
    }

    #[test]
    fn new_expression_produces_new_call_edge() {
        let src = "const x = new MyClass();";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "MyClass" && r.kind.as_deref() == Some("new_call")));
    }
    
    #[test]
    fn dump_import_alias_ast() {
        let src = "import { Foo as Bar } from './foo';";
        let language = tree_sitter_javascript::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(src, None).unwrap();
        println!("AST_DUMP: {}", tree.root_node().to_sexp());
    }
}
