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

    fn parse_and_extract(&self, source_code: &str, path: &str) -> Option<(graph::models::FileMetadata, Vec<SymbolNode>, Vec<ImportNode>)> {
        let mut symbols = Vec::new();

        if path.ends_with("package.json") {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(source_code) {
                if let Some(deps) = json.get("dependencies").and_then(|d| d.as_object()) {
                    for (k, _) in deps {
                        symbols.push(SymbolNode {
                            name: k.clone(),
                            kind: "dependency".to_string(),
                            prop_type: None,
                            start_line: 0, end_line: 0, start_byte: 0, end_byte: 0,
                        });
                    }
                }
                if let Some(dev_deps) = json.get("devDependencies").and_then(|d| d.as_object()) {
                    for (k, _) in dev_deps {
                        symbols.push(SymbolNode {
                            name: k.clone(),
                            kind: "devDependency".to_string(),
                            prop_type: None,
                            start_line: 0, end_line: 0, start_byte: 0, end_byte: 0,
                        });
                    }
                }
                if let Some(scripts) = json.get("scripts").and_then(|d| d.as_object()) {
                    for (k, _) in scripts {
                        symbols.push(SymbolNode {
                            name: k.clone(),
                            kind: "script".to_string(),
                            prop_type: None,
                            start_line: 0, end_line: 0, start_byte: 0, end_byte: 0,
                        });
                    }
                }
            }
        }

        Some((graph::models::FileMetadata::default(), symbols, Vec::new()))
    }
}
