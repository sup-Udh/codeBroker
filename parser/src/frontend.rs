use graph::{SymbolNode, ImportNode};

pub trait LanguageFrontend {
    fn can_handle(&self, extension: &str) -> bool;

    // parse and extract to unified domains
    fn parse_and_extract(&self, soruce_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)>;
}