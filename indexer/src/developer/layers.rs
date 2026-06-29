use crate::developer::manifest::{ArchitectureLayer, Subsystem};
use std::collections::{HashMap, HashSet};

pub struct LayerDetector;

impl LayerDetector {
    pub fn detect(subsystems: &[Subsystem]) -> Vec<ArchitectureLayer> {
        // Build a directed graph of subsystems
        let mut dependents_count: HashMap<String, usize> = HashMap::new();
        let mut dependencies_count: HashMap<String, usize> = HashMap::new();
        
        for sub in subsystems {
            dependents_count.insert(sub.name.clone(), sub.dependents.len());
            dependencies_count.insert(sub.name.clone(), sub.dependencies.len());
        }
        
        let mut presentation = Vec::new();
        let mut business = Vec::new();
        let mut persistence = Vec::new();
        let mut infrastructure = Vec::new();
        
        for sub in subsystems {
            let name = sub.name.to_lowercase();
            let deps = sub.dependencies.len();
            let dependents = sub.dependents.len();
            
            // Heuristic layer assignment based on fan-in / fan-out
            // Presentation usually has high dependencies, few dependents (or is an entrypoint)
            // Persistence usually has many dependents, few dependencies.
            // Business is in the middle.
            // Infrastructure / Utilities has massive dependents, almost zero dependencies.
            
            if name.contains("db") || name.contains("database") || name.contains("model") || name.contains("schema") {
                persistence.push(sub.name.clone());
            } else if name.contains("util") || name.contains("config") || name.contains("core") {
                infrastructure.push(sub.name.clone());
            } else if name.contains("api") || name.contains("route") || name.contains("controller") || name.contains("view") || name.contains("page") || name.contains("component") {
                presentation.push(sub.name.clone());
            } else if name.contains("service") || name.contains("manager") || name.contains("logic") || name.contains("auth") {
                business.push(sub.name.clone());
            } else {
                // Infer from edges if naming isn't clear
                if dependents == 0 && deps > 0 {
                    presentation.push(sub.name.clone());
                } else if deps == 0 && dependents > 0 {
                    infrastructure.push(sub.name.clone());
                } else {
                    business.push(sub.name.clone());
                }
            }
        }
        
        vec![
            ArchitectureLayer {
                name: "Presentation".to_string(),
                subsystems: presentation,
                depends_on: vec!["Business".to_string()],
            },
            ArchitectureLayer {
                name: "Business".to_string(),
                subsystems: business,
                depends_on: vec!["Persistence".to_string(), "Infrastructure".to_string()],
            },
            ArchitectureLayer {
                name: "Persistence".to_string(),
                subsystems: persistence,
                depends_on: vec!["Infrastructure".to_string()],
            },
            ArchitectureLayer {
                name: "Infrastructure".to_string(),
                subsystems: infrastructure,
                depends_on: vec![],
            }
        ]
    }
}
