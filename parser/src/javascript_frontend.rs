use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};
use tree_sitter::{Parser, Query, QueryCursor, Tree, StreamingIterator};

pub struct JavaScriptFrontend;

impl LanguageFrontend for JavaScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".js") || path.ends_with(".jsx")
    }

    fn parse_and_extract(&self, source_code: &str, _path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
        let language = tree_sitter_javascript::LANGUAGE.into();
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

        let symbols = extract_js_symbols(&tree, source_code, language.clone(), _path);
        let imports = extract_js_imports(&tree, source_code, language);

        Some((metadata, symbols, imports))
    }
}

fn extract_js_symbols(tree: &Tree, source_code: &str, language: tree_sitter::Language, path: &str) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let filename = std::path::Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or_default();
    let is_jsx = path.ends_with(".jsx");
    let query_str = "
        (class_declaration name: (identifier) @type)
        (function_declaration name: (identifier) @function)
    ";
    
    let language = tree_sitter_javascript::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let name_str = name.to_string();
                let mut kind = capture_kind.to_string();

                if kind == "function" {
                    if name_str.starts_with("use") {
                        kind = "hook".to_string();
                    } else if name_str.ends_with("Provider") {
                        kind = "provider".to_string();
                    } else if is_jsx && name_str.chars().next().unwrap_or('a').is_uppercase() {
                        kind = "component".to_string();
                        if filename == "page.jsx" {
                            kind = "page".to_string();
                        } else if filename == "layout.jsx" {
                            kind = "layout".to_string();
                        }
                    }
                }

                let parent = node.parent().unwrap_or(node);
                let end_line = parent.end_position().row + 1;
                symbols.push(SymbolNode {
                    name: name_str,
                    kind,
                    start_line: node.start_position().row + 1,
                    end_line,
                    start_byte: parent.start_byte(),
                    end_byte: parent.end_byte(),
                });
            }
        }
    }
    symbols
}

fn extract_js_imports(tree: &Tree, source_code: &str, language: tree_sitter::Language) -> Vec<ImportNode> {
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
    ";
    
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut import_name = String::new();
        let mut import_source = String::new();
        let mut line_number = 0;

        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(text) = node.utf8_text(source_code.as_bytes()) {
                if *capture_kind == "import" {
                    import_name = text.trim().to_string();
                    line_number = node.start_position().row + 1;
                } else if *capture_kind == "source" {
                    import_source = text.trim().to_string();
                }
            }
        }

        if !import_name.is_empty() {
            imports.push(ImportNode {
                name: import_name,
                source: if import_source.is_empty() { None } else { Some(import_source) },
                line_number,
            });
        }
    }
    imports
}
