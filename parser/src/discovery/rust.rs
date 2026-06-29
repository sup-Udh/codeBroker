use super::collector::RelationshipCollector;
use super::relationship::{Relationship, RelationshipKind};
use super::visitor::LanguageVisitor;
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

pub struct RustVisitor;

impl LanguageVisitor for RustVisitor {
    fn visit(&self, tree: &Tree, source_code: &str, collector: &mut RelationshipCollector) {
        let language = tree_sitter_rust::LANGUAGE.into();

        emit_imports(tree, source_code, &language, collector);
        emit_calls(tree, source_code, &language, collector);
        emit_impl_relationships(tree, source_code, &language, collector);
        emit_type_refs(tree, source_code, &language, collector);
        emit_attributes(tree, source_code, &language, collector);
        emit_generic_constraints(tree, source_code, &language, collector);
    }

    fn visit_semantic(&self, tree: &Tree, source_code: &str) -> Vec<graph::SemanticBinding> {
        let language = tree_sitter_rust::LANGUAGE.into();
        let mut bindings = Vec::new();

        // ── Import alias: use foo::Bar as Baz ────────────────────────
        let q_import_alias = "
            (use_as_clause path: (_) @source_name alias: (identifier) @alias_name)
        ";
        if let Ok(query) = Query::new(&language, q_import_alias) {
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
                            "source_name" => {
                                // Extract just the leaf name from path (e.g., `foo::Bar` -> `Bar`)
                                let raw = text.trim();
                                if let Some(last) = raw.split("::").last() {
                                    source_name = last.trim().to_string();
                                }
                            }
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
}

fn emit_imports(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    let query_str = "(use_declaration argument: (_) @use_path)";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(raw) = capture.node.utf8_text(source_code.as_bytes()) {
                let line = capture.node.start_position().row + 1;
                collect_use_leaves(raw.trim(), line, None, collector);
            }
        }
    }
}

/// Recursively extracts leaf names from a use path, emitting one Import per leaf.
/// Preserves the module path as `source` so the resolution pipeline can classify
/// stdlib vs external vs crate-local imports without guessing from name alone.
fn collect_use_leaves(
    raw: &str,
    line: usize,
    prefix: Option<&str>,
    collector: &mut RelationshipCollector,
) {
    let raw = raw.trim();
    if matches!(raw, "*" | "self" | "super" | "crate") {
        return;
    }

    if let Some(brace_start) = raw.find('{') {
        let outer_prefix = raw[..brace_start].trim_end_matches(':').trim();
        let combined = match (prefix, outer_prefix.is_empty()) {
            (Some(p), false) => format!("{}::{}", p, outer_prefix),
            (Some(p), true) => p.to_string(),
            (None, false) => outer_prefix.to_string(),
            (None, true) => String::new(),
        };
        let inner = &raw[brace_start + 1..];
        let brace_end = inner.rfind('}').unwrap_or(inner.len());
        let inner = &inner[..brace_end];
        for item in inner.split(',') {
            let item = item.trim();
            if !item.is_empty() {
                collect_use_leaves(
                    item,
                    line,
                    if combined.is_empty() { None } else { Some(&combined) },
                    collector,
                );
            }
        }
        return;
    }

    // Strip `as` renames: `HashMap as Map` → work with `HashMap`
    let name = if let Some(pos) = raw.find(" as ") { &raw[..pos] } else { raw };

    // Split into path segments; leaf is the last one
    let segments: Vec<&str> = name.split("::").filter(|s| !s.is_empty()).collect();
    let (leaf, source_segs) = match segments.split_last() {
        Some(pair) => pair,
        None => return,
    };
    let leaf = leaf.trim();
    if leaf.is_empty() || matches!(leaf, "*" | "self" | "super" | "crate") {
        return;
    }
    // Filter single-char generics (T, K, V …)
    if leaf.len() == 1 {
        return;
    }

    // Build the parent-module path as source
    let source: Option<String> = {
        let from_name = if source_segs.is_empty() {
            None
        } else {
            Some(source_segs.join("::"))
        };
        match (prefix, from_name) {
            (Some(p), Some(s)) => Some(format!("{}::{}", p, s)),
            (Some(p), None) => Some(p.to_string()),
            (None, Some(s)) => Some(s),
            (None, None) => None,
        }
    };

    let rel = Relationship::new(leaf.to_string(), RelationshipKind::Import, line);
    let rel = if let Some(src) = source { rel.with_source(src) } else { rel };
    collector.emit(rel);
}

