use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, IMportNode};

pub struct TypeScriptFrontend;

impl LanguageFrontend for TypeScriptFrontend {
    fn can_handle(&self, extension: &str) -> bool {
        extension == "ts" || extension == "tsx" || extensions == "js"
    }

    // parse-extract:

    fn parse_and_extract(&self, source_code: &str) -> Option<(Vec<SymbolNode>, Vec<RelationshipNode>)> {
        // 1. Initialize the TypeScript language parser
        let language = tree_sitter_typescript::language_typescript();
        let mut parser = tree_sitter::Parser::new();
        parser.set_language(language).ok()?;
        let tree = parser.parse(source_code, None)?;
        // 2. Run the TS-specific extractors (outlined below)
        let symbols = extract_ts_symbols(&tree, source_code);
        let imports = extract_ts_imports(&tree, source_code);
        Some((symbols, imports))
    }

}