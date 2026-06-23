use serde::{Serialize, Deserialize};
use storage::Database;
use std::collections::{HashSet, VecDeque};
use rusqlite::{Result, OptionalExtension};

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphResponse {
    pub root: String,
    pub depth: usize,
    pub truncated: bool,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn explore_graph(db: &Database, symbol_name: &str, max_depth: usize, direction: GraphDirection, max_nodes: usize) -> Result<GraphResponse> {
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
         WHERE symbols.name = ?1 LIMIT 1"
    )?;
    
    let root_info: Option<(i64, String, String, String)> = stmt.query_row(rusqlite::params![symbol_name], |row| {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
        ))
    }).optional()?;

    let root_info = match root_info {
        Some(info) => info,
        None => {
            return Ok(GraphResponse {
                root: symbol_name.to_string(),
                depth: safe_depth,
                truncated: false,
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

    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind 
         FROM edges 
         WHERE source_file_id = (SELECT file_id FROM symbols WHERE id = ?1 LIMIT 1)"
    )?;

    let mut in_stmt = db.conn.prepare(
        "SELECT source_file_id, kind 
         FROM edges 
         WHERE target_symbol_id = ?1"
    )?;

    let mut resolve_sym_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.id = ?1 LIMIT 1"
    )?;

    let mut resolve_file_syms_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.file_id = ?1"
    )?;

    let mut truncated = false;

    while let Some((curr_id, current_depth)) = queue.pop_front() {
        if truncated {
            break;
        }
        if current_depth >= safe_depth {
            continue;
        }

        let is_outgoing = matches!(direction, GraphDirection::Outgoing | GraphDirection::Both | GraphDirection::Undirected);
        let is_incoming = matches!(direction, GraphDirection::Incoming | GraphDirection::Both | GraphDirection::Undirected);

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
                    });
                }

                if !visited_symbols.contains(&target_id) {
                    visited_symbols.insert(target_id);
                    queue.push_back((target_id, current_depth + 1));
                    
                    if let Ok((id, name, kind, path)) = resolve_sym_stmt.query_row(rusqlite::params![target_id], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
                    }) {
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

                let source_file_id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;

                let mut sym_rows = resolve_file_syms_stmt.query(rusqlite::params![source_file_id])?;
                while let Some(sym_row) = sym_rows.next()? {
                    if nodes.len() >= max_nodes || edges.len() >= max_edges {
                        truncated = true;
                        break;
                    }

                    let source_id: i64 = sym_row.get(0)?;
                    let source_name: String = sym_row.get(1)?;
                    let source_kind: String = sym_row.get(2)?;
                    let source_path: String = sym_row.get(3)?;

                    // Skip self-loops: the resolved source file also contains
                    // curr_id itself, which would otherwise emit a node -> node
                    // edge (e.g. DELETE -> DELETE).
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
                        });
                    }

                    if !visited_symbols.contains(&source_id) {
                        visited_symbols.insert(source_id);
                        queue.push_back((source_id, current_depth + 1));
                        
                        nodes.push(GraphNode {
                            id: source_id.to_string(),
                            name: source_name,
                            kind: source_kind,
                            file_path: source_path,
                        });
                    }
                }
            }
        }
    }

    Ok(GraphResponse {
        root: symbol_name.to_string(),
        depth: safe_depth,
        truncated,
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
}

pub fn shortest_path(db: &Database, from_symbol: &str, to_symbol: &str) -> Result<ShortestPathResponse> {
    let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
    let graph_unindexed = total_edges == 0;

    // 1. Find root and target symbol IDs
    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 1"
    )?;
    
    let from_info: Option<(i64, String, String, String)> = stmt.query_row(rusqlite::params![from_symbol], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    }).optional()?;

    let to_info: Option<(i64, String, String, String)> = stmt.query_row(rusqlite::params![to_symbol], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    }).optional()?;

    if from_info.is_none() || to_info.is_none() {
        return Ok(ShortestPathResponse {
            from: from_symbol.to_string(),
            to: to_symbol.to_string(),
            found: false,
            distance: 0,
            nodes: vec![],
            edges: vec![],
            graph_unindexed,
        });
    }

    let from_info = from_info.unwrap();
    let to_info = to_info.unwrap();

    let from_id = from_info.0;
    let to_id = to_info.0;

    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    
    // Maps target_id -> (source_id, edge_kind)
    let mut parent_map: std::collections::HashMap<i64, (i64, String)> = std::collections::HashMap::new();

    queue.push_back(from_id);
    visited.insert(from_id);

    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind 
         FROM edges 
         WHERE source_file_id = (SELECT file_id FROM symbols WHERE id = ?1 LIMIT 1)"
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

            if !visited.contains(&target_id) {
                visited.insert(target_id);
                parent_map.insert(target_id, (curr_id, kind));
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
        return Ok(ShortestPathResponse {
            from: from_symbol.to_string(),
            to: to_symbol.to_string(),
            found: false,
            distance: 0,
            nodes: vec![],
            edges: vec![],
            graph_unindexed,
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
        if let Some(&(parent_id, ref kind)) = parent_map.get(&current) {
            edges_backward.push((parent_id, current, kind.clone()));
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
         WHERE symbols.id = ?1 LIMIT 1"
    )?;

    for id in path_ids {
        if let Ok((name, kind, path)) = resolve_sym_stmt.query_row(rusqlite::params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
        }) {
            path_nodes.push(PathNode {
                symbol_name: name,
                symbol_kind: kind,
                file_path: path,
            });
        }
    }

    for (src_id, tgt_id, kind) in edges_backward {
        // Let's just use IDs as strings or resolve them
        let src_name_str = resolve_sym_stmt.query_row(rusqlite::params![src_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| src_id.to_string());
        let tgt_name_str = resolve_sym_stmt.query_row(rusqlite::params![tgt_id], |r| r.get::<_, String>(0)).unwrap_or_else(|_| tgt_id.to_string());

        path_edges.push(PathEdge {
            source: src_name_str,
            target: tgt_name_str,
            edge_kind: kind,
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
pub struct HotspotResponse {
    pub total_symbols: usize,
    pub total_edges: usize,
    pub top_hotspots: Vec<HotspotNode>,
}

pub fn architectural_hotspots(db: &Database, limit: usize, path_scope: Option<&str>) -> Result<HotspotResponse> {
    let mut incoming_map = std::collections::HashMap::new();
    let mut in_stmt = db.conn.prepare(
        "SELECT edges.target_symbol_id, COUNT(symbols.id)
         FROM edges
         JOIN symbols ON edges.source_file_id = symbols.file_id
         GROUP BY edges.target_symbol_id"
    )?;

    let mut in_rows = in_stmt.query([])?;
    while let Some(row) = in_rows.next()? {
        let sym_id: i64 = row.get(0)?;
        let count: i64 = row.get(1)?;
        incoming_map.insert(sym_id, count);
    }

    let mut outgoing_map = std::collections::HashMap::new();
    let mut out_stmt = db.conn.prepare(
        "SELECT symbols.id, COUNT(edges.id)
         FROM symbols
         JOIN edges ON symbols.file_id = edges.source_file_id
         GROUP BY symbols.id"
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
         JOIN files ON symbols.file_id = files.id"
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
        b.hotspot_score.cmp(&a.hotspot_score)
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
        db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0)
    };

    nodes.truncate(limit);

    Ok(HotspotResponse {
        total_symbols,
        total_edges: total_edges as usize,
        top_hotspots: nodes,
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

pub fn dependency_cycles(db: &Database, limit: usize, path_scope: Option<&str>, include_same_file: bool) -> Result<DependencyCyclesResponse> {
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
         WHERE edges.source_symbol_id IS NOT NULL"
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
                        
                        let min_pos = cycle_slice.iter()
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
        if truncated { break; }
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
         WHERE symbols.id = ?1 LIMIT 1"
    )?;

    let cycles_found = unique_cycles.len();
    let mut cross_file_cycles: Vec<DependencyCycle> = Vec::new();
    let mut same_file_cycles: Vec<DependencyCycle> = Vec::new();
    for cycle_ids in unique_cycles.into_iter() {
        let mut cycle_nodes = Vec::new();
        for &id in cycle_ids.iter().take(MAX_NODES_PER_CYCLE) {
            if let Ok((name, kind, path)) = sym_stmt.query_row(rusqlite::params![id], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?))
            }) {
                cycle_nodes.push(CycleNode {
                    name,
                    kind,
                    file_path: path,
                });
            }
        }
        let distinct_files: HashSet<&str> = cycle_nodes.iter().map(|n| n.file_path.as_str()).collect();
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
    let same_file_examples: Vec<DependencyCycle> = same_file_cycles.iter().take(3).cloned().collect();

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
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GraphSubtreeResponse {
    pub root: String,
    pub depth: usize,
    pub node_count: usize,
    pub edge_count: usize,
    pub nodes: Vec<SubtreeNode>,
    pub edges: Vec<SubtreeEdge>,
}

pub fn graph_subtree(db: &Database, root_symbol: &str, depth: usize, max_nodes: Option<usize>) -> Result<GraphSubtreeResponse> {
    let safe_depth = std::cmp::min(depth, 5);
    // Previously a fixed 500 with no escape hatch on hub symbols. Defaulting
    // to 100 matches explore_graph's default, while still allowing a caller
    // to raise it (capped at 500) when they actually want the wide view.
    let max_nodes = max_nodes.unwrap_or(100).clamp(1, 500);

    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    let mut stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 1"
    )?;
    
    let root_info: Option<(i64, String, String, String)> = stmt.query_row(rusqlite::params![root_symbol], |row| {
        Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
    }).optional()?;

    let root_info = match root_info {
        Some(info) => info,
        None => {
            return Ok(GraphSubtreeResponse {
                root: root_symbol.to_string(),
                depth: safe_depth,
                node_count: 0,
                edge_count: 0,
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

    let mut out_stmt = db.conn.prepare(
        "SELECT target_symbol_id, kind 
         FROM edges 
         WHERE source_file_id = (SELECT file_id FROM symbols WHERE id = ?1 LIMIT 1)"
    )?;

    let mut in_stmt = db.conn.prepare(
        "SELECT source_file_id, kind 
         FROM edges 
         WHERE target_symbol_id = ?1"
    )?;

    let mut resolve_sym_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.id = ?1 LIMIT 1"
    )?;

    let mut resolve_file_syms_stmt = db.conn.prepare(
        "SELECT symbols.id, symbols.name, symbols.kind, files.path 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.file_id = ?1"
    )?;

    let mut actual_depth_reached = 0;

    while let Some((curr_id, current_depth)) = queue.pop_front() {
        if current_depth > actual_depth_reached {
            actual_depth_reached = current_depth;
        }

        if current_depth >= safe_depth {
            continue;
        }

        // Outgoing
        let mut out_rows = out_stmt.query(rusqlite::params![curr_id])?;
        while let Some(row) = out_rows.next()? {
            let target_id: i64 = row.get(0)?;
            let kind: String = row.get(1)?;

            let edge_key = (curr_id, target_id, kind.clone());
            if !visited_edges.contains(&edge_key) {
                visited_edges.insert(edge_key);
                
                let src_name_str = resolve_sym_stmt.query_row(rusqlite::params![curr_id], |r| r.get::<_, String>(1)).unwrap_or_else(|_| curr_id.to_string());
                let tgt_name_str = resolve_sym_stmt.query_row(rusqlite::params![target_id], |r| r.get::<_, String>(1)).unwrap_or_else(|_| target_id.to_string());
                
                edges.push(SubtreeEdge {
                    source: src_name_str,
                    target: tgt_name_str,
                    edge_kind: kind,
                });
            }

            if !visited_symbols.contains(&target_id) && nodes.len() < max_nodes {
                visited_symbols.insert(target_id);
                queue.push_back((target_id, current_depth + 1));
                
                if let Ok((_id, name, kind, path)) = resolve_sym_stmt.query_row(rusqlite::params![target_id], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?))
                }) {
                    nodes.push(SubtreeNode {
                        name,
                        kind,
                        file_path: path,
                        depth: current_depth + 1,
                    });
                }
            }
        }

        // Incoming
        let mut in_rows = in_stmt.query(rusqlite::params![curr_id])?;
        while let Some(row) = in_rows.next()? {
            let source_file_id: i64 = row.get(0)?;
            let kind: String = row.get(1)?;

            let mut sym_rows = resolve_file_syms_stmt.query(rusqlite::params![source_file_id])?;
            while let Some(sym_row) = sym_rows.next()? {
                let source_id: i64 = sym_row.get(0)?;
                let source_name: String = sym_row.get(1)?;
                let source_kind: String = sym_row.get(2)?;
                let source_path: String = sym_row.get(3)?;

                let edge_key = (source_id, curr_id, kind.clone());
                if !visited_edges.contains(&edge_key) {
                    visited_edges.insert(edge_key);
                    
                    let tgt_name_str = resolve_sym_stmt.query_row(rusqlite::params![curr_id], |r| r.get::<_, String>(1)).unwrap_or_else(|_| curr_id.to_string());
                    
                    edges.push(SubtreeEdge {
                        source: source_name.clone(),
                        target: tgt_name_str,
                        edge_kind: kind.clone(),
                    });
                }

                if !visited_symbols.contains(&source_id) && nodes.len() < max_nodes {
                    visited_symbols.insert(source_id);
                    queue.push_back((source_id, current_depth + 1));
                    
                    nodes.push(SubtreeNode {
                        name: source_name,
                        kind: source_kind,
                        file_path: source_path,
                        depth: current_depth + 1,
                    });
                }
            }
        }
    }

    Ok(GraphSubtreeResponse {
        root: root_symbol.to_string(),
        depth: actual_depth_reached,
        node_count: nodes.len(),
        edge_count: edges.len(),
        nodes,
        edges,
    })
}

#[cfg(test)]
mod dependency_cycles_tests {
    use super::*;

    fn sym(db: &Database, file_id: i64, name: &str, start: i64, end: i64) -> i64 {
        db.insert_symbol(file_id, &graph::SymbolNode {
            name: name.to_string(),
            kind: "function".to_string(),
            prop_type: None,
            start_line: start as usize,
            end_line: end as usize,
            start_byte: 0,
            end_byte: 0,
            signature: None,
        }).unwrap()
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
        db.insert_edge_attributed(f, Some(parent), child, "renders_component").unwrap();

        let resp = dependency_cycles(&db, 50, None, true).unwrap();
        assert_eq!(resp.cycles_found, 0, "one-directional render is not a cycle");
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
        assert_eq!(resp.cycles_returned, 1, "cross-file cycle returned even with include_same_file=false");
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
        assert_eq!(resp.cycles_found, 0, "NULL-source edges must not form cycles");
    }
}
