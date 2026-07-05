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
    /// Which `TraversalScope` produced this result.
    pub scope: String,
    /// True when the discovered subsystem exceeded `MAX_SUBSYSTEM_FILES` and
    /// was cut off — a caller must be told this explicitly rather than
    /// silently receiving a partial (and possibly misleadingly "complete"
    /// looking) file list.
    pub truncated: bool,
}

/// How far `discover_subsystem`'s graph-expansion step is allowed to walk
/// past the initial lexical/semantic seed matches. Exists because the
/// previous fixed 3-hop expansion had no caller-facing control at all — a
/// cohesively-connected subsystem could pull in hundreds of files with no
/// way to ask for a narrower (or deliberately wider) view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalScope {
    /// Only the seed matches themselves — no graph expansion.
    Strict,
    /// Today's default: up to 3 cohesion-gated hops.
    Expanded,
    /// A deliberately wider dependency radius (more hops), still subject to
    /// `MAX_SUBSYSTEM_FILES`.
    Full,
}

impl TraversalScope {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "strict" => TraversalScope::Strict,
            "full" => TraversalScope::Full,
            _ => TraversalScope::Expanded,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            TraversalScope::Strict => "strict",
            TraversalScope::Expanded => "expanded",
            TraversalScope::Full => "full",
        }
    }

    fn max_hops(&self) -> usize {
        match self {
            TraversalScope::Strict => 0,
            TraversalScope::Expanded => 3,
            TraversalScope::Full => 8,
        }
    }
}

/// Hard ceiling on files returned by `discover_subsystem`, applied
/// regardless of `TraversalScope` — even `Full` must not be able to dump an
/// entire repository into one response.
pub const MAX_SUBSYSTEM_FILES: usize = 150;

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
    pub cli: Vec<EntrypointEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
            "SELECT symbols.id, symbols.name, symbols.kind, files.path, symbols.attributes
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             JOIN symbol_features f ON symbols.id = f.symbol_id
             WHERE f.is_entrypoint = 1",
        )
        .map_err(|e| e.to_string())?;

    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;
    let mut routes = Vec::new();
    let mut pages = Vec::new();
    let mut cli = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let _id: i64 = row.get(0).unwrap_or(0);
        let name: String = row.get(1).unwrap_or_default();
        let kind: String = row.get(2).unwrap_or_default();
        let path: String = row.get(3).unwrap_or_default();
        let attributes: Option<String> = row.get(4).unwrap_or(None);

        if let Some(scope) = path_scope {
            if !crate::path_matches_scope(&path, scope) {
                continue;
            }
        }

        let entry = EntrypointEntry {
            name: name.clone(),
            kind: kind.clone(),
            file_path: db.resolve_path(&path),
        };
        // Route/page/cli split uses the same shared classifier that set
        // is_entrypoint, so the bucket assignment can never disagree with detection.
        match storage::entrypoints::classify_entrypoint_json(
            &name,
            &kind,
            &path,
            attributes.as_deref(),
        ) {
            Some(storage::entrypoints::EntrypointClass::Page) => pages.push(entry),
            Some(storage::entrypoints::EntrypointClass::Cli) => cli.push(entry),
            _ => routes.push(entry),
        }
    }

    routes.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    pages.sort_by(|a, b| a.file_path.cmp(&b.file_path));
    cli.sort_by(|a, b| a.file_path.cmp(&b.file_path));

    let total = routes.len() + pages.len() + cli.len();
    let note = if total == 0 {
        Some("No entrypoints detected. This tool recognises FastAPI/Flask routes, Next.js pages, and Python CLI entry functions (def main / __main__.py). Other frameworks or languages are not yet covered.".to_string())
    } else {
        None
    };

    Ok(EntrypointReport {
        total,
        routes,
        pages,
        cli,
        note,
    })
}

