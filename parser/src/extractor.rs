use graph::RelationshipNode;
use graph::SymbolNode;
use tree_sitter::{Query, QueryCursor, StreamingIterator, Tree};

/// Extracts top-level Rust symbols (functions, structs, enums, traits,
/// type aliases, constants, statics, macros, and modules) from a parsed
/// tree. Only items that are directly children of the module root or
/// declared inside a `pub mod` block are extracted; nested/local items are
/// intentionally skipped because they are not independently reachable as
/// graph nodes.
pub fn extract_symbols(tree: &Tree, source_code: &str) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();

    let query_str = "
        (function_item name: (identifier) @function)
        (struct_item name: (type_identifier) @struct)
        (enum_item name: (type_identifier) @enum)
        (trait_item name: (type_identifier) @trait)
        (type_item name: (type_identifier) @type_alias)
        (const_item name: (identifier) @constant)
        (static_item name: (identifier) @constant)
        (macro_definition name: (identifier) @macro)
        (mod_item name: (identifier) @module)
    ";

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let parent = node.parent().unwrap_or(node);
                symbols.push(SymbolNode {
                    name: name.to_string(),
                    kind: capture_kind.to_string(),
                    start_line: node.start_position().row + 1,
                    end_line: parent.end_position().row + 1,
                    start_byte: parent.start_byte(),
                    end_byte: parent.end_byte(),
                    signature: None,
                    attributes: Vec::new(),
                    metadata: None,
                });
            }
        }
    }

    symbols
}

