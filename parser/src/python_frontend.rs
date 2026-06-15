use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};
use tree_sitter::{Parser, Query, QueryCursor, Tree, StreamingIterator};

pub struct PythonFrontend;

impl LanguageFrontend for PythonFrontend {
    fn can_handle(&self, extension: &str) -> bool {
        extension == "py"
    }

    fn parse_and_extract(&self, source_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)> {
        let language = tree_sitter_python::LANGUAGE.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        
        let tree = parser.parse(source_code, None)?;

        let symbols = extract_py_symbols(&tree, source_code);
        let imports = extract_py_imports(&tree, source_code);

        Some((symbols, imports))
    }
}

fn extract_py_symbols(tree: &Tree, source_code: &str) -> Vec<SymbolNode> {
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
                symbols.push(SymbolNode {
                    name: name.to_string(),
                    kind: capture_kind.to_string(),
                    line_number: node.start_position().row + 1,
                });
            }
        }
    }
    symbols
}

fn extract_py_imports(tree: &Tree, source_code: &str) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    // Grab any individual identifier inside any import statement
    let query_str = "
        (import_statement) @import
        (import_from_statement) @import
    ";
    
    let language = tree_sitter_python::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                imports.push(ImportNode {
                    name: name.trim().to_string(),
                    line_number: node.start_position().row + 1,
                });
            }
        }
    }
    imports
}
