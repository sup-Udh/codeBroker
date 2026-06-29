use crate::developer::manifest::CategorizedEntrypoint;
use crate::resolver::index::SymbolIndex;
use std::collections::HashSet;

pub struct EntrypointDetector;

impl EntrypointDetector {
    pub fn detect(symbol_index: &SymbolIndex, file_entrypoints: &HashSet<i64>) -> Vec<CategorizedEntrypoint> {
        let mut entrypoints = Vec::new();
        
        for &sym_id in file_entrypoints {
            if let Some(symbol) = symbol_index.get_symbol(sym_id) {
                let file_path = symbol_index.file_paths.get(&symbol.file_id).map(|s| s.as_str()).unwrap_or("");
                let category = Self::categorize(symbol.name.as_str(), file_path);
                
                entrypoints.push(CategorizedEntrypoint {
                    category,
                    symbol_id: sym_id,
                    file_id: symbol.file_id,
                    name: symbol.name.clone(),
                });
            }
        }
        
        entrypoints
    }
    
    fn categorize(name: &str, file_path: &str) -> String {
        let path = file_path.to_lowercase();
        let sym_name = name.to_lowercase();
        
        if path.contains("/api/") || path.contains("routes") || sym_name.contains("route") || sym_name.contains("handler") {
            "HTTP/API".to_string()
        } else if path.contains("test") || path.contains("spec") {
            "Tests".to_string()
        } else if path.contains("cli") || path.contains("bin") || sym_name == "main" {
            "CLI".to_string()
        } else if path.contains("worker") || path.contains("job") {
            "Workers".to_string()
        } else if path.contains("cron") {
            "Cron".to_string()
        } else if path.contains("scripts/") {
            "Scripts".to_string()
        } else if path.contains("/pages/") || path.contains("/app/") {
            "Next.js Pages".to_string()
        } else {
            "Library Exports".to_string()
        }
    }
}
