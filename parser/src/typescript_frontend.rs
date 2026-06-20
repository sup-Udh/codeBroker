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
    let query_str = "
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
    ";
    
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
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

        if let (Some(name_str), Some(node), Some(parent)) = (Some(symbol_name).filter(|s| !s.is_empty()), main_node, parent_node) {
            let mut kind = symbol_kind;

            if kind == "function" {
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
        (call_expression function: (identifier) @hook_call)
    ");
    
    if is_tsx {
        query_str.push_str("
        (jsx_opening_element (identifier) @jsx_element)
        (jsx_self_closing_element (identifier) @jsx_element)
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
                        import_name = name;
                        import_kind = "renders_component".to_string();
                        line_number = node.start_position().row + 1;
                    }
                } else if *capture_kind == "hook_call" {
                    let name = text.trim().to_string();
                    if name.starts_with("use") {
                        import_name = name;
                        import_kind = "consumes_hook".to_string();
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
