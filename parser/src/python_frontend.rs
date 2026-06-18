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
        (class_definition name: (identifier) @type)
        (function_definition name: (identifier) @function)
    ";
    
    let language = tree_sitter_python::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            let capture_kind = &query.capture_names()[capture.index as usize];
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let parent = node.parent().unwrap_or(node);
                let end_line = parent.end_position().row + 1;
                symbols.push(SymbolNode {
                    name: name.to_string(),
                    kind: capture_kind.to_string(),
                    prop_type: None,
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
