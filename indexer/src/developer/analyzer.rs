use storage::Database;
use crate::resolver::index::SymbolIndex;
use crate::developer::manifest::{RepositoryManifest, RepositoryFingerprint, RepositoryStatistics};
use crate::developer::frameworks::FrameworkDetector;
use crate::developer::entrypoints::EntrypointDetector;
use crate::developer::hotspots::HotspotDetector;
use crate::developer::layers::LayerDetector;
use crate::developer::subsystems::SubsystemDetector;
use std::collections::HashSet;

pub struct RepositoryAnalyzer;

impl RepositoryAnalyzer {
    pub fn analyze(db: &Database) -> Result<RepositoryManifest, String> {
        let symbol_index = SymbolIndex::build(db).map_err(|e| e.to_string())?;
        
        // 1. Statistics
        let mut total_files = 0;
        let mut total_symbols = 0;
        let mut total_relationships = 0;
        
        if let Ok(mut stmt) = db.conn.prepare("SELECT COUNT(*) FROM files") {
            total_files = stmt.query_row([], |row| row.get::<_, i64>(0)).unwrap_or(0) as usize;
        }
        if let Ok(mut stmt) = db.conn.prepare("SELECT COUNT(*) FROM symbols") {
            total_symbols = stmt.query_row([], |row| row.get::<_, i64>(0)).unwrap_or(0) as usize;
        }
        if let Ok(mut stmt) = db.conn.prepare("SELECT COUNT(*) FROM relationships") {
            total_relationships = stmt.query_row([], |row| row.get::<_, i64>(0)).unwrap_or(0) as usize;
        }
        
        let statistics = RepositoryStatistics {
            total_files,
            total_symbols,
            total_relationships,
        };
        
        // 2. Languages
        let mut languages_set = HashSet::new();
        for path in symbol_index.file_paths.values() {
            if path.ends_with(".ts") || path.ends_with(".tsx") {
                languages_set.insert("TypeScript".to_string());
            } else if path.ends_with(".js") || path.ends_with(".jsx") {
                languages_set.insert("JavaScript".to_string());
            } else if path.ends_with(".rs") {
                languages_set.insert("Rust".to_string());
            } else if path.ends_with(".py") {
                languages_set.insert("Python".to_string());
            }
        }
        let languages: Vec<String> = languages_set.into_iter().collect();
        
        // 3. Frameworks
        let frameworks = FrameworkDetector::detect(db, &symbol_index)?;
        let main_frameworks: Vec<String> = frameworks.iter().map(|f| f.framework.clone()).collect();
        
        // 4. Subsystems
        let subsystems = SubsystemDetector::detect(db, &symbol_index)?;
        
        // 5. Entrypoints
        // Let's get entrypoints from symbol_features
        let mut file_entrypoints = HashSet::new();
        if let Ok(mut stmt) = db.conn.prepare("SELECT symbol_id FROM symbol_features WHERE is_entrypoint = 1") {
            if let Ok(rows) = stmt.query_map([], |row| row.get::<_, i64>(0)) {
                for row in rows.flatten() {
                    file_entrypoints.insert(row);
                }
            }
        }
        let entrypoints = EntrypointDetector::detect(&symbol_index, &file_entrypoints);
        
        // 6. Architecture Layers
        let architecture_layers = LayerDetector::detect(&subsystems);
        
        // 7. Hotspots
        let hotspots = HotspotDetector::detect(db, &symbol_index)?;
        
        // 8. Fingerprint
        let project_type = if entrypoints.iter().any(|e| e.category == "HTTP/API" || e.category == "Next.js Pages") {
            "Full Stack".to_string()
        } else if entrypoints.iter().any(|e| e.category == "CLI") {
            "CLI Tool".to_string()
        } else {
            "Library".to_string()
        };
        
        let complexity = if total_symbols > 5000 {
            "High".to_string()
        } else if total_symbols > 500 {
            "Medium".to_string()
        } else {
            "Low".to_string()
        };
        
        let fingerprint = RepositoryFingerprint {
            project_type,
            main_frameworks,
            architecture_style: "Layered".to_string(), // Inferred from ArchitectureLayers
            primary_languages: languages.clone(),
            complexity,
        };

        Ok(RepositoryManifest {
            fingerprint,
            statistics,
            languages,
            frameworks,
            subsystems,
            entrypoints,
            architecture_layers,
            hotspots,
        })
    }
}