/// Diffs two subsystems' edge sets to answer "how do A and B communicate":
/// counts edges whose source symbol belongs to A and target belongs to B
/// (and vice versa), with up to 10 example symbol-name pairs per direction.
/// `subsystem_stats`'s `dependencies`/`consumers` fields are symbol/file
/// lists relative to a SINGLE subsystem; answering a two-subsystem question
/// previously required calling it twice and manually diffing the two result
/// sets (benchmark run_001's gap #3).
/// `files_a`/`files_b` must come from the caller's own subsystem resolution
/// (e.g. `resolver::resolve_subsystem`'s `ResolvedSubsystem.files`) rather
/// than this function re-deriving them from `a`/`b` via a second, independent
/// `discover_subsystem` call — otherwise a name that resolved via a
/// canonicalized alias (e.g. "authentication" -> "auth") could silently
/// re-resolve differently (or fail) here, using the raw un-canonicalized
/// input. `a`/`b` are only used as output labels.
pub fn subsystem_communication(
    db: &Database,
    a: &str,
    b: &str,
    files_a: &[String],
    files_b: &[String],
) -> Result<SubsystemCommunication, String> {
    // Resolve each subsystem's files to file ids. We key communication on FILE
    // membership (source_file_id / target symbol's file_id), exactly as
    // `discover_subsystem`'s `consumers`/`dependencies` do — previously this
    // function only counted edges whose `source_symbol_id` was non-NULL and
    // resolved to a symbol, so file-level edges (e.g. a top-level
    // `import`/`fetch` from a frontend component into an orchestrator symbol,
    // whose enclosing symbol is NULL) were invisible here while still showing
    // up in `subsystem_stats(...).consumers`. The two tools now agree on
    // whether an A<->B edge exists because they read the same edge basis.
    let file_ids_for = |files: &[String]| -> Result<HashSet<i64>, String> {
        let mut ids = HashSet::new();
        for f in files {
            let mut stmt = db
                .conn
                .prepare("SELECT id FROM files WHERE path = ?1")
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

    let file_ids_a = file_ids_for(files_a)?;
    let file_ids_b = file_ids_for(files_b)?;

    // Every edge with a target symbol, carrying both the source file and the
    // target symbol's file so we can classify by file membership. The source
    // label falls back to the source file's basename when the edge has no
    // enclosing symbol (NULL source_symbol_id).
    let mut stmt = db
        .conn
        .prepare(
            "SELECT edges.source_file_id, tgt.file_id, src.name, tgt.name, srcf.path
             FROM edges
             JOIN symbols tgt ON edges.target_symbol_id = tgt.id
             JOIN files srcf ON edges.source_file_id = srcf.id
             LEFT JOIN symbols src ON edges.source_symbol_id = src.id",
        )
        .map_err(|e| e.to_string())?;
    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    let mut a_to_b: Vec<(String, String)> = Vec::new();
    let mut b_to_a: Vec<(String, String)> = Vec::new();
    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let src_file: i64 = row.get(0).map_err(|e| e.to_string())?;
        let tgt_file: i64 = row.get(1).map_err(|e| e.to_string())?;
        // Edges entirely within one file aren't cross-subsystem communication.
        if src_file == tgt_file {
            continue;
        }
        let src_name: Option<String> = row.get(2).map_err(|e| e.to_string())?;
        let tgt_name: String = row.get(3).map_err(|e| e.to_string())?;
        let src_path: String = row.get(4).map_err(|e| e.to_string())?;
        let src_label = src_name.unwrap_or_else(|| {
            std::path::Path::new(&src_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&src_path)
                .to_string()
        });

        if file_ids_a.contains(&src_file) && file_ids_b.contains(&tgt_file) {
            a_to_b.push((src_label.clone(), tgt_name.clone()));
        }
        if file_ids_b.contains(&src_file) && file_ids_a.contains(&tgt_file) {
            b_to_a.push((src_label, tgt_name));
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
    scope: TraversalScope,
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
        if r.score >= 100 || r.confidence.starts_with("High") || r.confidence.starts_with("Medium")
        {
            let mut stmt = db
                .conn
                .prepare(
                    "SELECT symbols.id, symbols.file_id, files.path 
                     FROM symbols JOIN files ON symbols.file_id = files.id 
                     WHERE symbols.name = ?1 AND symbols.kind = ?2",
                )
                .map_err(|e| e.to_string())?;
            let mut rows = stmt
                .query(rusqlite::params![r.name, r.kind])
                .map_err(|e| e.to_string())?;
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

    // 2. Graph-Based Expansion (Cohesion-Driven Localized Traversal)
    // Expand from seeds to adjacent nodes that are tightly coupled to the subsystem.
    let mut current_frontier = matched_symbol_ids.clone();
    let mut truncated = false;

    // Hop ceiling is caller-controlled via `scope` (see `TraversalScope`);
    // `MAX_SUBSYSTEM_FILES` is an absolute ceiling that applies regardless of
    // scope, so even `Full` can't pull in an entire repository.
    for _hop in 0..scope.max_hops() {
        if matched_file_ids.len() >= MAX_SUBSYSTEM_FILES {
            truncated = true;
            break;
        }
        let mut next_candidates = HashSet::new();

        // Gather all immediate neighbors of the current frontier
        for &s_id in &current_frontier {
            if let Ok(mut stmt) = db
                .conn
                .prepare("SELECT target_symbol_id FROM edges WHERE source_symbol_id = ?1")
            {
                if let Ok(mut rows) = stmt.query(rusqlite::params![s_id]) {
                    while let Ok(Some(row)) = rows.next() {
                        let tgt: i64 = row.get(0).unwrap();
                        if !matched_symbol_ids.contains(&tgt) {
                            next_candidates.insert(tgt);
                        }
                    }
                }
            }
            if let Ok(mut stmt) = db.conn.prepare("SELECT source_symbol_id FROM edges WHERE target_symbol_id = ?1 AND source_symbol_id IS NOT NULL") {
                if let Ok(mut rows) = stmt.query(rusqlite::params![s_id]) {
                    while let Ok(Some(row)) = rows.next() {
                        let src: i64 = row.get(0).unwrap();
                        if !matched_symbol_ids.contains(&src) { next_candidates.insert(src); }
                    }
                }
            }
        }

        if next_candidates.is_empty() {
            break;
        }

        let matched_list = matched_symbol_ids
            .iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let query_str = format!(
            "SELECT COUNT(*) FROM edges WHERE (source_symbol_id = ?1 AND target_symbol_id IN ({})) OR (source_symbol_id IN ({}) AND target_symbol_id = ?1)",
            matched_list, matched_list
        );

        let mut next_frontier = HashSet::new();
        let mut added_any = false;

        for &cand_id in &next_candidates {
            let cohesive_edges: i64 = db
                .conn
                .query_row(&query_str, rusqlite::params![cand_id], |r| r.get(0))
                .unwrap_or(0);

            let total_edges: i64 = db
                .conn
                .query_row(
                    "SELECT fan_in + fan_out FROM symbol_features WHERE symbol_id = ?1",
                    rusqlite::params![cand_id],
                    |r| r.get(0),
                )
                .unwrap_or(1);

            let is_local: bool = db
                .conn
                .query_row(
                    "SELECT is_local FROM symbol_features WHERE symbol_id = ?1",
                    rusqlite::params![cand_id],
                    |r| r.get(0),
                )
                .unwrap_or(false);

            if is_local {
                continue; // Skip pure local variables from expanding the subsystem boundaries
            }

            let cohesion_ratio = cohesive_edges as f64 / (total_edges as f64).max(1.0);

            // If the candidate commits at least 30% of its connectivity to the subsystem, OR it's deeply connected (>2 edges)
            if cohesion_ratio >= 0.3 || cohesive_edges >= 3 {
                matched_symbol_ids.insert(cand_id);
                next_frontier.insert(cand_id);
                added_any = true;
                if let Ok(f_id) = db.conn.query_row(
                    "SELECT file_id FROM symbols WHERE id = ?1",
                    rusqlite::params![cand_id],
                    |r| r.get::<_, i64>(0),
                ) {
                    matched_file_ids.insert(f_id);
                }
            }
        }

        if !added_any {
            break; // Cohesion dropped, subsystem boundary found
        }
        current_frontier = next_frontier;
    }

    // Final safeguard: even if the per-hop check above didn't catch it (e.g.
    // the seed matches alone already exceed the cap under `Strict`), never
    // hand back more than `MAX_SUBSYSTEM_FILES` — deterministically keep the
    // lowest file ids and drop any matched symbol whose file didn't make the
    // cut, so `files`/`symbols`/`dependencies`/`consumers` all stay
    // consistent with each other.
    if matched_file_ids.len() > MAX_SUBSYSTEM_FILES {
        truncated = true;
        let mut sorted_fids: Vec<i64> = matched_file_ids.iter().cloned().collect();
        sorted_fids.sort();
        sorted_fids.truncate(MAX_SUBSYSTEM_FILES);
        let kept_fids: HashSet<i64> = sorted_fids.into_iter().collect();
        matched_symbol_ids.retain(|&s_id| {
            db.conn
                .query_row(
                    "SELECT file_id FROM symbols WHERE id = ?1",
                    rusqlite::params![s_id],
                    |r| r.get::<_, i64>(0),
                )
                .map(|f_id| kept_fids.contains(&f_id))
                .unwrap_or(false)
        });
        matched_file_ids = kept_fids;
    }

    // 3. Formatting Output
    let mut files = Vec::new();
    let mut symbols = Vec::new();
    let mut routes = Vec::new();
    let mut page_entrypoints = Vec::new();

    for &s_id in &matched_symbol_ids {
        if let Ok((s_name, kind, f_id, attributes, path)) = db.conn.query_row(
            "SELECT symbols.name, symbols.kind, symbols.file_id, symbols.attributes, files.path
             FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.id = ?1",
            rusqlite::params![s_id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, String>(4)?,
                ))
            },
        ) {
            symbols.push(s_name.clone());
            if let Ok(Some(is_ep)) = db.conn.query_row(
                "SELECT is_entrypoint FROM symbol_features WHERE symbol_id = ?1",
                rusqlite::params![s_id],
                |r| r.get::<_, Option<bool>>(0),
            ) {
                if is_ep {
                    // Same shared classifier as detection — route/page split
                    // is always consistent across tools.
                    match storage::entrypoints::classify_entrypoint_json(
                        &s_name,
                        &kind,
                        &path,
                        attributes.as_deref(),
                    ) {
                        Some(storage::entrypoints::EntrypointClass::Page) => {
                            page_entrypoints.push(s_name.clone())
                        }
                        _ => routes.push(s_name.clone()),
                    }
                }
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
        let mut edge_rows = edge_stmt
            .query(rusqlite::params![f_id])
            .map_err(|e| e.to_string())?;
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
        let mut edge_rows = edge_stmt
            .query(rusqlite::params![s_id])
            .map_err(|e| e.to_string())?;
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
        scope: scope.as_str().to_string(),
        truncated,
    })
}
