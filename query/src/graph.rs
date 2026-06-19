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
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

pub fn explore_graph(db: &Database, symbol_name: &str, max_depth: usize, direction: GraphDirection) -> Result<GraphResponse> {
    let safe_depth = std::cmp::min(max_depth, 5);
    let max_nodes = 200;

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

    while let Some((curr_id, current_depth)) = queue.pop_front() {
        if current_depth >= safe_depth {
            continue;
        }

        let is_outgoing = matches!(direction, GraphDirection::Outgoing | GraphDirection::Both);
        let is_incoming = matches!(direction, GraphDirection::Incoming | GraphDirection::Both);

        // Process Outgoing dependencies
        if is_outgoing {
            let mut out_rows = out_stmt.query(rusqlite::params![curr_id])?;
            while let Some(row) = out_rows.next()? {
                if nodes.len() >= max_nodes { break; }
                
                let target_id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;

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

        if nodes.len() >= max_nodes { break; }

        // Process Incoming dependencies
        if is_incoming {
            let mut in_rows = in_stmt.query(rusqlite::params![curr_id])?;
            while let Some(row) = in_rows.next()? {
                if nodes.len() >= max_nodes { break; }
                
                let source_file_id: i64 = row.get(0)?;
                let kind: String = row.get(1)?;

                let mut sym_rows = resolve_file_syms_stmt.query(rusqlite::params![source_file_id])?;
                while let Some(sym_row) = sym_rows.next()? {
                    if nodes.len() >= max_nodes { break; }

                    let source_id: i64 = sym_row.get(0)?;
                    let source_name: String = sym_row.get(1)?;
                    let source_kind: String = sym_row.get(2)?;
                    let source_path: String = sym_row.get(3)?;

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
        nodes,
        edges,
    })
}