fn emit_calls(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // ---- Receiver-aware method calls (emitted first for dedup priority) ----
    // Captures: receiver.method()  where receiver is a simple identifier
    let q_recv = "(call_expression function: (field_expression value: (identifier) @receiver field: (field_identifier) @method))";
    if let Ok(query) = Query::new(language, q_recv) {
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
            if !method.is_empty() && !is_noisy_rust_call(&method) {
                let rel = Relationship::new(method, RelationshipKind::MethodCall, line);
                let rel = if !receiver.is_empty() { rel.with_source(receiver) } else { rel };
                collector.emit(rel);
            }
        }
    }

    // ---- Free calls + fallback method calls (deduplicated against receiver-aware) ----
    let query_str = "
        (call_expression function: (identifier) @call_name)
        (call_expression function: (field_expression field: (field_identifier) @method_call))
    ";
    let query = Query::new(language, query_str).expect("invalid query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if *capture_name == "call_name" {
                    if !is_noisy_rust_call(&name) {
                        collector.emit(Relationship::new(name, RelationshipKind::Call, line));
                    }
                } else if *capture_name == "method_call" {
                    if !is_noisy_rust_call(&name) {
                        // No source here; deduplicated if already emitted with receiver above
                        collector.emit(Relationship::new(name, RelationshipKind::MethodCall, line));
                    }
                }
            }
        }
    }
}

fn emit_impl_relationships(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // impl Trait for Type → Implements(Trait) + Inherits(Type)
    let query_str = "
        (impl_item trait: (type_identifier) @impl_trait)
        (impl_item trait: (scoped_type_identifier name: (type_identifier) @impl_trait))
        (impl_item trait: (generic_type type: (type_identifier) @impl_trait))
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
                    collector.emit(Relationship::new(
                        name,
                        RelationshipKind::Implements,
                        line,
                    ));
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
    // Capture type identifiers appearing in struct fields, function params,
    // return types, and type aliases. In tree-sitter-rust, field_declaration
    // stores the type as an anonymous child (no named "type:" field), so we
    // match it without a field label.
    let query_str = "
        (field_declaration (type_identifier) @type_ref)
        (field_declaration (generic_type type: (type_identifier) @type_ref))
        (field_declaration (scoped_type_identifier name: (type_identifier) @type_ref))
        (field_declaration (reference_type (type_identifier) @type_ref))
        (parameter type: (type_identifier) @type_ref)
        (parameter type: (generic_type type: (type_identifier) @type_ref))
        (parameter type: (reference_type (type_identifier) @type_ref))
        (function_item return_type: (type_identifier) @type_ref)
        (function_item return_type: (generic_type type: (type_identifier) @type_ref))
        (type_item type: (type_identifier) @type_ref)
        (let_declaration type: (type_identifier) @type_ref)
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
                if !name.is_empty() && !is_primitive_type(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::TypeRef, line));
                }
            }
        }
    }
}

fn emit_attributes(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // Rust attributes: #[derive(Debug, Clone)] → Annotation(Debug), Annotation(Clone)
    // #[test] → Annotation(test)
    let query_str = "
        (attribute (identifier) @attr_name)
        (attribute arguments: (token_tree (identifier) @derive_arg))
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
                if !name.is_empty() && !is_builtin_attribute(&name) {
                    collector.emit(Relationship::new(name, RelationshipKind::Annotation, line));
                }
            }
        }
    }
}

fn emit_generic_constraints(
    tree: &Tree,
    source_code: &str,
    language: &tree_sitter::Language,
    collector: &mut RelationshipCollector,
) {
    // T: Clone + Debug → GenericConstraint(Clone), GenericConstraint(Debug)
    // tree-sitter-rust uses `trait_bounds` containing `type_identifier` for bounds.
    // The `constrained_type_parameter` node wraps `T: Bound` in type_parameters.
    let query_str = "
        (where_clause (where_predicate bounds: (trait_bounds (type_identifier) @bound)))
        (where_clause (where_predicate bounds: (trait_bounds (scoped_type_identifier name: (type_identifier) @bound))))
    ";
    let query = match Query::new(language, query_str) {
        Ok(q) => q,
        Err(_) => return,
    };
    // Also check inline type parameter bounds via a separate simpler query
    let inline_query_str = "(trait_bounds (type_identifier) @bound)";
    if let Ok(inline_q) = Query::new(language, inline_query_str) {
        let mut ic = QueryCursor::new();
        let mut im = ic.matches(&inline_q, tree.root_node(), source_code.as_bytes());
        while let Some(m) = im.next() {
            for capture in m.captures {
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    let name = text.trim().to_string();
                    let line = capture.node.start_position().row + 1;
                    if !name.is_empty() && !is_primitive_trait(&name) {
                        collector.emit(Relationship::new(
                            name,
                            RelationshipKind::GenericConstraint,
                            line,
                        ));
                    }
                }
            }
        }
    }
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                let name = text.trim().to_string();
                let line = capture.node.start_position().row + 1;
                if !name.is_empty() && !is_primitive_trait(&name) {
                    collector.emit(Relationship::new(
                        name,
                        RelationshipKind::GenericConstraint,
                        line,
                    ));
                }
            }
        }
    }
}

