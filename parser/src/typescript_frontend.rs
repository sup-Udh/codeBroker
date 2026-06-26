use crate::frontend::LanguageFrontend;
use graph::{ImportNode, SymbolNode};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator, Tree};

pub struct TypeScriptFrontend;

impl LanguageFrontend for TypeScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".ts")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<ImportNode>,
    )> {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(source_code, None)?;

        let metadata = graph::models::FileMetadata { metadata: None };

        let symbols = extract_ts_symbols(&tree, source_code, language.clone(), path);
        let imports = extract_ts_imports(&tree, source_code, language, false);

        Some((metadata, symbols, imports))
    }
}

pub struct TsxFrontend;

impl LanguageFrontend for TsxFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".tsx")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<ImportNode>,
    )> {
        let language = tree_sitter_typescript::LANGUAGE_TSX.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;

        let tree = parser.parse(source_code, None)?;

        let metadata = graph::models::FileMetadata { metadata: None };

        let symbols = extract_ts_symbols(&tree, source_code, language.clone(), path);
        let imports = extract_ts_imports(&tree, source_code, language, true);

        Some((metadata, symbols, imports))
    }
}

fn extract_ts_symbols(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
    path: &str,
) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let filename = std::path::Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default();
    let is_tsx = path.ends_with(".tsx");
    let mut query_str = String::from(
        "
        (class_declaration name: (type_identifier) @type)
        (interface_declaration name: (type_identifier) @type)
        (function_declaration 
            name: (identifier) @function
            parameters: (formal_parameters 
                (required_parameter type: (type_annotation (type_identifier) @prop_type))
            )?
        )
        (lexical_declaration 
            (variable_declarator 
                name: (identifier) @function 
                value: (arrow_function 
                    parameters: (formal_parameters 
                        (required_parameter type: (type_annotation (type_identifier) @prop_type))
                    )?
                )
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
                value: [(array) (object) (string) (template_string) (number)]
            )
        )
    ",
    );

    if is_tsx {
        query_str.push_str(
            "
        (return_statement (jsx_element) @jsx_render)
        (return_statement (parenthesized_expression (jsx_element) @jsx_render))
        ",
        );
    }

    let query = Query::new(&language, &query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut symbol_name = String::new();
        let mut symbol_kind = String::new();
        let mut parent_node = None;
        let mut main_node = None;

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "function"
                    || *capture_kind == "type"
                    || *capture_kind == "data_const"
                {
                    symbol_name = text.to_string();
                    symbol_kind = capture_kind.to_string();
                    main_node = Some(node);
                    parent_node = Some(node.parent().unwrap_or(node));
                }
            }
        }

        if let (Some(mut name_str), Some(_node), Some(parent)) = (
            Some(symbol_name).filter(|s| !s.is_empty()),
            main_node,
            parent_node,
        ) {
            let mut kind = symbol_kind;

            if kind == "jsx_render" {
                name_str = "render".to_string();
                kind = "jsx_element".to_string();
            }

            let mut parent = parent;

            // The node that actually owns a "body" field (function/class/interface/arrow),
            // captured before `parent` gets widened to its lexical_declaration/export_statement.
            let decl_node = if parent.kind() == "variable_declarator" {
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

            // Only exported data constants (e.g. `export const SUPPORTED_LANGUAGES = [...]`)
            // are indexed as symbols. Without this, every local array/object/string
            // literal assignment anywhere in the file — including inside function
            // bodies — would flood the symbol table. Exported ones are the ones a
            // caller actually needs a graph node for, e.g. so `explore_graph` can
            // find UI components that import and render from a shared data constant.
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

            // Build a one-line signature: everything from the start of this
            // declaration up to the opening of its body, e.g.
            // "export async function GET(request: Request)". Falls back to
            // None (skeleton view will fall back to the bare name) when there
            // is no body field to anchor on (e.g. interfaces with no methods).
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
                end_line: parent.end_position().row + 1,
                start_byte: parent.start_byte(),
                end_byte: parent.end_byte(),
                signature,
                attributes: Vec::new(),
                metadata: None,
            });
        }
    }
    symbols
}

fn extract_ts_imports(
    tree: &Tree,
    source_code: &str,
    language: tree_sitter::Language,
    is_tsx: bool,
) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    let mut query_str = String::from(
        "
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
        (extends_clause value: (identifier) @extends_class)
        (extends_clause value: (type_identifier) @extends_class)
        (implements_clause (type_identifier) @implements_type)
    ",
    );

    let query = match Query::new(&language, &query_str) {
        Ok(q) => q,
        Err(_) => return imports, // Fallback gracefully if query fails
    };
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
                } else if *capture_kind == "implements_type" {
                    let name = text.trim().to_string();
                    if !name.is_empty() {
                        import_name = name.clone();
                        import_kind = "implements".to_string();
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
            imports.push(ImportNode {
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
mod data_const_tests {
    use super::*;
    use crate::frontend::LanguageFrontend;

    // Regression: an exported array/object data constant (e.g.
    // `export const SUPPORTED_LANGUAGES = [...]`) must become a real symbol
    // so consumers importing it generate graph edges. Previously the symbol
    // query only matched arrow_function/call_expression values, so plain
    // data literals were invisible to the graph entirely.
    #[test]
    fn exported_array_const_is_indexed_as_constant() {
        let src = r#"
            export const SUPPORTED_LANGUAGES = ["python", "kotlin"];
            const localOnly = ["nope"];
        "#;
        let symbols = extract_ts_symbols(
            &{
                let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
                let mut parser = Parser::new();
                parser.set_language(&language).unwrap();
                parser.parse(src, None).unwrap()
            },
            src,
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "languages.ts",
        );

        let sym = symbols
            .iter()
            .find(|s| s.name == "SUPPORTED_LANGUAGES")
            .expect("exported data constant should be indexed");
        assert_eq!(sym.kind, "constant");

        assert!(
            symbols.iter().all(|s| s.name != "localOnly"),
            "non-exported local data constant must not be indexed"
        );
    }

    #[test]
    fn smoke_test_via_frontend() {
        let src = "export const FOO = [1, 2, 3];";
        let (_meta, symbols, _imports) = TypeScriptFrontend
            .parse_and_extract(src, "foo.ts")
            .expect("parse should succeed");
        assert!(
            symbols
                .iter()
                .any(|s| s.name == "FOO" && s.kind == "constant")
        );
    }
}
