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

        let mut directive = None;
        if source_code.contains("\"use client\"") || source_code.contains("'use client'") {
            directive = Some("use client".to_string());
        } else if source_code.contains("\"use server\"") || source_code.contains("'use server'") {
            directive = Some("use server".to_string());
        }

        let metadata = graph::models::FileMetadata {
            directive,
            ..Default::default()
        };

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

        let mut directive = None;
        if source_code.contains("\"use client\"") || source_code.contains("'use client'") {
            directive = Some("use client".to_string());
        } else if source_code.contains("\"use server\"") || source_code.contains("'use server'") {
            directive = Some("use server".to_string());
        }

        let metadata = graph::models::FileMetadata {
            directive,
            ..Default::default()
        };

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
        let mut prop_type = None;
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
                } else if *capture_kind == "prop_type" {
                    prop_type = Some(text.to_string());
                }
            }
        }

        if let (Some(mut name_str), Some(_node), Some(parent)) = (
            Some(symbol_name).filter(|s| !s.is_empty()),
            main_node,
            parent_node,
        ) {
            let mut kind = symbol_kind;
            let mut route_path = None;
            let mut route_method = None;

            if kind == "jsx_render" {
                name_str = "render".to_string();
                kind = "jsx_element".to_string();
            } else if kind == "function" {
                // Next.js route handlers: a top-level exported GET/POST/etc.
                // in a route.ts(x) file is the literal entrypoint for that
                // API route — subsystem_stats relies on this "route" kind to
                // populate its routes/entrypoints fields.
                const HTTP_METHODS: [&str; 7] =
                    ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
                if filename == "route.ts" || filename == "route.tsx" {
                    if HTTP_METHODS.contains(&name_str.as_str()) {
                        kind = "route".to_string();

                        let mut p = path.to_string();
                        if p.starts_with("./src/app") {
                            p = p.replacen("./src/app", "", 1);
                        } else if p.starts_with("./app") {
                            p = p.replacen("./app", "", 1);
                        } else if p.starts_with("src/app") {
                            p = p.replacen("src/app", "", 1);
                        } else if p.starts_with("app") {
                            p = p.replacen("app", "", 1);
                        }

                        if p.ends_with("/route.ts") {
                            p = p.replacen("/route.ts", "", 1);
                        } else if p.ends_with("/route.tsx") {
                            p = p.replacen("/route.tsx", "", 1);
                        }

                        if p.is_empty() {
                            p = "/".to_string();
                        }
                        if !p.starts_with('/') {
                            p = format!("/{}", p);
                        }

                        route_path = Some(p);
                        route_method = Some(name_str.clone());
                    }
                } else if name_str.starts_with("use") {
                    kind = "hook".to_string();
                } else if name_str.ends_with("Provider") {
                    kind = "provider".to_string();
                } else if is_tsx && name_str.chars().next().unwrap_or('a').is_uppercase() {
                    kind = "component".to_string();
                    if filename == "page.tsx" {
                        kind = "page".to_string();
                    } else if filename == "layout.tsx" {
                        kind = "layout".to_string();
                    }
                }
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
                prop_type,
                start_line: parent.start_position().row + 1,
                end_line: parent.end_position().row + 1,
                start_byte: parent.start_byte(),
                end_byte: parent.end_byte(),
                signature,
                route_path,
                route_method,
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
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @method_call))
        (string (string_fragment) @route_string)
    ",
    );

    if is_tsx {
        query_str.push_str(
            "
        (jsx_opening_element (identifier) @jsx_element)
        (jsx_self_closing_element (identifier) @jsx_element)
        (jsx_expression (identifier) @call_name)
        (jsx_expression (member_expression property: (property_identifier) @method_call))
        ",
        );
    }

    query_str.push_str("
        (call_expression 
            function: [(identifier) @http_fn (member_expression property: (property_identifier) @http_fn)]
            arguments: (arguments [(string (string_fragment) @http_route) (template_string (string_fragment) @http_route)])
        )
        (member_expression property: (property_identifier) @member_access)
    ");

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
                } else if *capture_kind == "jsx_element" {
                    let name = text.trim().to_string();
                    if name.chars().next().unwrap_or('a').is_uppercase() {
                        import_name = name.clone();
                        if name.ends_with("Provider") {
                            import_kind = "renders_provider".to_string();
                        } else {
                            import_kind = "renders_component".to_string();
                        }
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "call_name" {
                    let name = text.trim().to_string();
                    if crate::utils::is_noisy_call_name(&name) {
                        continue;
                    }
                    import_name = name.clone();
                    if name.starts_with("use") {
                        import_kind = "consumes_hook".to_string();
                    } else {
                        import_kind = "calls".to_string();
                    }
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
                } else if *capture_kind == "http_fn" {
                    let fn_name = text.trim().to_string();
                    let is_http = fn_name == "fetch"
                        || fn_name == "get"
                        || fn_name == "post"
                        || fn_name == "put"
                        || fn_name == "delete"
                        || fn_name == "patch"
                        || fn_name == "request";
                    if is_http {
                        if let Some(route_node) = m
                            .captures
                            .iter()
                            .find(|c| query.capture_names()[c.index as usize] == "http_route")
                        {
                            if let Ok(route) = route_node.node.utf8_text(source_code.as_bytes()) {
                                let r = route.trim().to_string();
                                if r.starts_with('/') {
                                    import_name = r.clone();
                                    import_kind = "HTTP_CALL".to_string();
                                    line_number = node.start_position().row + 1;
                                    let m = if fn_name == "fetch" || fn_name == "request" {
                                        "GET"
                                    } else {
                                        &fn_name
                                    };
                                    import_source = m.to_uppercase();
                                }
                            }
                        }
                    }
                } else if *capture_kind == "route_string" {
                    let val = text.trim().to_string();
                    if val.starts_with('/') && import_kind == "imports" {
                        import_name = val;
                        import_kind = "route_push".to_string();
                        line_number = node.start_position().row + 1;
                    }
                }
            }
        }

        if import_kind == "HTTP_CALL" {
            imports.push(ImportNode {
                name: import_name.clone(),
                source: if import_source.is_empty() {
                    None
                } else {
                    Some(import_source.clone())
                },
                line_number,
                kind: Some(import_kind.clone()),
            });
        } else if !import_name.is_empty() && import_kind != "imports" {
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
