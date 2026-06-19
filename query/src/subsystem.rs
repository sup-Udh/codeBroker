use storage::Database;
use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use sha2::{Sha256, Digest};

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SubsystemStats {
    pub name: String,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
    pub dependencies: Vec<String>, // Other systems/symbols this subsystem relies on
    pub consumers: Vec<String>,    // Other systems/symbols that rely on this subsystem
    pub routes: Vec<String>,
    pub entrypoints: Vec<String>,
    pub subsystem_hash: String,
}

pub fn discover_subsystem(db: &Database, name: &str) -> Result<SubsystemStats, String> {
    // 1. Folder-based and Framework-based Discovery
    let search_term = format!("%{}%", name);
    let exact_folder_term = format!("%/{}/%", name);
    let start_folder_term = format!("{}/%", name);

    // Find Files
    let mut file_stmt = db.conn.prepare("SELECT id, path FROM files WHERE path LIKE ?1 OR path LIKE ?2 OR path LIKE ?3").map_err(|e| e.to_string())?;
    let mut file_rows = file_stmt.query(rusqlite::params![search_term, exact_folder_term, start_folder_term]).map_err(|e| e.to_string())?;
    
    let mut matched_file_ids = HashSet::new();
    let mut files = Vec::new();
    while let Some(row) = file_rows.next().unwrap_or(None) {
        let id: i64 = row.get(0).unwrap();
        let path: String = row.get(1).unwrap();
        matched_file_ids.insert(id);
        files.push(path);
    }

    // Find Symbols (by name or inside matched files)
    let mut matched_symbol_ids = HashSet::new();
    let mut symbols = Vec::new();
    let mut routes = Vec::new();

    // Add symbols from matched files
    for &f_id in &matched_file_ids {
        let mut sym_stmt = db.conn.prepare("SELECT id, name, kind FROM symbols WHERE file_id = ?1").unwrap();
        let mut sym_rows = sym_stmt.query(rusqlite::params![f_id]).unwrap();
        while let Some(row) = sym_rows.next().unwrap_or(None) {
            let id: i64 = row.get(0).unwrap();
            let s_name: String = row.get(1).unwrap();
            let kind: String = row.get(2).unwrap();
            matched_symbol_ids.insert(id);
            symbols.push(s_name.clone());
            if kind == "route" || kind == "endpoint" {
                routes.push(s_name);
            }
        }
    }

    // Add symbols by name (that aren't already included)
    let mut sym_name_stmt = db.conn.prepare("SELECT id, name, file_id, kind FROM symbols WHERE name LIKE ?1").unwrap();
    let mut sym_name_rows = sym_name_stmt.query(rusqlite::params![search_term]).unwrap();
    while let Some(row) = sym_name_rows.next().unwrap_or(None) {
        let id: i64 = row.get(0).unwrap();
        let s_name: String = row.get(1).unwrap();
        let f_id: i64 = row.get(2).unwrap();
        let kind: String = row.get(3).unwrap();
        if !matched_symbol_ids.contains(&id) {
            matched_symbol_ids.insert(id);
            symbols.push(s_name.clone());
            if kind == "route" || kind == "endpoint" {
                routes.push(s_name);
            }
            if !matched_file_ids.contains(&f_id) {
                matched_file_ids.insert(f_id);
                // Also get the file path
                let path: String = db.conn.query_row("SELECT path FROM files WHERE id = ?1", rusqlite::params![f_id], |r| r.get(0)).unwrap_or_default();
                if !path.is_empty() {
                    files.push(path);
                }
            }
        }
    }

    // 2. Graph-Based Expansion
    // Find dependencies
    let mut dependencies = HashSet::new();
    for &f_id in &matched_file_ids {
        let mut edge_stmt = db.conn.prepare(
            "SELECT symbols.name, symbols.id FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id 
             WHERE edges.source_file_id = ?1"
        ).unwrap();
        let mut edge_rows = edge_stmt.query(rusqlite::params![f_id]).unwrap();
        while let Some(row) = edge_rows.next().unwrap_or(None) {
            let s_name: String = row.get(0).unwrap();
            let s_id: i64 = row.get(1).unwrap();
            if !matched_symbol_ids.contains(&s_id) {
                dependencies.insert(s_name);
            }
        }
    }

    // Find consumers
    let mut consumers = HashSet::new();
    let mut entrypoints = HashSet::new(); 
    for &s_id in &matched_symbol_ids {
        let mut edge_stmt = db.conn.prepare(
            "SELECT files.path, files.id FROM edges 
             JOIN files ON edges.source_file_id = files.id 
             WHERE edges.target_symbol_id = ?1"
        ).unwrap();
        let mut edge_rows = edge_stmt.query(rusqlite::params![s_id]).unwrap();
        let mut has_consumers = false;
        while let Some(row) = edge_rows.next().unwrap_or(None) {
            let f_path: String = row.get(0).unwrap();
            let f_id: i64 = row.get(1).unwrap();
            has_consumers = true;
            if !matched_file_ids.contains(&f_id) {
                consumers.insert(f_path);
            }
        }
        if !has_consumers {
            // Not adding logic here for entrypoints since we just rely on routes
        }
    }
    
    // Any route in this subsystem is an entrypoint
    entrypoints.extend(routes.iter().cloned());

    files.sort();
    files.dedup();
    symbols.sort();
    symbols.dedup();
    routes.sort();
    routes.dedup();
    
    let mut deps_vec: Vec<String> = dependencies.into_iter().collect();
    deps_vec.sort();
    let mut cons_vec: Vec<String> = consumers.into_iter().collect();
    cons_vec.sort();
    let mut entry_vec: Vec<String> = entrypoints.into_iter().collect();
    entry_vec.sort();

    // 3. Compute Deterministic Hash
    let mut hash_input = String::new();
    let mut sorted_fids: Vec<i64> = matched_file_ids.into_iter().collect();
    sorted_fids.sort();
    let mut sorted_sids: Vec<i64> = matched_symbol_ids.into_iter().collect();
    sorted_sids.sort();
    
    for id in sorted_fids {
        hash_input.push_str(&format!("f{}_", id));
    }
    for id in sorted_sids {
        hash_input.push_str(&format!("s{}_", id));
    }
    
    let mut hasher = Sha256::new();
    hasher.update(hash_input.as_bytes());
    let subsystem_hash = format!("{:x}", hasher.finalize());

    Ok(SubsystemStats {
        name: name.to_string(),
        files,
        symbols,
        dependencies: deps_vec,
        consumers: cons_vec,
        routes,
        entrypoints: entry_vec,
        subsystem_hash,
    })
}
