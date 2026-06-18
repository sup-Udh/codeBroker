use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};
use tree_sitter::{Parser, Query, QueryCursor, Tree, StreamingIterator};

pub struct PythonFrontend;

impl LanguageFrontend for PythonFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".py")
    }

    fn parse_and_extract(&self, source_code: &str, _path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
        let language = tree_sitter_python::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        
        let tree = parser.parse(source_code, None)?;

        let symbols = extract_py_symbols(&tree, source_code, language.clone());
        let imports = extract_py_imports(&tree, source_code, language);

        Some((graph::models::FileMetadata::default(), symbols, imports))
    }
}

fn extract_py_symbols(tree: &Tree, source_code: &str, language: tree_sitter::Language) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let query_str = "
        (class_definition name: (identifier) @name) @class
        (function_definition name: (identifier) @name) @function
    ";
    
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        let mut name = String::new();
        let mut kind = String::new();
        let mut def_node = None;
        
        for capture in m.captures {
            let capture_name = &query.capture_names()[capture.index as usize];
            if *capture_name == "name" {
                if let Ok(text) = capture.node.utf8_text(source_code.as_bytes()) {
                    name = text.to_string();
                }
            } else if *capture_name == "class" {
                kind = "class".to_string();
                def_node = Some(capture.node);
            } else if *capture_name == "function" {
                kind = "function".to_string();
                def_node = Some(capture.node);
            }
        }
        
        if let Some(node) = def_node {
            let mut signature_parts = Vec::new();
            
            // 1. Check for decorators
            if let Some(parent) = node.parent() {
                if parent.kind() == "decorated_definition" {
                    let mut wcursor = parent.walk();
                    for child in parent.children(&mut wcursor) {
                        if child.kind() == "decorator" {
                            if let Ok(text) = child.utf8_text(source_code.as_bytes()) {
                                signature_parts.push(format!("[{}]", text.trim()));
                            }
                        }
                    }
                }
            }
            
            // 2. If it's a function, check if it's a method
            if kind == "function" {
                let mut current = node.parent();
                while let Some(p) = current {
                    if p.kind() == "class_definition" {
                        if let Some(class_name_node) = p.child_by_field_name("name") {
                            if let Ok(class_name) = class_name_node.utf8_text(source_code.as_bytes()) {
                                name = format!("{}.{}", class_name, name);
                                kind = "method".to_string();
                            }
                        }
                        break;
                    }
                    if p.kind() == "decorated_definition" {
                        current = p.parent();
                        continue;
                    }
                    current = p.parent();
                }
                
                // 3. Extract parameters for signature
                if let Some(params_node) = node.child_by_field_name("parameters") {
                    if let Ok(params_text) = params_node.utf8_text(source_code.as_bytes()) {
                        let clean_params = params_text.replace("\n", "").replace("  ", " ");
                        signature_parts.push(clean_params);
                    }
                }
            }
            
            let signature = if signature_parts.is_empty() {
                None
            } else {
                Some(signature_parts.join(" "))
            };

            let end_line = node.end_position().row + 1;
            symbols.push(SymbolNode {
                name,
                kind,
                prop_type: None,
                start_line: node.start_position().row + 1,
                end_line,
                start_byte: node.start_byte(),
                end_byte: node.end_byte(),
                signature,
            });
        }
    }
    
    symbols.sort_by_key(|s| s.start_byte);
    symbols.dedup_by_key(|s| s.start_byte);

    symbols
}

fn extract_py_imports(tree: &Tree, source_code: &str, language: tree_sitter::Language) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    // Grab any individual identifier inside any import statement
    let query_str = "
        (import_statement name: (_) @import)
        (import_from_statement module_name: (_) @source name: (_) @import)
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
                kind: None,
            });
        }
    }
    imports
}