/// Extracts Rust edges (imports via `use`, function calls, trait
/// implementations, and `impl … for …` relationships) from a parsed tree.
///
/// `use` declarations: only the **leaf** name is extracted, not the full
/// path, because the linker resolves by symbol name, not by module path.
/// `use std::collections::HashMap;` → RelationshipNode { name: "HashMap", … }.
/// Glob imports (`use foo::*;`) produce no import node since `*` cannot
/// resolve to a specific symbol.
///
/// Call expressions: `foo(args)` produces a `calls` edge to `foo`. Method
/// calls (`obj.method()`) are NOT extracted here — without type resolution
/// the receiver type is unknown, so global fallback matching would fabricate
/// phantom edges.
///
/// `impl Trait for Type`: produces an `implements` edge from the `impl` block's
/// source file to the trait name, and an `inherits` edge to the self type.
pub fn extract_imports(tree: &Tree, source_code: &str) -> Vec<RelationshipNode> {
    let mut imports = Vec::new();

    let query_str = "
        (use_declaration argument: (_) @use_path)
        (call_expression function: (identifier) @call_name)
        (impl_item trait: (type_identifier) @impl_trait)
        (impl_item trait: (scoped_type_identifier name: (type_identifier) @impl_trait))
    ";

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];

            if let Ok(raw) = node.utf8_text(source_code.as_bytes()) {
                let raw = raw.trim();

                match *capture_kind {
                    "use_path" => {
                        // Extract only the leaf identifiers from the use path.
                        // `std::collections::HashMap` → ["HashMap"]
                        // `std::collections::{HashMap, BTreeMap}` → ["HashMap", "BTreeMap"]
                        // `crate::*` / `super::*` / `self::*` → skipped (glob)
                        extract_use_leaf_names(raw, node.start_position().row + 1, &mut imports);
                    }
                    "call_name" => {
                        let name = raw.to_string();
                        if !is_noisy_rust_call(&name) {
                            imports.push(RelationshipNode {
                                name,
                                source: None,
                                line_number: node.start_position().row + 1,
                                kind: Some("calls".to_string()),
                            });
                        }
                    }
                    "impl_trait" => {
                        let name = raw.to_string();
                        if !name.is_empty() {
                            imports.push(RelationshipNode {
                                name,
                                source: None,
                                line_number: node.start_position().row + 1,
                                kind: Some("implements".to_string()),
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    imports
}

/// Walks a Rust use-path text and pushes one `RelationshipNode` per leaf name.
/// Examples:
/// - `"std::collections::HashMap"` → pushes `"HashMap"`
/// - `"{HashMap, BTreeMap}"` → pushes both (the caller handles the outer path)
/// - `"*"` → skipped (glob, unresolvable)
/// - `"self"` / `"super"` / `"crate"` → skipped (relative self-referential)
fn extract_use_leaf_names(raw: &str, line: usize, out: &mut Vec<RelationshipNode>) {
    let raw = raw.trim();
    if raw == "*" || raw == "self" || raw == "super" || raw == "crate" {
        return;
    }

    // Brace group: `{HashMap, BTreeMap, Entry}` or a nested path ending in braces.
    if let Some(brace_start) = raw.find('{') {
        let inner = &raw[brace_start + 1..];
        let brace_end = inner.rfind('}').map(|i| i).unwrap_or(inner.len());
        let inner = &inner[..brace_end];
        for item in inner.split(',') {
            let item = item.trim();
            if item.is_empty() {
                continue;
            }
            // Each item may itself be a path (`a::b`) or a rename (`HashMap as HM`).
            extract_use_leaf_names(item, line, out);
        }
        return;
    }

    // Handle `as` renames: `HashMap as Map` → import `HashMap`, the local alias
    // is not a symbol defined in the target, so we link to the source name.
    let name = if let Some(pos) = raw.find(" as ") {
        &raw[..pos]
    } else {
        raw
    };

    // Extract the last path component: `std::collections::HashMap` → `HashMap`.
    let leaf = name.split("::").last().unwrap_or(name).trim();

    if leaf.is_empty() || leaf == "*" || leaf == "self" || leaf == "super" || leaf == "crate" {
        return;
    }

    // Skip all-lowercase single-character identifiers — they're almost always
    // generic type parameters (`T`, `E`, `K`, `V`) captured by the query on
    // scoped paths inside `impl` blocks, not real symbol references.
    if leaf.len() == 1 {
        return;
    }

    out.push(RelationshipNode {
        name: leaf.to_string(),
        source: None,
        line_number: line,
        kind: None, // resolves to "imports" by default
    });
}

/// Rust built-in and standard-library call names that appear so frequently
/// they would create phantom edges if linked globally. Subset of the
/// cross-language noisy-call list in `utils.rs`, plus Rust-specific names.
fn is_noisy_rust_call(name: &str) -> bool {
    matches!(
        name,
        "println"
            | "eprintln"
            | "print"
            | "eprint"
            | "format"
            | "vec"
            | "panic"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "debug_assert"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "unwrap"
            | "expect"
            | "ok"
            | "err"
            | "map"
            | "and_then"
            | "or_else"
            | "filter"
            | "collect"
            | "iter"
            | "into_iter"
            | "clone"
            | "to_string"
            | "to_owned"
            | "len"
            | "is_empty"
            | "push"
            | "pop"
            | "insert"
            | "remove"
            | "get"
            | "contains"
            | "extend"
            | "default"
            | "new"
    )
}

#[cfg(test)]
mod extractor_tests {
    use crate::frontend::LanguageFrontend;
    use crate::frontend::RustFrontend;

    #[test]
    fn use_leaf_extracted_not_full_path() {
        let src = "use std::collections::HashMap;";
        let (_m, _syms, imports, _) = RustFrontend.parse_and_extract(src, "a.rs").unwrap();
        let names: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
        assert!(
            names.contains(&"HashMap"),
            "leaf 'HashMap' must be indexed, got {names:?}"
        );
        assert!(
            !names.iter().any(|n| n.contains("::")),
            "full path must not appear in import names, got {names:?}"
        );
    }

    #[test]
    fn brace_group_yields_individual_leaves() {
        let src = "use std::collections::{HashMap, BTreeMap};";
        let (_m, _syms, imports, _) = RustFrontend.parse_and_extract(src, "a.rs").unwrap();
        let names: Vec<&str> = imports.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"HashMap"), "HashMap missing from {names:?}");
        assert!(
            names.contains(&"BTreeMap"),
            "BTreeMap missing from {names:?}"
        );
    }

    #[test]
    fn glob_import_produces_no_node() {
        let src = "use std::io::*;";
        let (_m, _syms, imports, _) = RustFrontend.parse_and_extract(src, "a.rs").unwrap();
        assert!(
            imports.iter().all(|i| i.name != "*"),
            "glob must not produce an import node"
        );
    }

    #[test]
    fn enum_and_trait_are_indexed() {
        let src = r#"
            enum Color { Red, Green, Blue }
            trait Drawable { fn draw(&self); }
        "#;
        let (_m, syms, _, _) = RustFrontend.parse_and_extract(src, "a.rs").unwrap();
        let kinds: Vec<&str> = syms.iter().map(|s| s.kind.as_str()).collect();
        assert!(
            kinds.contains(&"enum"),
            "enum kind expected, got {kinds:?}"
        );
        assert!(
            kinds.contains(&"trait"),
            "trait kind expected, got {kinds:?}"
        );
    }

    #[test]
    fn impl_trait_produces_implements_edge() {
        let src = r#"
            trait Drawable {}
            struct Circle;
            impl Drawable for Circle {}
        "#;
        let (_m, _syms, imports, _) = RustFrontend.parse_and_extract(src, "a.rs").unwrap();
        let impl_edges: Vec<&str> = imports
            .iter()
            .filter(|i| i.kind.as_deref() == Some("implements"))
            .map(|i| i.name.as_str())
            .collect();
        assert!(
            impl_edges.contains(&"Drawable"),
            "impl Trait for Type must produce an 'implements' edge to the trait, got {impl_edges:?}"
        );
    }
}
