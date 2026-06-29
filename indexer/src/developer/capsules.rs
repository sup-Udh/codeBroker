use crate::developer::manifest::{Subsystem, Hotspots};
use crate::resolver::index::SymbolIndex;
use storage::Database;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct ContextCapsule {
    pub target: String,
    pub core_files: Vec<String>,
    pub public_apis: Vec<String>,
    pub models: Vec<String>,
    pub tests: Vec<String>,
    pub dependencies: Vec<String>,
    pub dependents: Vec<String>,
    pub entry_points: Vec<String>,
    pub hotspots: Hotspots,
}

pub struct CapsuleBuilder;

impl CapsuleBuilder {
    pub fn build(target: &str, subsystems: &[Subsystem], symbol_index: &SymbolIndex, hotspots: &Hotspots) -> Option<ContextCapsule> {
        let subsystem = subsystems.iter().find(|s| s.name.eq_ignore_ascii_case(target))?;
        
        let mut core_files = Vec::new();
        let mut public_apis = Vec::new();
        let mut models = Vec::new();
        let mut tests = Vec::new();
        
        for &file_id in &subsystem.file_ids {
            if let Some(path) = symbol_index.file_paths.get(&file_id) {
                core_files.push(path.clone());
                
                if path.contains("model") || path.contains("schema") || path.contains("entities") {
                    models.push(path.clone());
                }
                if path.contains("test") || path.contains("spec") {
                    tests.push(path.clone());
                }
            }
        }
        
        for &sym_id in &subsystem.entrypoint_ids {
            if let Some(sym) = symbol_index.get_symbol(sym_id) {
                public_apis.push(sym.name.clone());
            }
        }
        
        let mut entry_points = Vec::new();
        for &sym_id in &subsystem.entrypoint_ids {
            if let Some(sym) = symbol_index.get_symbol(sym_id) {
                if let Some(path) = symbol_index.file_paths.get(&sym.file_id) {
                    entry_points.push(format!("{} in {}", sym.name, path));
                }
            }
        }
        
        // Impact radius and hotspots...
        // For now, we attach the global hotspots, or filter them for this subsystem.
        // Let's just clone the global hotspots for now.
        let sub_hotspots = hotspots.clone();
        
        Some(ContextCapsule {
            target: target.to_string(),
            core_files,
            public_apis,
            models,
            tests,
            dependencies: subsystem.dependencies.clone(),
            dependents: subsystem.dependents.clone(),
            entry_points,
            hotspots: sub_hotspots,
        })
    }
}
