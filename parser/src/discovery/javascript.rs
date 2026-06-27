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
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut source = String::new();
        let mut kind = RelationshipKind::Import;
        let mut line = 0usize;

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
                    "source" => source = text,
                    _ => {}
                }
            }
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
}
