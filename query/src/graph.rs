use rusqlite::{OptionalExtension, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};
use storage::Database;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GraphDirection {
    Incoming,
    Outgoing,
    Both,
    /// Same traversal as `Both` (visits the same node set via both incoming and
    /// outgoing edges) — provided so `explore_graph` can fully replace
    /// `graph_subtree`, whose only documented difference was "undirected".
    Undirected,
}

impl Default for GraphDirection {
    fn default() -> Self {
        GraphDirection::Both
    }
}

impl From<&str> for GraphDirection {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "incoming" => GraphDirection::Incoming,
            "outgoing" => GraphDirection::Outgoing,
            "undirected" => GraphDirection::Undirected,
            _ => GraphDirection::Both,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphEdge {
    pub source: String,
    pub target: String,
    pub kind: String,
    /// "static" (found by walking syntax) or "logical" (inferred by matching
    /// a runtime boundary, e.g. `fetch("/api/...")` resolved to its route
    /// handler) — see `graph::EdgeType`.
    #[serde(default = "default_edge_type")]
    pub edge_type: String,
}

fn default_edge_type() -> String {
    "static".to_string()
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphResponse {
    pub root: String,
    pub depth: usize,
    pub truncated: bool,
    /// Set when a single file supplies a large share of the returned nodes —
    /// a caller should treat dense same-file edges with suspicion rather than
    /// assuming every co-located symbol is genuinely coupled to the root.
    pub dominant_file_warning: Option<String>,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Flags when a single file accounts for a disproportionate share of a
/// traversal's nodes. A graph traversal that's mostly one hub file is exactly
/// the shape produced by the file-granularity edge-attribution bug (see
/// dependency_cycles/architectural_hotspots fix notes above) — even with
/// correct per-symbol edges, a caller examining a god-file's symbol should
/// know its "neighbors" may just be co-located declarations.
fn dominant_file_warning<'a>(file_paths: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    let mut total = 0usize;
    for p in file_paths {
        *counts.entry(p).or_insert(0) += 1;
        total += 1;
    }
    let (dominant_path, count) = counts.into_iter().max_by_key(|&(_, c)| c)?;
    if count >= 5 && (count as f64) / (total as f64) > 0.4 {
        Some(format!(
            "This subtree is dominated by `{}`, which supplies {} of {} nodes — verify these are real per-symbol relationships rather than an artifact of many symbols being co-located in one file.",
            dominant_path, count, total
        ))
    } else {
        None
    }
}

impl GraphResponse {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("### Graph: {}\n", self.root));
        md.push_str(&format!(
            "Depth: {}, Nodes: {}, Edges: {}\n",
            self.depth,
            self.nodes.len(),
            self.edges.len()
        ));
        if self.truncated {
            md.push_str("**Warning: Graph traversal truncated due to size limits.**\n");
        }
        if let Some(warning) = &self.dominant_file_warning {
            md.push_str(&format!("**Warning: {}**\n", warning));
        }

        let mut id_to_name = std::collections::HashMap::new();
        for node in &self.nodes {
            id_to_name.insert(node.id.clone(), format!("{} ({})", node.name, node.kind));
        }

        md.push_str("\n#### Nodes\n");
        for node in &self.nodes {
            md.push_str(&format!(
                "- **{}** ({}) in `{}`\n",
                node.name, node.kind, node.file_path
            ));
        }
        md.push_str("\n#### Relationships\n");
        for edge in &self.edges {
            let source_name = id_to_name.get(&edge.source).unwrap_or(&edge.source);
            let target_name = id_to_name.get(&edge.target).unwrap_or(&edge.target);
            let logical_tag = if edge.edge_type == "logical" {
                " [logical: runtime boundary, not a static call]"
            } else {
                ""
            };
            md.push_str(&format!(
                "* {} -> ({}) -> {}{}\n",
                source_name, edge.kind, target_name, logical_tag
            ));
        }
        md
    }

    pub fn build_json(&self, profile: crate::response::ResponseProfile) -> serde_json::Value {
        let mut nodes = Vec::new();
        for n in &self.nodes {
            nodes.push(serde_json::json!([n.id, n.name, n.kind, n.file_path]));
        }
        let mut edges = Vec::new();
        for e in &self.edges {
            if matches!(profile, crate::response::ResponseProfile::Verbose) {
                edges.push(serde_json::json!([e.source, e.target, e.kind, e.edge_type]));
            } else {
                edges.push(serde_json::json!([e.source, e.target, e.kind]));
            }
        }
        
        let mut map = serde_json::Map::new();
        map.insert("root".to_string(), serde_json::json!(self.root));
        map.insert("depth".to_string(), serde_json::json!(self.depth));
        map.insert("truncated".to_string(), serde_json::json!(self.truncated));
        if let Some(w) = &self.dominant_file_warning {
            map.insert("dominant_file_warning".to_string(), serde_json::json!(w));
        }
        map.insert("nodes".to_string(), serde_json::json!(nodes));
        map.insert("edges".to_string(), serde_json::json!(edges));
        
        serde_json::Value::Object(map)
    }
}

