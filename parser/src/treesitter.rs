use tree_sitter::{Parser, Tree};

pub fn parse_rust_code(source_code: &str) -> Option<Tree> {
    // new tree-sitter parser init
    let mut parser = Parser::new();

    let rust_language = tree_sitter_rust::LANGUAGE.into();

    parser.set_language(&rust_language).expect("error Loading the rust grammar");

    parser.parse(source_code, None) // none - optional for previus parsing

}