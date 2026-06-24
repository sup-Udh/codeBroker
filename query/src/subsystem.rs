use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use storage::Database;

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct SubsystemStats {
    pub name: String,
    pub files: Vec<String>,
    pub symbols: Vec<String>,
    pub dependencies: Vec<String>, // Other systems/symbols this subsystem relies on
    pub consumers: Vec<String>,    // Other systems/symbols that rely on this subsystem
    pub routes: Vec<String>,
    pub entrypoints: Vec<String>,
    pub clusters: Vec<Vec<String>>,
    pub subsystem_hash: String,
    pub confidence: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EntrypointEntry {
    pub name: String,
    pub kind: String,
    pub file_path: String,
}

#[derive(Serialize, Deserialize, Debug, Default)]
pub struct EntrypointReport {
    pub total: usize,
    pub routes: Vec<EntrypointEntry>,
    pub pages: Vec<EntrypointEntry>,
}

/// Repo-wide entrypoint enumeration: every `route`/`endpoint` (API handlers)
/// and `page`/`layout` (Next.js page entrypoints) symbol in the index, with
/// no subsystem name required. `discover_subsystem`'s `entrypoints` field
/// only ever surfaces entrypoints once a subsystem name is already known and
/// scoped to it — there was previously no way to answer "what are this
/// repo's entrypoints" without already knowing which subsystem names to
/// guess (benchmark run_001's gap #1: requirement #3 had to be reconstructed
/// manually by calling `subsystem_stats` per-subsystem).
pub fn list_entrypoints(
    db: &Database,
    path_scope: Option<&str>,
) -> Result<EntrypointReport, String> {
    let mut stmt = db
        .conn
        .prepare(
            "SELECT symbols.name, symbols.kind, files.path
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.kind IN ('route', 'endpoint', 'page', 'layout')",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut routes = Vec::new();
    let mut pages = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let name: String = row.get(0).unwrap_or_default();
        let kind: String = row.get(1).unwrap_or_default();
        let path: String = row.get(2).unwrap_or_default();
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        let entry = EntrypointEntry {
            name,
            kind: kind.clone(),
            file_path: db.resolve_path(&path),
        };
        if kind == "route" || kind == "endpoint" {
            routes.push(entry);
        } else {
            pages.push(entry);
        }
    }

    routes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    pages.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    Ok(EntrypointReport {
        total: routes.len() + pages.len(),
        routes,
        pages,
    })
}

/// Diffs two subsystems' edge sets to answer "how do A and B communicate":
/// counts edges whose source symbol belongs to A and target belongs to B
/// (and vice versa), with up to 10 example symbol-name pairs per direction.
/// `subsystem_stats`'s `dependencies`/`consumers` fields are symbol/file
/// lists relative to a SINGLE subsystem; answering a two-subsystem question
/// previously required calling it twice and manually diffing the two result
/// sets (benchmark run_001's gap #3).
pub fn subsystem_communication(
    db: &Database,
    a: &str,
    b: &str,
) -> Result<SubsystemCommunication, String> {
    let stats_a = discover_subsystem(db, a, &[], None)?;
    let stats_b = discover_subsystem(db, b, &[], None)?;

    let symbol_ids_for = |files: &[String]| -> Result<HashSet<i64>, String> {
        let mut ids = HashSet::new();
        for f in files {
            let mut stmt = db
                .conn
                .prepare("SELECT id FROM symbols WHERE file_id IN (SELECT id FROM files WHERE path = ?1)")
                .map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query(rusqlite::params![f])
                .map_err(|e| e.to_string())?;
            while let Some(row) = rows.next().map_err(|e| e.to_string())? {
                ids.insert(row.get(0).map_err(|e| e.to_string())?);
            }
        }
        Ok(ids)
    };

    let ids_a = symbol_ids_for(&stats_a.files)?;
    let ids_b = symbol_ids_for(&stats_b.files)?;

    let mut stmt = db
        .conn
        .prepare(
            "SELECT edges.source_symbol_id, edges.target_symbol_id,
                    src.name, tgt.name
             FROM edges
             JOIN symbols src ON edges.source_symbol_id = src.id
             JOIN symbols tgt ON edges.target_symbol_id = tgt.id
             WHERE edges.source_symbol_id IS NOT NULL",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    let mut a_to_b: Vec<(String, String)> = Vec::new();
    let mut b_to_a: Vec<(String, String)> = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let src_id: i64 = row.get(0).map_err(|e| e.to_string())?;
        let tgt_id: i64 = row.get(1).map_err(|e| e.to_string())?;
        let src_name: String = row.get(2).map_err(|e| e.to_string())?;
        let tgt_name: String = row.get(3).map_err(|e| e.to_string())?;

        if ids_a.contains(&src_id) && ids_b.contains(&tgt_id) {
            a_to_b.push((src_name.clone(), tgt_name.clone()));
        }
        if ids_b.contains(&src_id) && ids_a.contains(&tgt_id) {
            b_to_a.push((src_name, tgt_name));
        }
    }

    Ok(SubsystemCommunication {
        subsystem_a: a.to_string(),
        subsystem_b: b.to_string(),
        a_to_b_edges: a_to_b.len(),
        b_to_a_edges: b_to_a.len(),
        a_to_b_examples: a_to_b.into_iter().take(10).collect(),
        b_to_a_examples: b_to_a.into_iter().take(10).collect(),
    })
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SubsystemCommunication {
    pub subsystem_a: String,
    pub subsystem_b: String,
    pub a_to_b_edges: usize,
    pub b_to_a_edges: usize,
    pub a_to_b_examples: Vec<(String, String)>,
    pub b_to_a_examples: Vec<(String, String)>,
}

pub fn discover_subsystem(
    db: &Database,
    name: &str,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
) -> Result<SubsystemStats, String> {
    // 1. Seed Generation (Hybrid Lexical + Semantic + Graph)
    let (seed_results, _) = crate::engine::search_symbols(
        db,
        name,
        semantic_tokens,
        query_vector,
        !semantic_tokens.is_empty(),
        None,
        crate::engine::SearchMode::Symbol,
        false,
        Some("low"),
    )
    .map_err(|e| e.to_string())?;

    let mut matched_symbol_ids = HashSet::new();
    let mut matched_file_ids = HashSet::new();
    let mut confidence_val = "Low".to_string();
    let mut top_score = 0;

    for r in seed_results {
        if r.kind == "file" || r.kind == "text_match" {
            continue;
        }
        if r.score >= 100 || r.confidence.starts_with("High") || r.confidence.starts_with("Medium") {
            let mut stmt = db
                .conn
                .prepare(
                    "SELECT symbols.id, symbols.file_id, files.path 
                     FROM symbols JOIN files ON symbols.file_id = files.id 
                     WHERE symbols.name = ?1 AND symbols.kind = ?2",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt.query(rusqlite::params![r.name, r.kind]).map_err(|e| e.to_string())?;
            while let Ok(Some(row)) = rows.next() {
                let s_id: i64 = row.get(0).map_err(|e| e.to_string())?;
                let f_id: i64 = row.get(1).map_err(|e| e.to_string())?;
                let path: String = row.get(2).map_err(|e| e.to_string())?;
                if db.resolve_path(&path) == r.path {
                    matched_symbol_ids.insert(s_id);
                    matched_file_ids.insert(f_id);
                    if r.score > top_score {
                        top_score = r.score;
                        confidence_val = if r.confidence.starts_with("High") {
                            "High".to_string()
                        } else if r.confidence.starts_with("Medium") {
                            "Medium".to_string()
                        } else {
                            "Low".to_string()
                        };
                    }
                }
            }
        }
    }

    // 2. Graph-Based Expansion
    for _ in 0..2 {
        let mut new_symbols = HashSet::new();

        // A) Route Ownership Expansion
        for &s_id in &matched_symbol_ids {
            let mut edge_stmt = db
                .conn
                .prepare("SELECT source_symbol_id FROM edges WHERE target_symbol_id = ?1 AND source_symbol_id IS NOT NULL")
                .map_err(|e| e.to_string())?;
            let mut edge_rows = edge_stmt.query(rusqlite::params![s_id]).map_err(|e| e.to_string())?;
            while let Ok(Some(row)) = edge_rows.next() {
                let source_id: i64 = row.get(0).map_err(|e| e.to_string())?;
                if !matched_symbol_ids.contains(&source_id) {
                    let kind: String = db
                        .conn
                        .query_row(
                            "SELECT kind FROM symbols WHERE id = ?1",
                            rusqlite::params![source_id],
                            |r| r.get(0),
                        )
                        .unwrap_or_default();
                    if kind == "route" || kind == "endpoint" || kind == "page" || kind == "layout" {
                        new_symbols.insert(source_id);
                    }
                }
            }
        }

        // B) Shared Dependency Expansion
        let mut dependency_counts: std::collections::HashMap<i64, i32> = std::collections::HashMap::new();
        for &s_id in &matched_symbol_ids {
            let mut edge_stmt = db
                .conn
                .prepare("SELECT target_symbol_id FROM edges WHERE source_symbol_id = ?1")
                .map_err(|e| e.to_string())?;
            let mut edge_rows = edge_stmt.query(rusqlite::params![s_id]).map_err(|e| e.to_string())?;
            while let Ok(Some(row)) = edge_rows.next() {
                let target_id: i64 = row.get(0).map_err(|e| e.to_string())?;
                if !matched_symbol_ids.contains(&target_id) {
                    *dependency_counts.entry(target_id).or_insert(0) += 1;
                }
            }
        }

        for (target_id, count) in dependency_counts {
            let total_in: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM edges WHERE target_symbol_id = ?1",
                    rusqlite::params![target_id],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if total_in > 0 && (count as f64 / total_in as f64) >= 0.5 {
                new_symbols.insert(target_id);
            } else if count >= 2 {
                new_symbols.insert(target_id);
            }
        }

        if new_symbols.is_empty() {
            break;
        }

        for &ns in &new_symbols {
            matched_symbol_ids.insert(ns);
            if let Ok(f_id) = db.conn.query_row(
                "SELECT file_id FROM symbols WHERE id = ?1",
                rusqlite::params![ns],
                |r| r.get::<_, i64>(0),
            ) {
                matched_file_ids.insert(f_id);
            }
        }
    }

    // 3. Formatting Output
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    let mut routes = Vec::new();
    let mut page_entrypoints = Vec::new();

    for &s_id in &matched_symbol_ids {
        if let Ok((s_name, kind, f_id)) = db.conn.query_row(
            "SELECT name, kind, file_id FROM symbols WHERE id = ?1",
            rusqlite::params![s_id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, i64>(2)?)),
        ) {
            symbols.push(s_name.clone());
            if kind == "route" || kind == "endpoint" {
                routes.push(s_name.clone());
            }
            if kind == "page" || kind == "layout" {
                page_entrypoints.push(s_name);
            }
            if let Ok(path) = db.conn.query_row(
                "SELECT path FROM files WHERE id = ?1",
                rusqlite::params![f_id],
                |r| r.get::<_, String>(0),
            ) {
                files.push(path);
            }
        }
    }

    // Dependencies (outgoing from subsystem)
    let mut dependencies = HashSet::new();
    for &f_id in &matched_file_ids {
        let mut edge_stmt = db
            .conn
            .prepare(
                "SELECT symbols.name, symbols.id FROM edges 
                 JOIN symbols ON edges.target_symbol_id = symbols.id 
                 WHERE edges.source_file_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut edge_rows = edge_stmt.query(rusqlite::params![f_id]).map_err(|e| e.to_string())?;
        while let Ok(Some(row)) = edge_rows.next() {
            let s_name: String = row.get(0).map_err(|e| e.to_string())?;
            let s_id: i64 = row.get(1).map_err(|e| e.to_string())?;
            if !matched_symbol_ids.contains(&s_id) {
                dependencies.insert(s_name);
            }
        }
    }

    // Consumers (incoming to subsystem)
    let mut consumers = HashSet::new();
    for &s_id in &matched_symbol_ids {
        let mut edge_stmt = db
            .conn
            .prepare(
                "SELECT files.path, files.id FROM edges 
                 JOIN files ON edges.source_file_id = files.id 
                 WHERE edges.target_symbol_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let mut edge_rows = edge_stmt.query(rusqlite::params![s_id]).map_err(|e| e.to_string())?;
        while let Ok(Some(row)) = edge_rows.next() {
            let f_path: String = row.get(0).map_err(|e| e.to_string())?;
            let f_id: i64 = row.get(1).map_err(|e| e.to_string())?;
            if !matched_file_ids.contains(&f_id) {
                consumers.insert(f_path);
            }
        }
    }

    let mut entrypoints = HashSet::new();
    entrypoints.extend(routes.iter().cloned());
    entrypoints.extend(page_entrypoints.into_iter());

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

    // 4. Compute Hash
    let mut hash_input = String::new();
    let mut sorted_fids: Vec<i64> = matched_file_ids.into_iter().collect();
    sorted_fids.sort();
    let mut sorted_sids: Vec<i64> = matched_symbol_ids.into_iter().collect();
    sorted_sids.sort();

    for id in &sorted_fids {
        hash_input.push_str(&format!("f{}_", id));
    }
    for id in &sorted_sids {
        hash_input.push_str(&format!("s{}_", id));
    }

    let mut file_graph: std::collections::HashMap<i64, std::collections::HashSet<i64>> =
        std::collections::HashMap::new();
    for &f_id in &sorted_fids {
        file_graph.insert(f_id, std::collections::HashSet::new());
    }
    for &f_id in &sorted_fids {
        if let Ok(mut edge_stmt) = db.conn.prepare(
            "SELECT symbols.file_id FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1"
        ) {
            if let Ok(mut rows) = edge_stmt.query(rusqlite::params![f_id]) {
                while let Ok(Some(row)) = rows.next() {
                    let target_f_id: i64 = row.get(0).unwrap_or(0);
                    if sorted_fids.contains(&target_f_id) && f_id != target_f_id {
                        if let Some(set) = file_graph.get_mut(&f_id) {
                            set.insert(target_f_id);
                        }
                        if let Some(set) = file_graph.get_mut(&target_f_id) {
                            set.insert(f_id);
                        }
                    }
                }
            }
        }
    }

    let mut clusters: Vec<Vec<String>> = Vec::new();
    let mut visited = HashSet::new();
    for &f_id in &sorted_fids {
        if !visited.contains(&f_id) {
            let mut cluster = Vec::new();
            let mut stack = vec![f_id];
            visited.insert(f_id);
            while let Some(node) = stack.pop() {
                if let Ok(path) = db.conn.query_row(
                    "SELECT path FROM files WHERE id = ?1",
                    rusqlite::params![node],
                    |r| r.get::<_, String>(0),
                ) {
                    cluster.push(db.resolve_path(&path));
                }
                if let Some(neighbors) = file_graph.get(&node) {
                    for &neighbor in neighbors {
                        if !visited.contains(&neighbor) {
                            visited.insert(neighbor);
                            stack.push(neighbor);
                        }
                    }
                }
            }
            if cluster.len() > 1 {
                cluster.sort();
                clusters.push(cluster);
            }
        }
    }
    clusters.sort_by(|a, b| b.len().cmp(&a.len()));

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
        clusters,
        subsystem_hash,
        confidence: confidence_val,
    })
}