fn is_noisy_rust_call(name: &str) -> bool {
    matches!(
        name,
        // Macros / panic
        "println" | "eprintln" | "print" | "eprint" | "format" | "vec" | "panic"
        | "assert" | "assert_eq" | "assert_ne" | "debug_assert" | "todo"
        | "unimplemented" | "unreachable"
        // Option / Result combinators
        | "unwrap" | "expect" | "ok" | "err" | "ok_or" | "ok_or_else"
        | "map" | "map_err" | "and_then" | "or_else" | "or" | "flatten"
        | "unwrap_or" | "unwrap_or_else" | "unwrap_or_default"
        // Iterator adapters
        | "filter" | "collect" | "iter" | "into_iter" | "iter_mut"
        | "enumerate" | "zip" | "chain" | "take" | "skip" | "peekable"
        | "flat_map" | "filter_map" | "fold" | "reduce" | "find" | "find_map"
        | "any" | "all" | "count" | "sum" | "product" | "min" | "max"
        | "min_by" | "max_by" | "min_by_key" | "max_by_key" | "cloned" | "copied"
        | "rev" | "inspect" | "scan" | "position" | "index" | "step_by"
        // String / str methods
        | "clone" | "to_string" | "to_owned" | "trim" | "trim_start" | "trim_end"
        | "split" | "split_once" | "splitn" | "starts_with" | "ends_with"
        | "contains" | "replace" | "replacen" | "chars" | "bytes" | "lines"
        | "as_str" | "as_bytes" | "push_str" | "push" | "pop" | "truncate"
        | "to_uppercase" | "to_lowercase" | "repeat" | "is_ascii"
        | "strip_prefix" | "strip_suffix"
        // Vec / slice methods
        | "len" | "is_empty" | "first" | "last" | "get" | "get_mut"
        | "insert" | "remove" | "retain" | "dedup" | "dedup_by" | "dedup_by_key"
        | "drain" | "extend" | "extend_from_slice" | "append" | "resize"
        | "capacity" | "reserve" | "reserve_exact" | "shrink_to_fit" | "shrink_to"
        | "swap" | "sort" | "sort_by" | "sort_by_key" | "sort_unstable"
        | "sort_unstable_by" | "sort_unstable_by_key" | "windows" | "chunks"
        | "chunks_exact" | "split_at" | "contains" | "iter" | "concat" | "join"
        // HashMap / HashSet
        | "entry" | "or_insert" | "or_insert_with" | "or_default" | "and_modify"
        | "keys" | "values" | "values_mut" | "into_keys" | "into_values"
        // Conversions / misc
        | "default" | "new" | "from" | "into" | "as_ref" | "as_mut" | "borrow"
        | "borrow_mut" | "take" | "replace" | "drop" | "parse"
        | "to_vec" | "to_slice" | "to_bytes" | "into_bytes" | "into_string"
        | "into_os_string" | "as_os_str" | "to_path_buf"
        // Ordering / comparison
        | "cmp" | "partial_cmp" | "eq" | "ne" | "lt" | "le" | "gt" | "ge"
        | "clamp"
        // Path / fs methods
        | "parent" | "file_name" | "extension" | "file_stem"
        | "display" | "exists" | "is_file" | "is_dir" | "canonicalize"
        | "read_to_string" | "write_all" | "flush"
        | "metadata" | "read_dir"
        // Tree-sitter node methods (external crate — never repo symbols)
        | "root_node" | "utf8_text" | "start_position" | "end_position"
        | "start_byte" | "end_byte" | "child_count" | "named_child_count"
        | "child" | "named_child" | "children" | "named_children" | "walk"
        | "kind" | "is_named" | "is_missing" | "has_error" | "error_node"
        | "capture_names" | "node_type_name" | "next" | "prev"
        // rusqlite / SQL methods (external crate — never repo symbols)
        | "query_row" | "query_map" | "prepare" | "query" | "execute_batch"
        | "last_insert_rowid" | "changes" | "transaction"
        // serde_json / JSON methods
        | "as_object" | "as_array" | "as_f64" | "as_i64" | "as_u64"
        | "as_bool" | "is_null" | "is_object" | "is_array"
        // Duration / Instant / time methods
        | "elapsed" | "as_millis" | "as_secs" | "as_secs_f64" | "as_nanos"
        | "duration_since" | "saturating_sub" | "saturating_add"
        // Path / OsStr conversion methods
        | "to_str" | "to_string_lossy" | "as_path"
        // Arc / Rc / Box / Cell / Mutex
        | "lock" | "try_lock" | "upgrade" | "downgrade" | "strong_count" | "weak_count"
        // Regex
        | "captures_iter" | "is_match" | "captures" | "find_iter"
    )
}

