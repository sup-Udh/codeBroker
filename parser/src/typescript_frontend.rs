use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};
use tree_sitter::{Parser, Query, QueryCursor, Tree, StreamingIterator};

pub struct TypeScriptFrontend;

impl LanguageFrontend for TypeScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".ts")
    }

    fn parse_and_extract(&self, source_code: &str, path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
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

    fn parse_and_extract(&self, source_code: &str, path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
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

fn extract_ts_symbols(tree: &Tree, source_code: &str, language: tree_sitter::Language, path: &str) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let filename = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let is_tsx = path.ends_with(".tsx");
    let mut query_str = String::from("
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
    ");

    if is_tsx {
        query_str.push_str("
        (return_statement (jsx_element) @jsx_render)
        (return_statement (parenthesized_expression (jsx_element) @jsx_render))
        ");
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
                if *capture_kind == "function" || *capture_kind == "type" {
                    symbol_name = text.to_string();
                    symbol_kind = capture_kind.to_string();
                    main_node = Some(node);
                    parent_node = Some(node.parent().unwrap_or(node));
                } else if *capture_kind == "prop_type" {
                    prop_type = Some(text.to_string());
                }
            }
        }

        if let (Some(mut name_str), Some(node), Some(parent)) = (Some(symbol_name).filter(|s| !s.is_empty()), main_node, parent_node) {
            let mut kind = symbol_kind;

            if kind == "jsx_render" {
                name_str = "render".to_string();
                kind = "jsx_element".to_string();
            } else if kind == "function" {
                if name_str.starts_with("use") {
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
                if p.kind() == "program" { break; }
                current = p;
            }

            if is_call_expr_assignment && kind == "function" && !is_exported {
                continue; // Skip indexing this local generic variable
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

            symbols.push(SymbolNode {
                name: name_str,
                kind,
                prop_type,
                start_line: parent.start_position().row + 1,
                end_line: parent.end_position().row + 1,
                start_byte: parent.start_byte(),
                end_byte: parent.end_byte(),
                signature: None,
            });
        }
    }
    symbols
}

fn extract_ts_imports(tree: &Tree, source_code: &str, language: tree_sitter::Language, is_tsx: bool) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    let mut query_str = String::from("
        (import_statement 
            (import_clause (named_imports (import_specifier name: (identifier) @import)))
            source: (string (string_fragment) @source)
        )
        (import_statement
            (import_clause (identifier) @import)
            source: (string (string_fragment) @source)
        )
        (call_expression function: (identifier) @call_name)
        (call_expression function: (member_expression property: (property_identifier) @call_name))
        (string (string_fragment) @route_string)
    ");
    
    if is_tsx {
        query_str.push_str("
        (jsx_opening_element (identifier) @jsx_element)
        (jsx_self_closing_element (identifier) @jsx_element)
        (jsx_expression (identifier) @call_name)
        (jsx_expression (member_expression property: (property_identifier) @call_name))
        ");
    }
    
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
                } else if *capture_kind == "route_string" {
                    let val = text.trim().to_string();
                    if val.starts_with('/') {
                        import_name = val;
                        import_kind = "route_push".to_string();
                        line_number = node.start_position().row + 1;
                    }
                }
            }
        }

        if !import_name.is_empty() {
            imports.push(ImportNode {
                name: import_name,
                source: if import_source.is_empty() { None } else { Some(import_source) },
                line_number,
                kind: Some(import_kind),
            });
        }
    }
    imports
}
