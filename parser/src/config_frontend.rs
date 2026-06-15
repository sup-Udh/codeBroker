use crate::frontend::LanguageFrontend;
use graph::{SymbolNode, ImportNode};

pub struct ConfigFrontend;

impl LanguageFrontend for ConfigFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with("package.json") ||
        path.ends_with("tsconfig.json") ||
        path.ends_with("Cargo.toml") ||
        path.ends_with("pyproject.toml") ||
        path.ends_with("requirements.txt") ||
        path.ends_with("docker-compose.yml") ||
        path.ends_with("Dockerfile")
    }

    fn parse_and_extract(&self, _source_code: &str) -> Option<(Vec<SymbolNode>, Vec<ImportNode>)> {
        // Configuration files are indexed but not deeply parsed for symbols or imports.
        // Returning an empty set ensures the router formally logs the file in the database
        // so that Layer 3 (AI) can locate it and read its contents.
        Some((Vec::new(), Vec::new()))
    }
}
