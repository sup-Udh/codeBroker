use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};
use tree_sitter::{Parser, Query, QueryCursor, Tree, StreamingIterator};

pub struct TypeScriptFrontend;

impl LanguageFrontend for TypeScriptFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".ts")
    }

    fn parse_and_extract(&self, source_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)> {
        let language = tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        
        let tree = parser.parse(source_code, None)?;

        let symbols = extract_ts_symbols(&tree, source_code, language.clone());
        let imports = extract_ts_imports(&tree, source_code, language);

        Some((symbols, imports))
    }
}

pub struct TsxFrontend;

impl LanguageFrontend for TsxFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".tsx")
    }

    fn parse_and_extract(&self, source_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)> {
        let language = tree_sitter_typescript::LANGUAGE_TSX.into();
        let mut parser = Parser::new();
        parser.set_language(&language).ok()?;
        
        let tree = parser.parse(source_code, None)?;

        let symbols = extract_ts_symbols(&tree, source_code, language.clone());
        let imports = extract_ts_imports(&tree, source_code, language);

        Some((symbols, imports))
    }
}

fn extract_ts_symbols(tree: &Tree, source_code: &str, language: tree_sitter::Language) -> Vec<SymbolNode> {
    let mut symbols = Vec::new();
    let query_str = "
        (class_declaration name: (type_identifier) @type)
        (interface_declaration name: (type_identifier) @type)
        (function_declaration name: (identifier) @function)
        (lexical_declaration (variable_declarator name: (identifier) @function value: (arrow_function)))
    ";
    
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

fn extract_ts_imports(tree: &Tree, source_code: &str, language: tree_sitter::Language) -> Vec<ImportNode> {
    let mut imports = Vec::new();
    let query_str = "
        (import_statement 
            (import_clause (named_imports (import_specifier name: (identifier) @import)))
        )
    ";
    
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
