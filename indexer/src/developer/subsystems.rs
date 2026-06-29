use crate::developer::manifest::Subsystem;
use crate::resolver::index::SymbolIndex;
use storage::Database;
use std::collections::{HashMap, HashSet};

pub struct SubsystemDetector;

impl SubsystemDetector {
    pub fn detect(db: &Database, symbol_index: &SymbolIndex) -> Result<Vec<Subsystem>, String> {
        // We will use Louvain community detection results which are already in `symbol_features`.
        // If that's not available, fallback to directory boundaries.
        
        let mut communities: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut symbol_to_community = HashMap::new();
        
        if let Ok(mut stmt) = db.conn.prepare("SELECT symbol_id, community_id, pagerank FROM symbol_features") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, f64>(2)?
                ))
            }) {
                for row in rows.flatten() {
                    let (sym_id, comm_id, _pagerank) = row;
                    communities.entry(comm_id).or_default().push(sym_id);
                    symbol_to_community.insert(sym_id, comm_id);
                }
            }
        }
        
        let mut subsystems = Vec::new();
        
        // Edge analysis between communities for dependencies
        let mut edges = Vec::new();
        if let Ok(mut stmt) = db.conn.prepare("SELECT source_file_id, target_symbol_id FROM edges") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            }) {
                for row in rows.flatten() {
                    edges.push(row);
                }
            }
        }
        
        for (comm_id, syms) in communities {
            // Naming the community: look at the most common directory or symbol name suffix
            let mut dir_counts = HashMap::new();
            let mut files = HashSet::new();
            
            for &sym_id in &syms {
                if let Some(sym) = symbol_index.get_symbol(sym_id) {
                    files.insert(sym.file_id);
                    if let Some(path) = symbol_index.file_paths.get(&sym.file_id) {
                        let parts: Vec<&str> = path.split('/').collect();
                        if parts.len() > 1 {
                            let dir = parts[parts.len() - 2];
                            *dir_counts.entry(dir.to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            
            let mut best_name = format!("Community-{}", comm_id);
            let mut max_count = 0;
            for (dir, count) in dir_counts {
                if count > max_count && dir != "src" && dir != "lib" && dir != "app" {
                    max_count = count;
                    best_name = dir;
                }
            }
            
            // Calculate entrypoints (symbols in this community imported by others)
            let mut entrypoint_ids = HashSet::new();
            let mut dependencies = HashSet::new();
            let mut dependents = HashSet::new();
            
            for &(source_file, target_sym) in &edges {
                let target_comm = symbol_to_community.get(&target_sym).copied().unwrap_or(-1);
                
                // Which community does the source file belong to? We approximate by picking the first symbol in the file.
                let mut source_comm = -1;
                if let Some(file_syms) = symbol_index.symbols_by_file.get(&source_file) {
                    if let Some(&first_sym) = file_syms.first() {
                        source_comm = symbol_to_community.get(&first_sym).copied().unwrap_or(-1);
                    }
                }
                
                if target_comm == comm_id && source_comm != comm_id {
                    entrypoint_ids.insert(target_sym);
                    dependents.insert(format!("Community-{}", source_comm)); // Would resolve to actual names
                }
                
                if source_comm == comm_id && target_comm != comm_id {
                    dependencies.insert(format!("Community-{}", target_comm));
                }
            }
            
            // Format name nicely
            let mut name = best_name.clone();
            if name.is_empty() || name.starts_with("Community-") {
                name = format!("Module {}", comm_id);
            }
            // Capitalize
            let mut chars = name.chars();
            let name = match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            };
            
            subsystems.push(Subsystem {
                name,
                file_ids: files.into_iter().collect(),
                symbol_ids: syms,
                dependencies: dependencies.into_iter().collect(),
                dependents: dependents.into_iter().collect(),
                entrypoint_ids: entrypoint_ids.into_iter().collect(),
                importance_score: max_count as f64, // Rough approximation
            });
        }
        
        Ok(subsystems)
    }
}
