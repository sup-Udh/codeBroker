use graph::{SymbolNode, ImportNode};

pub trait LanguageFrontend {
    fn can_handle(&self, path: &str) -> bool;

    // parse and extract to unified domains
    fn parse_and_extract(&self, source_code: &str, path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)>;
}

pub struct RustFrontend;

impl LanguageFrontend for RustFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".rs")
    }

    fn parse_and_extract(&self, source_code: &str, _path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
        // 1. We call your existing tree-sitter parser
        let tree = crate::treesitter::parse_rust_code(source_code)?;
        
        // 2. We call your existing extractors
        let symbols = crate::extractor::extract_symbols(&tree, source_code);
        let imports = crate::extractor::extract_imports(&tree, source_code);
        Some((graph::models::FileMetadata::default(), symbols, imports))
    }

}