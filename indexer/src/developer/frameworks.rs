use storage::Database;
use crate::developer::manifest::{FrameworkDetection, ConfidenceLevel};
use crate::resolver::index::SymbolIndex;
use std::collections::HashSet;

pub struct FrameworkDetector;

impl FrameworkDetector {
    pub fn detect(db: &Database, symbol_index: &SymbolIndex) -> Result<Vec<FrameworkDetection>, String> {
        let mut detections = Vec::new();
        
        // 1. Parser-discovered imports (Highest Priority)
        let mut imports_evidence = Vec::new();
        if let Ok(relationships) = db.get_all_relationships_with_lines() {
            for (_, _, name, source, kind, _) in relationships {
                if kind.as_deref() == Some("imports") {
                    let import_source = source.unwrap_or_default();
                    if import_source.contains("next") {
                        imports_evidence.push(format!("Import('{}')", import_source));
                    } else if import_source.contains("express") {
                        imports_evidence.push(format!("Import('{}')", import_source));
                    } else if import_source.contains("react") {
                        imports_evidence.push(format!("Import('{}')", import_source));
                    }
                }
            }
        }
        
        let has_next_import = imports_evidence.iter().any(|e| e.contains("next"));
        let has_express_import = imports_evidence.iter().any(|e| e.contains("express"));
        let has_react_import = imports_evidence.iter().any(|e| e.contains("react"));
        
        // 2. Dependency files
        let mut next_dep_evidence: Vec<String> = Vec::new();
        let mut express_dep_evidence: Vec<String> = Vec::new();
        
        for path in symbol_index.file_paths.values() {
            if path.ends_with("package.json") {
                // In a real system, we'd parse the JSON. For now, rely on file existence as weak evidence.
                // Or if we know package.json was parsed, we could check its symbols.
            }
        }
        
        // 3. Entrypoint conventions
        let mut next_entry_evidence = Vec::new();
        for path in symbol_index.file_paths.values() {
            if path.contains("/app/") || path.contains("/pages/") {
                if path.ends_with("page.tsx") || path.ends_with("route.ts") {
                    next_entry_evidence.push(format!("RouteConvention('{}')", path));
                }
            }
        }
        
        if has_next_import || !next_entry_evidence.is_empty() {
            let mut evidence = Vec::new();
            evidence.extend(imports_evidence.iter().filter(|e| e.contains("next")).cloned());
            evidence.extend(next_entry_evidence);
            let confidence = if has_next_import { ConfidenceLevel::High } else { ConfidenceLevel::Medium };
            detections.push(FrameworkDetection {
                framework: "Next.js".to_string(),
                confidence,
                evidence,
            });
        }
        
        if has_express_import {
            let mut evidence = Vec::new();
            evidence.extend(imports_evidence.iter().filter(|e| e.contains("express")).cloned());
            detections.push(FrameworkDetection {
                framework: "Express".to_string(),
                confidence: ConfidenceLevel::High,
                evidence,
            });
        } else if has_react_import && !has_next_import {
            let mut evidence = Vec::new();
            evidence.extend(imports_evidence.iter().filter(|e| e.contains("react")).cloned());
            detections.push(FrameworkDetection {
                framework: "React".to_string(),
                confidence: ConfidenceLevel::High,
                evidence,
            });
        }

        Ok(detections)
    }
}
