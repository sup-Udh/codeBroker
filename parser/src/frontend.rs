use graph::{SymbolNode, ImportNode};

pub trait LanguageFrontend {
    fn can_handle(&self, extension: &str) -> bool;

    // parse and extract to unified domains
    fn parse_and_extract(&self, soruce_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)>;
}

pub struct RustFrontend;

impl LanguageFrontend for RustFrontend {
    fn can_handle(&self, extension: &str) -> bool {
        extension == "rs"
    }

    fn parse_and_extract(&self, source_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)> {
        // 1. We call your existing tree-sitter parser
        let tree = crate::treesitter::parse_rust_code(source_code)?;
        
        // 2. We call your existing extractors
        let symbols = crate::extractor::extract_symbols(&tree, source_code);
        let imports = crate::extractor::extract_imports(&tree, source_code);
        // 3. We return the unified tuple
        Some((symbols, imports))
    }

}