use tree_sitter::Tree;

pub fn parse_rust_code(source_code: &str) -> Option<Tree> {
    let rust_language = tree_sitter_rust::LANGUAGE.into();
    crate::pool::with_parser("rust", &rust_language, |parser| {
        parser.parse(source_code, None)
    })
}
