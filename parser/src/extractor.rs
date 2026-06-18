// gap between the tree sitter synatx and grpah models

use tree_sitter::{Query, QueryCursor, Tree, StreamingIterator};
use graph::SymbolNode;
use graph::ImportNode;


/// Traverses the AST to find functions and structs
pub fn extract_symbols(tree: &Tree, source_code: &str) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();

    // 1. Define our search query using Tree-sitter's query language
    // We are looking for function_items and struct_items, specifically 
    // capturing their `name` identifiers.
    let query_str = "
        (function_item name: (identifier) @function)
        (struct_item name: (type_identifier) @struct)
    ";

    let language = tree_sitter_rust::LANGUAGE.into();
    
    // Compile the query
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();

    // 2. Execute the query against the root node of our parsed tree
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());

    // 3. Iterate through the matches and build our domain models


    while let Some(m) = matches.next() {
        for capture in m.captures {
            // The node in the AST that matched  capture (e.g., the actual word "main")
            let node = capture.node;
            
            let capture_kind = &query.capture_names()[capture.index as usize];
            
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                let parent = node.parent().unwrap_or(node);
                let end_line = parent.end_position().row + 1;
                symbols.push(SymbolNode {
                    name: name.to_string(),
                    kind: capture_kind.to_string(),
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



/// Traverses the AST to find import/use statements
pub fn extract_imports(tree: &Tree, source_code: &str) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    // The query: Find a `use_declaration`, and capture whatever is inside 
    // its `argument` block and tag it as `@import`.

 let query_str = "
        (use_declaration argument: (_) @import)
    ";

    let language = tree_sitter_rust::LANGUAGE.into();
    let query = Query::new(&language, query_str).expect("Invalid Tree-sitter query");
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), source_code.as_bytes());
    while let Some(m) = matches.next() {
        for capture in m.captures {
            let node = capture.node;
            
            if let Ok(name) = node.utf8_text(source_code.as_bytes()) {
                imports.push(ImportNode {
                    // Tree-sitter includes spaces sometimes, so we trim it
                    name: name.trim().to_string(),
                    line_number: node.start_position().row + 1,
                });
            }
        }
    }
    imports
}