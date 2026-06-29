use tree_sitter::Parser;
fn main() {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_javascript::LANGUAGE.into()).unwrap();
    let tree = parser.parse("import { Foo as Bar } from './foo';", None).unwrap();
    println!("{}", tree.root_node().to_sexp());
}