fn is_primitive_type(name: &str) -> bool {
    matches!(
        name,
        "bool" | "u8" | "u16" | "u32" | "u64" | "u128" | "usize"
        | "i8" | "i16" | "i32" | "i64" | "i128" | "isize"
        | "f32" | "f64" | "char" | "str" | "String" | "Option"
        | "Result" | "Vec" | "Box" | "Rc" | "Arc" | "Cell" | "RefCell"
        | "HashMap" | "BTreeMap" | "HashSet" | "BTreeSet" | "Self"
    )
}

fn is_primitive_trait(name: &str) -> bool {
    matches!(
        name,
        "Clone" | "Copy" | "Debug" | "Display" | "Default" | "PartialEq"
        | "Eq" | "PartialOrd" | "Ord" | "Hash" | "Send" | "Sync"
        | "Sized" | "Unpin" | "Into" | "From" | "AsRef" | "AsMut"
        | "Iterator" | "IntoIterator" | "FromIterator" | "Extend"
        | "Fn" | "FnMut" | "FnOnce" | "Drop" | "Future" | "Stream"
    )
}

fn is_builtin_attribute(name: &str) -> bool {
    matches!(
        name,
        "cfg" | "allow" | "warn" | "deny" | "forbid" | "test" | "ignore"
        | "should_panic" | "bench" | "inline" | "cold" | "track_caller"
        | "repr" | "non_exhaustive" | "must_use" | "deprecated"
        | "automatically_derived" | "doc" | "macro_export" | "macro_use"
        | "path" | "recursion_limit" | "feature" | "link" | "global_allocator"
        | "panic_handler" | "proc_macro" | "proc_macro_attribute"
        | "proc_macro_derive" | "no_std" | "no_main" | "windows_subsystem"
    )
}

#[cfg(test)]
mod rust_visitor_tests {
    use super::*;
    use crate::discovery::collector::RelationshipCollector;

    fn parse_and_collect(src: &str) -> Vec<graph::RelationshipNode> {
        let language = tree_sitter_rust::LANGUAGE.into();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(&language).unwrap();
        let tree = parser.parse(src, None).unwrap();
        let mut collector = RelationshipCollector::new();
        RustVisitor.visit(&tree, src, &mut collector);
        collector.into_relationship_nodes()
    }

    #[test]
    fn use_import_extracted() {
        let src = "use std::collections::HashMap;";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "HashMap" && r.kind.as_deref() == Some("imports")));
    }

    #[test]
    fn brace_use_group_extracts_both_leaves() {
        let src = "use std::collections::{HashMap, BTreeMap};";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "HashMap"));
        assert!(rels.iter().any(|r| r.name == "BTreeMap"));
    }

    #[test]
    fn impl_trait_produces_implements() {
        let src = "trait Foo {} struct Bar; impl Foo for Bar {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Foo" && r.kind.as_deref() == Some("implements")));
    }

    #[test]
    fn derive_attribute_produces_annotation() {
        let src = "#[derive(Debug, Clone)] struct Foo {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Debug" && r.kind.as_deref() == Some("annotation")));
        assert!(rels.iter().any(|r| r.name == "Clone" && r.kind.as_deref() == Some("annotation")));
    }

    #[test]
    fn field_type_produces_type_ref() {
        let src = "struct Foo { bar: MyType, }";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "MyType" && r.kind.as_deref() == Some("type_ref")));
    }

    #[test]
    fn generic_constraint_produces_edge() {
        let src = "fn foo<T: Serialize>(x: T) {}";
        let rels = parse_and_collect(src);
        assert!(rels.iter().any(|r| r.name == "Serialize" && r.kind.as_deref() == Some("generic_constraint")));
    }

    #[test]
    fn no_duplicate_relationships_for_same_node() {
        let src = "use std::io::Write;";
        let rels = parse_and_collect(src);
        let write_count = rels.iter().filter(|r| r.name == "Write").count();
        assert_eq!(write_count, 1, "Write must appear exactly once, got {write_count}");
    }

}