pub fn explore_graph(
    db: &Database,
    symbol_name: &str,
    max_depth: usize,
    direction: GraphDirection,
    max_nodes: usize,
) -> Result<GraphResponse> {
    let safe_depth = std::cmp::min(max_depth, 5);
    let max_nodes = max_nodes.clamp(1, 200);
    let max_edges = max_nodes * 3;

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Find the root symbol
    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 1",
    )?;

    let root_info: Option<(i64, String, String, String)> = stmt
        .query_row(rusqlite::params![symbol_name], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .optional()?;

    let root_info = match root_info {
        Some(info) => info,
        None => {
            return Ok(GraphResponse {
                root: symbol_name.to_string(),
                depth: safe_depth,
                truncated: false,
                dominant_file_warning: None,
                nodes: vec![],
                edges: vec![],
            });
        }
    };

    let (root_id, r_name, r_kind, r_path) = root_info;

    let mut visited_symbols = HashSet::new();
    let mut visited_edges = HashSet::new();

    // (symbol_id, current_depth)
    let mut queue = VecDeque::new();

    nodes.push(GraphNode {
        id: root_id.to_string(),
        name: r_name,
        kind: r_kind,
        file_path: r_path,
    });
    visited_symbols.insert(root_id);
    queue.push_back((root_id, 0));

    // source_symbol_id, not source_file_id: edges are attributed to the
    // specific symbol they originate from, so a query against one symbol
    // doesn't fan out to every other symbol declared in the same file.
    // NULL-source edges (top-level imports with no enclosing symbol) are
    // unattributable and intentionally excluded, matching dependency_cycles.
    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind, edge_type
         FROM edges
         WHERE source_symbol_id = ?1",
    )?;

    let mut in_stmt = db.conn.prepare(
        "SELECT source_symbol_id, kind, edge_type
         FROM edges
         WHERE target_symbol_id = ?1 AND source_symbol_id IS NOT NULL",
    )?;

    let mut resolve_sym_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.id = ?1 LIMIT 1",
    )?;

    let mut truncated = false;

    while let Some((curr_id, current_depth)) = queue.pop_front() {
        if truncated {
            break;
        }
        if current_depth >= safe_depth {
            continue;
        }

        let is_outgoing = matches!(
            direction,
            GraphDirection::Outgoing | GraphDirection::Both | GraphDirection::Undirected
        );
        let is_incoming = matches!(
            direction,
            GraphDirection::Incoming | GraphDirection::Both | GraphDirection::Undirected
        );

        // Process Outgoing dependencies
        if is_outgoing {
            let mut out_rows = out_stmt.query(rusqlite::params![curr_id])?;
            while let Some(row) = out_rows.next()? {
                if nodes.len() >= max_nodes || edges.len() >= max_edges {
                    truncated = true;
                    break;
                }

                let target_id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;
                let edge_type: String = row.get(2)?;

                // A symbol depending on itself is a self-loop with no
                // navigational value (and was a visible artifact for recursive
                // or same-named handlers, e.g. DELETE -> DELETE). Skip it.
                if target_id == curr_id {
                    continue;
                }

                let edge_key = (curr_id, target_id, kind.clone());
                if !visited_edges.contains(&edge_key) {
                    visited_edges.insert(edge_key);
                    edges.push(GraphEdge {
                        source: curr_id.to_string(),
                        target: target_id.to_string(),
                        kind,
                        edge_type,
                    });
                }

                if !visited_symbols.contains(&target_id) {
                    visited_symbols.insert(target_id);
                    queue.push_back((target_id, current_depth + 1));

                    if let Ok((id, name, kind, path)) =
                        resolve_sym_stmt.query_row(rusqlite::params![target_id], |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                            ))
                        })
                    {
                        nodes.push(GraphNode {
                            id: id.to_string(),
                            name,
                            kind,
                            file_path: path,
                        });
                    }
                }
            }
        }

        if nodes.len() >= max_nodes || edges.len() >= max_edges {
            truncated = true;
            break;
        }

        // Process Incoming dependencies
        if is_incoming {
            let mut in_rows = in_stmt.query(rusqlite::params![curr_id])?;
            while let Some(row) = in_rows.next()? {
                if nodes.len() >= max_nodes || edges.len() >= max_edges {
                    truncated = true;
                    break;
                }

                let source_id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;
                let edge_type: String = row.get(2)?;

                // Skip self-loops (e.g. recursive functions).
                if source_id == curr_id {
                    continue;
                }

                let edge_key = (source_id, curr_id, kind.clone());
                if !visited_edges.contains(&edge_key) {
                    visited_edges.insert(edge_key);
                    edges.push(GraphEdge {
                        source: source_id.to_string(),
                        target: curr_id.to_string(),
                        kind: kind.clone(),
                        edge_type,
                    });
                }

                if !visited_symbols.contains(&source_id) {
                    visited_symbols.insert(source_id);
                    queue.push_back((source_id, current_depth + 1));

                    if let Ok((id, name, kind, path)) =
                        resolve_sym_stmt.query_row(rusqlite::params![source_id], |r| {
                            Ok((
                                r.get::<_, i64>(0)?,
                                r.get::<_, String>(1)?,
                                r.get::<_, String>(2)?,
                                r.get::<_, String>(3)?,
                            ))
                        })
                    {
                        nodes.push(GraphNode {
                            id: id.to_string(),
                            name,
                            kind,
                            file_path: path,
                        });
                    }
                }
            }
        }
    }

    let dominant_file_warning = dominant_file_warning(nodes.iter().map(|n| n.file_path.as_str()));

    Ok(GraphResponse {
        root: symbol_name.to_string(),
        depth: safe_depth,
        truncated,
        dominant_file_warning,
        nodes,
        edges,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PathNode {
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PathEdge {
    pub source: String,
    pub target: String,
    pub edge_kind: String,
    /// "static" or "logical" — see `graph::EdgeType`. A path containing a
    /// logical edge crossed a runtime boundary (e.g. HTTP fetch) rather than
    /// a chain of static calls/imports.
    #[serde(default = "default_edge_type")]
    pub edge_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SuggestedConnector {
    pub route_symbol: String,
    pub file_path: String,
    pub matched_literal: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ShortestPathResponse {
    pub from: String,
    pub to: String,
    pub found: bool,
    pub distance: usize,
    pub nodes: Vec<PathNode>,
    pub edges: Vec<PathEdge>,
    /// True when the repository's dependency graph has zero edges. A caller
    /// MUST check this before treating `found: false` as "these symbols are
    /// genuinely unrelated" — with an unindexed graph, `found: false` only
    /// means no traversal was possible, not that no relationship exists.
    pub graph_unindexed: bool,
    /// Populated only when `found` is false. The static call graph can't see
    /// across an HTTP boundary (e.g. a client component calling
    /// `fetch("/api/run")`, which the route handler at `app/api/run/route.ts`
    /// answers) — there's no static edge for that relationship at all, so
    /// `found: false` is the technically correct answer but gives no signal
    /// that the real connector is one query away. When the `from` symbol's
    /// own source contains a `fetch("/api/...")`-shaped literal that matches
    /// an indexed route/endpoint symbol, this surfaces it directly instead of
    /// requiring the caller to manually read the source and re-query
    /// (benchmark run_003's central finding).
    pub suggested_connector: Option<SuggestedConnector>,
}

/// Scans `source` for `fetch("/api/...")`-shaped string literals (single,
/// double, or backtick quoted) and returns the matched URL paths in order of
/// appearance. Deliberately simple substring/char scanning rather than a
/// regex dependency — this only needs to catch the common literal-argument
/// case, not template-literal interpolation or dynamically built URLs.
fn extract_fetch_literals(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = source.as_bytes();
    let mut i = 0;
    while let Some(rel) = source[i..].find("fetch(") {
        let start = i + rel + "fetch(".len();
        let mut j = start;
        while j < bytes.len() && (bytes[j] as char).is_whitespace() {
            j += 1;
        }
        if j < bytes.len() && matches!(bytes[j] as char, '"' | '\'' | '`') {
            let quote = bytes[j] as char;
            let lit_start = j + 1;
            if let Some(end_rel) = source[lit_start..].find(quote) {
                let literal = &source[lit_start..lit_start + end_rel];
                if literal.starts_with("/api/") {
                    out.push(literal.to_string());
                }
            }
        }
        i = start;
    }
    out
}

/// Converts a literal API path like `/api/run` or `/api/rooms/[id]` into the
/// path-fragment a Next.js route handler file would live under
/// (`api/run/route`), so it can be matched against indexed file paths via a
/// simple substring check. Dynamic segments (`[id]`) are stripped to their
/// static prefix since the indexed file path uses the literal bracket
/// directory name, not a resolved value.
fn route_file_fragment(literal: &str) -> String {
    let trimmed = literal.trim_start_matches('/');
    let static_prefix: String = trimmed
        .split('/')
        .take_while(|seg| !seg.starts_with('['))
        .collect::<Vec<_>>()
        .join("/");
    if static_prefix.is_empty() {
        trimmed.to_string()
    } else {
        static_prefix
    }
}

fn find_suggested_connector(db: &Database, from_id: i64) -> Option<SuggestedConnector> {
    let (start_byte, end_byte, file_id): (i64, i64, i64) = db
        .conn
        .query_row(
            "SELECT start_byte, end_byte, file_id FROM symbols WHERE id = ?1",
            rusqlite::params![from_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .ok()?;
    let rel_path: String = db
        .conn
        .query_row(
            "SELECT path FROM files WHERE id = ?1",
            rusqlite::params![file_id],
            |row| row.get(0),
        )
        .ok()?;
    let abs_path = db.resolve_path(&rel_path);
    let content = std::fs::read(&abs_path).ok()?;
    if end_byte <= start_byte || end_byte as usize > content.len() {
        return None;
    }
    let source =
        String::from_utf8_lossy(&content[start_byte as usize..end_byte as usize]).to_string();

    for literal in extract_fetch_literals(&source) {
        let fragment = route_file_fragment(&literal);
        if fragment.is_empty() {
            continue;
        }
        let pattern = format!("%{}%", fragment);
        let found: Option<(String, String)> = db
            .conn
            .query_row(
                "SELECT symbols.name, files.path
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.kind IN ('route', 'endpoint')
                   AND files.path LIKE ?1
                 LIMIT 1",
                rusqlite::params![pattern],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .ok();
        if let Some((route_symbol, route_path)) = found {
            return Some(SuggestedConnector {
                route_symbol,
                file_path: db.resolve_path(&route_path),
                matched_literal: literal,
            });
        }
    }
    None
}

/// Runs over every indexed symbol once and persists a real, traversable
/// `EdgeType::Logical` edge for each `fetch("/api/...")` literal that
/// resolves to an indexed route/endpoint handler — turning
/// `find_suggested_connector`'s query-time-only heuristic into first-class
/// graph data so `explore_graph`/`graph_subtree`/`dependency_cycles`/
/// `impact_analysis` see the relationship too, not just `shortest_path`.
/// Call once per (re)index, after all static edges for the workspace have
/// been inserted. Returns the number of `fetch -> route` matches detected
/// this pass (each is dedup-checked against existing rows by
/// `insert_logical_edge`, so this is a detection count, not a guarantee that
/// every one was a new row — re-running on an unchanged workspace still
/// reports every match it re-detects, just without growing the table).
pub fn detect_logical_edges(db: &Database) -> Result<usize> {
    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.file_id, symbols.start_byte, symbols.end_byte, files.path
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.kind NOT IN ('route', 'endpoint')
           AND symbols.end_byte > symbols.start_byte",
    )?;
    let mut rows = stmt.query([])?;

    let mut file_cache: std::collections::HashMap<i64, Vec<u8>> = std::collections::HashMap::new();
    let mut inserted = 0usize;

    let mut candidates = Vec::new();
    while let Some(row) = rows.next()? {
        candidates.push((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?,
            row.get::<_, String>(4)?,
        ));
    }
    drop(rows);

    for (symbol_id, file_id, start_byte, end_byte, rel_path) in candidates {
        let content = match file_cache.get(&file_id) {
            Some(c) => c,
            None => {
                let abs_path = db.resolve_path(&rel_path);
                let bytes = match std::fs::read(&abs_path) {
                    Ok(b) => b,
                    Err(_) => continue,
                };
                file_cache.insert(file_id, bytes);
                file_cache.get(&file_id).unwrap()
            }
        };
        if end_byte as usize > content.len() {
            continue;
        }
        let source =
            String::from_utf8_lossy(&content[start_byte as usize..end_byte as usize]).to_string();

        for literal in extract_fetch_literals(&source) {
            let fragment = route_file_fragment(&literal);
            if fragment.is_empty() {
                continue;
            }
            let pattern = format!("%{}%", fragment);
            let found: Option<(i64, String)> = db
                .conn
                .query_row(
                    "SELECT symbols.id, files.path
                     FROM symbols
                     JOIN files ON symbols.file_id = files.id
                     WHERE symbols.kind IN ('route', 'endpoint')
                       AND files.path LIKE ?1
                       AND symbols.file_id != ?2
                     LIMIT 1",
                    rusqlite::params![pattern, file_id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .ok();
            if let Some((route_symbol_id, _route_path)) = found {
                db.insert_logical_edge(file_id, Some(symbol_id), route_symbol_id, "fetches")?;
                inserted += 1;
            }
        }
    }

    Ok(inserted)
}

pub fn shortest_path(
    db: &Database,
    from_symbol: &str,
    to_symbol: &str,
    from_file_hint: Option<&str>,
    to_file_hint: Option<&str>,
) -> Result<ShortestPathResponse> {
    let total_edges: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap_or(0);
    let graph_unindexed = total_edges == 0;

    // 1. Find root and target symbol IDs. Unlike `get_context`/`find_symbol`/
    // `get_edit_context`, this previously had no way to disambiguate a name
    // that matches multiple symbols (e.g. two different `createClient`
    // functions, one per Supabase client/server module) — `LIMIT 1` with no
    // ORDER BY just took whichever row SQLite happened to return first,
    // which could silently be the WRONG symbol, making a real path look like
    // `found: false`. `from_file_hint`/`to_file_hint` let a caller pin each
    // endpoint to the file they actually mean, the same way `get_context`'s
    // `file_path` does.
    let lookup =
        |name: &str, hint: Option<&str>| -> Result<Option<(i64, String, String, String)>> {
            if let Some(h) = hint {
                let mut stmt = db.conn.prepare(
                    "SELECT symbols.id, symbols.name, symbols.kind, files.path
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 AND INSTR(files.path, ?2) > 0 LIMIT 1",
                )?;
                stmt.query_row(rusqlite::params![name, h], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .optional()
            } else {
                let mut stmt = db.conn.prepare(
                    "SELECT symbols.id, symbols.name, symbols.kind, files.path
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 LIMIT 1",
                )?;
                stmt.query_row(rusqlite::params![name], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .optional()
            }
        };

    let from_info = lookup(from_symbol, from_file_hint)?;
    let to_info = lookup(to_symbol, to_file_hint)?;

    if from_info.is_none() || to_info.is_none() {
        let suggested_connector = from_info
            .as_ref()
            .and_then(|info| find_suggested_connector(db, info.0));
        return Ok(ShortestPathResponse {
            from: from_symbol.to_string(),
            to: to_symbol.to_string(),
            found: false,
            distance: 0,
            nodes: vec![],
            edges: vec![],
            graph_unindexed,
            suggested_connector,
        });
    }

    let from_info = from_info.unwrap();
    let to_info = to_info.unwrap();

    let from_id = from_info.0;
    let to_id = to_info.0;

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    // Maps target_id -> (source_id, edge_kind, edge_type)
    let mut parent_map: std::collections::HashMap<i64, (i64, String, String)> =
        std::collections::HashMap::new();

    queue.push_back(from_id);
    visited.insert(from_id);

    // source_symbol_id, not source_file_id: see explore_graph for why
    // file-granularity traversal manufactures phantom edges to every other
    // symbol in a hub file. Includes logical edges (HTTP/websocket boundary
    // connectors) on equal footing with static ones — a logical edge IS a
    // real row in this table once detect_logical_edges has run, so no
    // separate query/union is needed here.
    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind, edge_type
         FROM edges
         WHERE source_symbol_id = ?1",
    )?;

    let mut found = false;

    while let Some(curr_id) = queue.pop_front() {
        if curr_id == to_id {
            found = true;
            break;
        }

        let mut out_rows = out_stmt.query(rusqlite::params![curr_id])?;
        while let Some(row) = out_rows.next()? {
            let target_id: i64 = row.get(0)?;
            let kind: String = row.get(1)?;
            let edge_type: String = row.get(2)?;

            if !visited.contains(&target_id) {
                visited.insert(target_id);
                parent_map.insert(target_id, (curr_id, kind, edge_type));
                queue.push_back(target_id);

                // Early exit if we found the target
                if target_id == to_id {
                    found = true;
                    break;
                }
            }
        }
        if found {
            break;
        }
    }

    if !found {
        let suggested_connector = find_suggested_connector(db, from_id);
        return Ok(ShortestPathResponse {
            from: from_symbol.to_string(),
            to: to_symbol.to_string(),
            found: false,
            distance: 0,
            nodes: vec![],
            edges: vec![],
            graph_unindexed,
            suggested_connector,
        });
    }

    // Reconstruct path
    let mut path_edges = Vec::new();
    let mut path_nodes = Vec::new();
    let mut current = to_id;

    // We will collect edges backwards from 'to' to 'from'
    let mut path_ids = vec![current];
    let mut edges_backward = Vec::new();

    while current != from_id {
        if let Some(&(parent_id, ref kind, ref edge_type)) = parent_map.get(&current) {
            edges_backward.push((parent_id, current, kind.clone(), edge_type.clone()));
            current = parent_id;
            path_ids.push(current);
        } else {
            break;
        }
    }

    // Reverse to get 'from' -> 'to'
    path_ids.reverse();
    edges_backward.reverse();

    let mut resolve_sym_stmt = db.conn.prepare(
        "SELECT symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.id = ?1 LIMIT 1",
    )?;

    for id in path_ids {
        if let Ok((name, kind, path)) = resolve_sym_stmt.query_row(rusqlite::params![id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            path_nodes.push(PathNode {
                symbol_name: name,
                symbol_kind: kind,
                file_path: path,
            });
        }
    }

    for (src_id, tgt_id, kind, edge_type) in edges_backward {
        // Let's just use IDs as strings or resolve them
        let src_name_str = resolve_sym_stmt
            .query_row(rusqlite::params![src_id], |r| r.get::<_, String>(0))
            .unwrap_or_else(|_| src_id.to_string());
        let tgt_name_str = resolve_sym_stmt
            .query_row(rusqlite::params![tgt_id], |r| r.get::<_, String>(0))
            .unwrap_or_else(|_| tgt_id.to_string());

        path_edges.push(PathEdge {
            source: src_name_str,
            target: tgt_name_str,
            edge_kind: kind,
            edge_type,
        });
    }

    Ok(ShortestPathResponse {
        from: from_symbol.to_string(),
        to: to_symbol.to_string(),
        found: true,
        distance: path_edges.len(),
        nodes: path_nodes,
        edges: path_edges,
        graph_unindexed,
        suggested_connector: None,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HotspotNode {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub incoming_edges: usize,
    pub outgoing_edges: usize,
    pub total_edges: usize,
    pub hotspot_score: usize,
    pub classification: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FileHotspot {
    pub file_path: String,
    pub symbol_count: usize,
    pub total_incoming: usize,
    pub total_outgoing: usize,
    pub aggregate_score: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct HotspotResponse {
    pub total_symbols: usize,
    pub total_edges: usize,
    pub top_hotspots: Vec<HotspotNode>,
    /// File-level rollup of the same per-symbol scores: `architectural_hotspots`
    /// only ever ranked individual symbols, so "which FILE is most critical"
    /// (benchmark run_001's gap #2 — "most critical files" had to be inferred
    /// by eyeballing which files repeatedly appeared among top symbols) had
    /// no direct answer. Computed by summing each file's symbols' already-
    /// computed `hotspot_score`/incoming/outgoing, so it stays consistent
    /// with `top_hotspots` by construction rather than being a separate
    /// query.
    pub top_file_hotspots: Vec<FileHotspot>,
}

pub fn architectural_hotspots(
    db: &Database,
    limit: usize,
    path_scope: Option<&str>,
) -> Result<HotspotResponse> {
    // Per-symbol attribution, not file-granularity: joining on
    // `source_file_id = symbols.file_id` made EVERY symbol declared in a file
    // the source/target of EVERY edge touching that file, so co-located
    // symbols in a hub file all reported identical incoming/outgoing counts
    // (and identical hotspot_score ties). Using source_symbol_id/
    // target_symbol_id collapses each edge to the one symbol it actually
    // belongs to, matching the fix already applied to graph_subtree,
    // explore_graph, and dependency_cycles. Edges with a NULL
    // source_symbol_id (unattributable top-level imports) are excluded from
    // the outgoing count for the same reason they're excluded elsewhere.
    let mut incoming_map = std::collections::HashMap::new();
    let mut in_stmt = db.conn.prepare(
        "SELECT target_symbol_id, COUNT(*)
         FROM edges
         WHERE target_symbol_id IS NOT NULL
         GROUP BY target_symbol_id",
    )?;

    let mut in_rows = in_stmt.query([])?;
    while let Some(row) = in_rows.next()? {
        let sym_id: i64 = row.get(0)?;
        let count: i64 = row.get(1)?;
        incoming_map.insert(sym_id, count);
    }

    let mut outgoing_map = std::collections::HashMap::new();
    let mut out_stmt = db.conn.prepare(
        "SELECT source_symbol_id, COUNT(*)
         FROM edges
         WHERE source_symbol_id IS NOT NULL
         GROUP BY source_symbol_id",
    )?;

    let mut out_rows = out_stmt.query([])?;
    while let Some(row) = out_rows.next()? {
        let sym_id: i64 = row.get(0)?;
        let count: i64 = row.get(1)?;
        outgoing_map.insert(sym_id, count);
    }

    let mut nodes = Vec::new();
    let mut sym_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path
         FROM symbols
         JOIN files ON symbols.file_id = files.id",
    )?;

    let mut sym_rows = sym_stmt.query([])?;
    while let Some(row) = sym_rows.next()? {
        let sym_id: i64 = row.get(0)?;
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let path: String = row.get(3)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }

        let incoming = *incoming_map.get(&sym_id).unwrap_or(&0);
        let outgoing = *outgoing_map.get(&sym_id).unwrap_or(&0);
        let total = incoming + outgoing;
        let score = (incoming * 3) + outgoing;

        let classification = if incoming > 20 {
            "critical"
        } else if incoming > 10 {
            "important"
        } else {
            "normal"
        };

        nodes.push(HotspotNode {
            name,
            kind,
            file_path: path,
            incoming_edges: incoming as usize,
            outgoing_edges: outgoing as usize,
            total_edges: total as usize,
            hotspot_score: score as usize,
            classification: classification.to_string(),
        });
    }

    nodes.sort_by(|a, b| {
        b.hotspot_score
            .cmp(&a.hotspot_score)
            .then(b.incoming_edges.cmp(&a.incoming_edges))
            .then(b.total_edges.cmp(&a.total_edges))
    });

    let total_symbols = nodes.len();
    let total_edges: i64 = if let Some(scope) = path_scope {
        let pattern = format!("%{}%", scope);
        db.conn.query_row(
            "SELECT COUNT(*) FROM edges JOIN files ON edges.source_file_id = files.id WHERE files.path LIKE ?1",
            rusqlite::params![pattern],
            |r| r.get(0),
        ).unwrap_or(0)
    } else {
        db.conn
            .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
            .unwrap_or(0)
    };

    // Roll up file-level aggregates from the FULL (pre-truncation) node set,
    // so a file with many moderately-scored symbols can still outrank a file
    // with one single very high-scoring symbol — the rollup is computed
    // before `nodes.truncate(limit)` below for that reason.
    let mut file_rollup: std::collections::HashMap<String, FileHotspot> =
        std::collections::HashMap::new();
    for node in &nodes {
        let entry = file_rollup
            .entry(node.file_path.clone())
            .or_insert_with(|| FileHotspot {
                file_path: node.file_path.clone(),
                symbol_count: 0,
                total_incoming: 0,
                total_outgoing: 0,
                aggregate_score: 0,
            });
        entry.symbol_count += 1;
        entry.total_incoming += node.incoming_edges;
        entry.total_outgoing += node.outgoing_edges;
        entry.aggregate_score += node.hotspot_score;
    }
    let mut top_file_hotspots: Vec<FileHotspot> = file_rollup.into_values().collect();
    top_file_hotspots.sort_by(|a, b| {
        b.aggregate_score
            .cmp(&a.aggregate_score)
            .then(b.total_incoming.cmp(&a.total_incoming))
            .then(a.file_path.cmp(&b.file_path))
    });
    top_file_hotspots.truncate(limit);

    nodes.truncate(limit);

    Ok(HotspotResponse {
        total_symbols,
        total_edges: total_edges as usize,
        top_hotspots: nodes,
        top_file_hotspots,
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CycleNode {
    pub name: String,
    pub kind: String,
    pub file_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyCycle {
    pub length: usize,
    /// True when this cycle spans more than one distinct file (a real
    /// cross-module import cycle). False means every node in the cycle lives
    /// in the same file — almost always benign same-file mutual recursion,
    /// not the architectural problem most callers asking about cycles care
    /// about. Narrowing path_scope shrinks the graph down toward exactly the
    /// same-file edges that always survive scoping, which is why cycle
    /// results used to get noisier (not cleaner) at smaller scope — this
    /// flag is what lets a caller filter that noise back out.
    pub cross_file: bool,
    pub nodes: Vec<CycleNode>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DependencyCyclesResponse {
    pub cycles_found: usize,
    pub cross_file_cycles_found: usize,
    pub same_file_cycles_found: usize,
    pub cycles_returned: usize,
    pub truncated: bool,
    pub nodes_scanned: usize,
    pub edges_scanned: usize,
    pub cycles: Vec<DependencyCycle>,
    /// Up to 3 representative same-file cycles, always populated when
    /// same_file_cycles_found > 0 — even when include_same_file is false and
    /// they're therefore excluded from `cycles` — so a caller can sanity-check
    /// the same_file_cycles_found count without a second query.
    pub same_file_examples: Vec<DependencyCycle>,
}

pub fn dependency_cycles(
    db: &Database,
    limit: usize,
    path_scope: Option<&str>,
    include_same_file: bool,
) -> Result<DependencyCyclesResponse> {
    const MAX_CYCLES: usize = 500;
    let return_limit = limit.clamp(1, MAX_CYCLES);
    const MAX_NODES_PER_CYCLE: usize = 40;

    let mut adj: std::collections::HashMap<i64, Vec<i64>> = std::collections::HashMap::new();
    // Build a true symbol->symbol graph from edges attributed to their source
    // symbol. The previous query joined `symbols.file_id = edges.source_file_id`,
    // which made EVERY symbol in a file the source of EVERY edge leaving that
    // file — a cartesian product that manufactured same-file "cycles" (a file
    // with k symbols and e edges produced up to k*e fake adjacencies). Using
    // source_symbol_id collapses each edge to exactly one source symbol.
    // Edges with a NULL source_symbol_id (top-level imports, or any DB indexed
    // before this column existed) are excluded — they can't be placed at symbol
    // granularity, and including them is precisely what produced the noise.
    let mut stmt = db.conn.prepare(
        "SELECT edges.source_symbol_id, edges.target_symbol_id, files.path
         FROM edges
         JOIN symbols ON edges.source_symbol_id = symbols.id
         JOIN files ON symbols.file_id = files.id
         WHERE edges.source_symbol_id IS NOT NULL",
    )?;

    let mut rows = stmt.query([])?;
    let mut edges_scanned = 0;
    while let Some(row) = rows.next()? {
        let source_id: i64 = row.get(0)?;
        let target_id: i64 = row.get(1)?;
        let path: String = row.get(2)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        // A symbol referencing itself (direct recursion, or a recursive type
        // referencing its own name) is not a dependency cycle in the sense
        // callers care about; drop these self-edges so they don't register as
        // length-1 cycles.
        if source_id == target_id {
            continue;
        }
        adj.entry(source_id).or_default().push(target_id);
        edges_scanned += 1;
    }

    let mut nodes_scanned = 0;
    let mut visited: HashSet<i64> = HashSet::new();
    let mut in_stack: HashSet<i64> = HashSet::new();
    let mut path: Vec<i64> = Vec::new();
    let mut unique_cycles: HashSet<Vec<i64>> = HashSet::new();
    let mut truncated = false;

    let all_nodes: Vec<i64> = adj.keys().copied().collect();

    struct DfsState<'a> {
        adj: &'a std::collections::HashMap<i64, Vec<i64>>,
        visited: &'a mut std::collections::HashSet<i64>,
        in_stack: &'a mut std::collections::HashSet<i64>,
        path: &'a mut Vec<i64>,
        unique_cycles: &'a mut std::collections::HashSet<Vec<i64>>,
        truncated: &'a mut bool,
        nodes_scanned: &'a mut usize,
    }

    fn dfs(node: i64, state: &mut DfsState) {
        if *state.truncated {
            return;
        }

        state.visited.insert(node);
        *state.nodes_scanned += 1;
        state.in_stack.insert(node);
        state.path.push(node);

        if let Some(neighbors) = state.adj.get(&node) {
            for &neighbor in neighbors {
                if *state.truncated {
                    break;
                }
                if state.in_stack.contains(&neighbor) {
                    if let Some(pos) = state.path.iter().position(|&x| x == neighbor) {
                        let cycle_slice = &state.path[pos..];

                        let min_pos = cycle_slice
                            .iter()
                            .enumerate()
                            .min_by_key(|&(_, &val)| val)
                            .map(|(idx, _)| idx)
                            .unwrap_or(0);

                        let mut canonical = Vec::with_capacity(cycle_slice.len());
                        canonical.extend_from_slice(&cycle_slice[min_pos..]);
                        canonical.extend_from_slice(&cycle_slice[..min_pos]);

                        if state.unique_cycles.insert(canonical) {
                            if state.unique_cycles.len() >= MAX_CYCLES {
                                *state.truncated = true;
                                break;
                            }
                        }
                    }
                } else if !state.visited.contains(&neighbor) {
                    dfs(neighbor, state);
                }
            }
        }

        state.in_stack.remove(&node);
        state.path.pop();
    }

    for &node in &all_nodes {
        if truncated {
            break;
        }
        if !visited.contains(&node) {
            let mut state = DfsState {
                adj: &adj,
                visited: &mut visited,
                in_stack: &mut in_stack,
                path: &mut path,
                unique_cycles: &mut unique_cycles,
                truncated: &mut truncated,
                nodes_scanned: &mut nodes_scanned,
            };
            dfs(node, &mut state);
        }
    }

    let mut sym_stmt = db.conn.prepare(
        "SELECT symbols.name, symbols.kind, files.path
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.id = ?1 LIMIT 1",
    )?;

    let cycles_found = unique_cycles.len();
    let mut cross_file_cycles: Vec<DependencyCycle> = Vec::new();
    let mut same_file_cycles: Vec<DependencyCycle> = Vec::new();
    for cycle_ids in unique_cycles.into_iter() {
        let mut cycle_nodes = Vec::new();
        for &id in cycle_ids.iter().take(MAX_NODES_PER_CYCLE) {
            if let Ok((name, kind, path)) = sym_stmt.query_row(rusqlite::params![id], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            }) {
                cycle_nodes.push(CycleNode {
                    name,
                    kind,
                    file_path: path,
                });
            }
        }
        let distinct_files: HashSet<&str> =
            cycle_nodes.iter().map(|n| n.file_path.as_str()).collect();
        let cross_file = distinct_files.len() > 1;
        let cycle = DependencyCycle {
            length: cycle_ids.len(),
            cross_file,
            nodes: cycle_nodes,
        };
        if cross_file {
            cross_file_cycles.push(cycle);
        } else {
            same_file_cycles.push(cycle);
        }
    }

    let cross_file_cycles_found = cross_file_cycles.len();
    let same_file_cycles_found = same_file_cycles.len();

    // Always surface a few same-file cycles for sanity-checking the count,
    // even when they're excluded from `cycles` (include_same_file=false).
    let same_file_examples: Vec<DependencyCycle> =
        same_file_cycles.iter().take(3).cloned().collect();

    // Cross-file cycles are the higher-signal result, so they always come
    // first; same-file ones are only included (and only count toward the
    // limit) when the caller explicitly opts in.
    let mut ordered = cross_file_cycles;
    let considered_total = if include_same_file {
        ordered.extend(same_file_cycles);
        cycles_found
    } else {
        cross_file_cycles_found
    };

    let cycles: Vec<DependencyCycle> = ordered.into_iter().take(return_limit).collect();

    Ok(DependencyCyclesResponse {
        cycles_found,
        cross_file_cycles_found,
        same_file_cycles_found,
        cycles_returned: cycles.len(),
        truncated: truncated || considered_total > cycles.len(),
        nodes_scanned,
        edges_scanned,
        cycles,
        same_file_examples,
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubtreeNode {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub depth: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubtreeEdge {
    pub source: String,
    pub target: String,
    pub edge_kind: String,
    #[serde(default = "default_edge_type")]
    pub edge_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphSubtreeResponse {
    pub root: String,
    pub depth: usize,
    pub node_count: usize,
    pub edge_count: usize,
    /// True if traversal stopped early because max_nodes/max_edges was hit —
    /// the subtree is incomplete. Lower `depth` or raise `max_nodes` (capped
    /// at 500) and retry if you need the full picture.
    pub truncated: bool,
    /// Set when a single file supplies a large share of the returned nodes —
    /// see `dominant_file_warning` for why this matters for hub files.
    pub dominant_file_warning: Option<String>,
    pub nodes: Vec<SubtreeNode>,
    pub edges: Vec<SubtreeEdge>,
}

impl GraphSubtreeResponse {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str(&format!("### Subtree: {}\n", self.root));
        md.push_str(&format!(
            "Depth: {}, Nodes: {}, Edges: {}\n",
            self.depth, self.node_count, self.edge_count
        ));
        if self.truncated {
            md.push_str("**Warning: Subtree traversal truncated due to size limits.**\n");
        }
        if let Some(warning) = &self.dominant_file_warning {
            md.push_str(&format!("**Warning: {}**\n", warning));
        }

        md.push_str("\n#### Nodes\n");
        for node in &self.nodes {
            md.push_str(&format!(
                "- **{}** ({}) in `{}` [depth: {}]\n",
                node.name, node.kind, node.file_path, node.depth
            ));
        }
        md.push_str("\n#### Relationships\n");
        for edge in &self.edges {
            let logical_tag = if edge.edge_type == "logical" {
                " [logical: runtime boundary, not a static call]"
            } else {
                ""
            };
            md.push_str(&format!(
                "* {} -> ({}) -> {}{}\n",
                edge.source, edge.edge_kind, edge.target, logical_tag
            ));
        }
        md
    }

    pub fn build_json(&self, profile: crate::response::ResponseProfile) -> serde_json::Value {
        let mut nodes = Vec::new();
        for n in &self.nodes {
            nodes.push(serde_json::json!([n.name, n.kind, n.file_path]));
        }
        let mut edges = Vec::new();
        for e in &self.edges {
            if matches!(profile, crate::response::ResponseProfile::Verbose) {
                edges.push(serde_json::json!([e.source, e.target, e.edge_kind, e.edge_type]));
            } else {
                edges.push(serde_json::json!([e.source, e.target, e.edge_kind]));
            }
        }
        
        let mut map = serde_json::Map::new();
        map.insert("root".to_string(), serde_json::json!(self.root));
        map.insert("depth".to_string(), serde_json::json!(self.depth));
        map.insert("node_count".to_string(), serde_json::json!(self.node_count));
        map.insert("edge_count".to_string(), serde_json::json!(self.edge_count));
        map.insert("truncated".to_string(), serde_json::json!(self.truncated));
        if let Some(w) = &self.dominant_file_warning {
            map.insert("dominant_file_warning".to_string(), serde_json::json!(w));
        }
        map.insert("nodes".to_string(), serde_json::json!(nodes));
        map.insert("edges".to_string(), serde_json::json!(edges));
        
        serde_json::Value::Object(map)
    }
}

pub fn graph_subtree(
    db: &Database,
    root_symbol: &str,
    depth: usize,
    max_nodes: Option<usize>,
) -> Result<GraphSubtreeResponse> {
    let safe_depth = std::cmp::min(depth, 5);
    // Previously a fixed 500 with no escape hatch on hub symbols. Defaulting
    // to 100 matches explore_graph's default, while still allowing a caller
    // to raise it (capped at 500) when they actually want the wide view.
    let max_nodes = max_nodes.unwrap_or(100).clamp(1, 500);
    // Edges were previously collected with no bound at all — only node
    // creation was gated by max_nodes, and the underlying outgoing/incoming
    // queries below are file-granularity (a busy file's edges all get pulled
    // in per visited node), so a hub symbol could still produce tens of
    // thousands of edges even when correctly stopped at max_nodes nodes.
    // Mirrors explore_graph's max_edges bound.
    let max_edges = max_nodes * 3;

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 1",
    )?;

    let root_info: Option<(i64, String, String, String)> = stmt
        .query_row(rusqlite::params![root_symbol], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .optional()?;

    let root_info = match root_info {
        Some(info) => info,
        None => {
            return Ok(GraphSubtreeResponse {
                root: root_symbol.to_string(),
                depth: safe_depth,
                node_count: 0,
                edge_count: 0,
                truncated: false,
                dominant_file_warning: None,
                nodes: vec![],
                edges: vec![],
            });
        }
    };

    let (root_id, r_name, r_kind, r_path) = root_info;

    let mut visited_symbols = HashSet::new();
    let mut visited_edges = HashSet::new();

    let mut queue = VecDeque::new();

    nodes.push(SubtreeNode {
        name: r_name,
        kind: r_kind,
        file_path: r_path,
        depth: 0,
    });
    visited_symbols.insert(root_id);
    queue.push_back((root_id, 0));

    // source_symbol_id, not source_file_id: see explore_graph for why
    // file-granularity traversal manufactures phantom edges to every other
    // symbol in a hub file.
    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind, edge_type
         FROM edges
         WHERE source_symbol_id = ?1",
    )?;

    let mut in_stmt = db.conn.prepare(
        "SELECT source_symbol_id, kind, edge_type
         FROM edges
         WHERE target_symbol_id = ?1 AND source_symbol_id IS NOT NULL",
    )?;

    let mut resolve_sym_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.id = ?1 LIMIT 1",
    )?;

    let mut actual_depth_reached = 0;
    let mut truncated = false;

    while let Some((curr_id, current_depth)) = queue.pop_front() {
        if truncated {
            break;
        }
        if current_depth > actual_depth_reached {
            actual_depth_reached = current_depth;
        }

        if current_depth >= safe_depth {
            continue;
        }

        // Outgoing
        let mut out_rows = out_stmt.query(rusqlite::params![curr_id])?;
        while let Some(row) = out_rows.next()? {
            if nodes.len() >= max_nodes || edges.len() >= max_edges {
                truncated = true;
                break;
            }

            let target_id: i64 = row.get(0)?;
            let kind: String = row.get(1)?;
            let edge_type: String = row.get(2)?;

            let edge_key = (curr_id, target_id, kind.clone());
            if !visited_edges.contains(&edge_key) {
                visited_edges.insert(edge_key);

                let src_name_str = resolve_sym_stmt
                    .query_row(rusqlite::params![curr_id], |r| r.get::<_, String>(1))
                    .unwrap_or_else(|_| curr_id.to_string());
                let tgt_name_str = resolve_sym_stmt
                    .query_row(rusqlite::params![target_id], |r| r.get::<_, String>(1))
                    .unwrap_or_else(|_| target_id.to_string());

                edges.push(SubtreeEdge {
                    source: src_name_str,
                    target: tgt_name_str,
                    edge_kind: kind,
                    edge_type,
                });
            }

            if !visited_symbols.contains(&target_id) && nodes.len() < max_nodes {
                visited_symbols.insert(target_id);
                queue.push_back((target_id, current_depth + 1));

                if let Ok((_id, name, kind, path)) =
                    resolve_sym_stmt.query_row(rusqlite::params![target_id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                {
                    nodes.push(SubtreeNode {
                        name,
                        kind,
                        file_path: path,
                        depth: current_depth + 1,
                    });
                }
            }
        }

        if truncated || nodes.len() >= max_nodes || edges.len() >= max_edges {
            truncated = true;
            break;
        }

        // Incoming
        let mut in_rows = in_stmt.query(rusqlite::params![curr_id])?;
        while let Some(row) = in_rows.next()? {
            if nodes.len() >= max_nodes || edges.len() >= max_edges {
                truncated = true;
                break;
            }

            let source_id: i64 = row.get(0)?;
            let kind: String = row.get(1)?;
            let edge_type: String = row.get(2)?;

            // Skip self-loops (e.g. recursive functions).
            if source_id == curr_id {
                continue;
            }

            let edge_key = (source_id, curr_id, kind.clone());
            if !visited_edges.contains(&edge_key) {
                visited_edges.insert(edge_key);

                let src_name_str = resolve_sym_stmt
                    .query_row(rusqlite::params![source_id], |r| r.get::<_, String>(1))
                    .unwrap_or_else(|_| source_id.to_string());
                let tgt_name_str = resolve_sym_stmt
                    .query_row(rusqlite::params![curr_id], |r| r.get::<_, String>(1))
                    .unwrap_or_else(|_| curr_id.to_string());

                edges.push(SubtreeEdge {
                    source: src_name_str,
                    target: tgt_name_str,
                    edge_kind: kind.clone(),
                    edge_type,
                });
            }

            if !visited_symbols.contains(&source_id) && nodes.len() < max_nodes {
                visited_symbols.insert(source_id);
                queue.push_back((source_id, current_depth + 1));

                if let Ok((_id, name, kind, path)) =
                    resolve_sym_stmt.query_row(rusqlite::params![source_id], |r| {
                        Ok((
                            r.get::<_, i64>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, String>(2)?,
                            r.get::<_, String>(3)?,
                        ))
                    })
                {
                    nodes.push(SubtreeNode {
                        name,
                        kind,
                        file_path: path,
                        depth: current_depth + 1,
                    });
                }
            }
        }
    }

    let dominant_file_warning = dominant_file_warning(nodes.iter().map(|n| n.file_path.as_str()));

    Ok(GraphSubtreeResponse {
        root: root_symbol.to_string(),
        depth: actual_depth_reached,
        node_count: nodes.len(),
        edge_count: edges.len(),
        truncated,
        dominant_file_warning,
        nodes,
        edges,
    })
}

#[cfg(test)]
mod shortest_path_tests {
    use super::*;

    fn sym(db: &Database, file_id: i64, name: &str) -> i64 {
        db.insert_symbol(
            file_id,
            &graph::SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 0,
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap()
    }

    // Regression: an ambiguous symbol name (two distinct `createClient`
    // functions, one per file — a real pattern for e.g. a browser vs. server
    // Supabase client) must be resolvable via file_hint, and an unscoped
    // lookup must not silently wander onto the wrong one. Before file hints
    // existed, `shortest_path` had no way to pick the intended endpoint when
    // a name collided, which could make a real path look like `found: false`
    // purely because the wrong same-named node was searched from/to.
    #[test]
    fn file_hint_disambiguates_same_named_symbols() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();

        let browser_file = db.insert_file("utils/supabase/client.ts", "h").unwrap();
        let server_file = db.insert_file("utils/supabase/server.ts", "h").unwrap();
        let _browser_client = sym(&db, browser_file, "createClient");
        let server_client = sym(&db, server_file, "createClient");

        let room_file = db.insert_file("server/room.ts", "h").unwrap();
        let use_room = sym(&db, room_file, "useRoom");

        // Only the SERVER createClient is actually reachable from useRoom.
        db.insert_edge_attributed(room_file, Some(use_room), server_client, "calls")
            .unwrap();

        // Scoped to the server file: must find the real path.
        let scoped =
            shortest_path(&db, "useRoom", "createClient", None, Some("server.ts")).unwrap();
        assert!(
            scoped.found,
            "scoping 'to' to server.ts must find the real useRoom -> createClient edge"
        );

        // Scoped to the (unconnected) browser file: must correctly report no path,
        // not silently fall back to the connected one.
        let wrong_scope =
            shortest_path(&db, "useRoom", "createClient", None, Some("client.ts")).unwrap();
        assert!(
            !wrong_scope.found,
            "scoping 'to' to the unrelated browser client.ts must not find a path"
        );
    }

    // Regression for benchmark run_003's central finding: when two symbols
    // are only related via an HTTP boundary (a client component calling
    // `fetch("/api/run")`, answered by the route handler in
    // `app/api/run/route.ts`), there is no static edge between them, so
    // `found: false` is technically correct but uninformative on its own.
    // `shortest_path` must detect the fetch literal in the `from` symbol's
    // source and suggest the real connector route handler.
    #[test]
    fn suggests_http_boundary_connector_when_no_static_path_exists() {
        let unique = format!(
            "codebroker_test_httpboundary_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(dir.join("app/api/run")).unwrap();
        std::fs::create_dir_all(dir.join(".codebroker")).unwrap();

        let run_code_source =
            "function runCode() {\n  return fetch(\"/api/run\", { method: \"POST\" });\n}\n";
        std::fs::write(dir.join("runCode.ts"), run_code_source).unwrap();

        let route_source = "function POST() {\n  return normalizeProblem();\n}\n";
        std::fs::write(dir.join("app/api/run/route.ts"), route_source).unwrap();

        let db_path = dir.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();

        let run_code_file = db
            .insert_file(
                "runCode.ts",
                &storage::hash_content(run_code_source.as_bytes()),
            )
            .unwrap();
        db.insert_symbol(
            run_code_file,
            &graph::SymbolNode {
                name: "runCode".to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: run_code_source.len(),
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap();

        let route_file = db
            .insert_file(
                "app/api/run/route.ts",
                &storage::hash_content(route_source.as_bytes()),
            )
            .unwrap();
        db.insert_symbol(
            route_file,
            &graph::SymbolNode {
                name: "POST".to_string(),
                kind: "route".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: route_source.len(),
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap();

        // No static edge connects runCode -> normalizeProblem (they're only
        // related via the fetch/route HTTP boundary), so a plain shortest_path
        // between unrelated names must come back found:false but WITH a
        // suggested connector pointing at the POST route handler.
        let resp = shortest_path(&db, "runCode", "normalizeProblem", None, None).unwrap();
        assert!(!resp.found);
        let connector = resp
            .suggested_connector
            .expect("expected a suggested HTTP-boundary connector");
        assert_eq!(connector.route_symbol, "POST");
        assert!(connector.file_path.contains("app/api/run/route.ts"));
        assert_eq!(connector.matched_literal, "/api/run");

        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(test)]
mod architectural_hotspots_tests {
    use super::*;

    fn sym(db: &Database, file_id: i64, name: &str, start: i64, end: i64) -> i64 {
        db.insert_symbol(
            file_id,
            &graph::SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: start as usize,
                end_line: end as usize,
                start_byte: 0,
                end_byte: 0,
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap()
    }

    // Regression for the file-granularity bug: co-located symbols in a hub
    // file must NOT all inherit the same incoming/outgoing counts. Only `b`
    // is actually called; `c` shares a file with `b` but has zero edges.
    #[test]
    fn colocated_symbols_get_distinct_counts() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("hub.ts", "h").unwrap();
        let a = sym(&db, f, "a", 1, 3);
        let b = sym(&db, f, "b", 5, 7);
        let _c = sym(&db, f, "c", 9, 11);
        db.insert_edge_attributed(f, Some(a), b, "calls").unwrap();

        let resp = architectural_hotspots(&db, 50, None).unwrap();
        let by_name = |n: &str| resp.top_hotspots.iter().find(|h| h.name == n).unwrap();

        assert_eq!(by_name("b").incoming_edges, 1);
        assert_eq!(by_name("a").outgoing_edges, 1);
        assert_eq!(
            by_name("c").incoming_edges + by_name("c").outgoing_edges,
            0,
            "c shares a file with a/b but has no real edges of its own"
        );
    }
}

#[cfg(test)]
mod dependency_cycles_tests {
    use super::*;

    fn sym(db: &Database, file_id: i64, name: &str, start: i64, end: i64) -> i64 {
        db.insert_symbol(
            file_id,
            &graph::SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: start as usize,
                end_line: end as usize,
                start_byte: 0,
                end_byte: 0,
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap()
    }

    // #1 Fixture A — same-file mutual recursion a()<->b() is exactly ONE cycle.
    #[test]
    fn fixture_a_mutual_recursion_is_one_cycle() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("recur.ts", "h").unwrap();
        let a = sym(&db, f, "a", 1, 3);
        let b = sym(&db, f, "b", 5, 7);
        // a calls b, b calls a — attributed to their source symbols.
        db.insert_edge_attributed(f, Some(a), b, "calls").unwrap();
        db.insert_edge_attributed(f, Some(b), a, "calls").unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(resp.cycles_found, 1, "a<->b is exactly one cycle");
        assert_eq!(resp.same_file_cycles_found, 1);
        assert_eq!(resp.cross_file_cycles_found, 0);
        // same_file_examples populated for sanity-checking.
        assert_eq!(resp.same_file_examples.len(), 1);
    }

    // #1 Fixture B (recursive type) — a symbol referencing itself is NOT a cycle.
    #[test]
    fn fixture_b_self_reference_is_not_a_cycle() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("tree.ts", "h").unwrap();
        let t = sym(&db, f, "TreeNode", 1, 3);
        // TreeNode references TreeNode (recursive type) -> self-edge.
        db.insert_edge_attributed(f, Some(t), t, "calls").unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(resp.cycles_found, 0, "self-reference must not be a cycle");
    }

    // #1 Fixture B (parent/child render) — one-directional edge is NOT a cycle.
    #[test]
    fn fixture_b_parent_child_render_is_not_a_cycle() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("ui.tsx", "h").unwrap();
        let parent = sym(&db, f, "Parent", 1, 5);
        let child = sym(&db, f, "Child", 7, 9);
        // Parent renders Child; Child does NOT render Parent.
        db.insert_edge_attributed(f, Some(parent), child, "renders_component")
            .unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(
            resp.cycles_found, 0,
            "one-directional render is not a cycle"
        );
    }

    // #1 Regression — the old cartesian bug: co-located symbols sharing a file
    // must NOT manufacture cycles. With per-symbol attribution, a single a->b
    // edge yields zero cycles even though a, b, c all live in one file.
    #[test]
    fn colocated_symbols_do_not_manufacture_cycles() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("util.ts", "h").unwrap();
        let a = sym(&db, f, "a", 1, 3);
        let b = sym(&db, f, "b", 5, 7);
        let _c = sym(&db, f, "c", 9, 11);
        db.insert_edge_attributed(f, Some(a), b, "calls").unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(resp.cycles_found, 0, "a->b alone is not a cycle");
    }

    // #1 — genuine cross-file cycle still detected and classified.
    #[test]
    fn cross_file_cycle_detected() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f1 = db.insert_file("a.ts", "h").unwrap();
        let f2 = db.insert_file("b.ts", "h").unwrap();
        let a = sym(&db, f1, "a", 1, 3);
        let b = sym(&db, f2, "b", 1, 3);
        db.insert_edge_attributed(f1, Some(a), b, "calls").unwrap();
        db.insert_edge_attributed(f2, Some(b), a, "calls").unwrap();

        let resp = dependency_cycles(&db, 50, None, false).unwrap();
        assert_eq!(resp.cross_file_cycles_found, 1);
        assert_eq!(
            resp.cycles_returned, 1,
            "cross-file cycle returned even with include_same_file=false"
        );
    }

    // #1 — edges with no source attribution (NULL) are ignored, not exploded.
    #[test]
    fn unattributed_edges_are_ignored() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("legacy.ts", "h").unwrap();
        let a = sym(&db, f, "a", 1, 3);
        let b = sym(&db, f, "b", 5, 7);
        // Simulate a pre-migration / top-level-import edge: no source symbol.
        db.insert_edge_attributed(f, None, b, "imports").unwrap();
        db.insert_edge_attributed(f, None, a, "imports").unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(
            resp.cycles_found, 0,
            "NULL-source edges must not form cycles"
        );
    }
}
