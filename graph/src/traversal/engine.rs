use crate::node::SemanticNode;
use crate::query::GraphQueryService;
use rusqlite::Result;
use std::collections::HashSet;

pub struct TraversalEngine<'a> {
    pub query_service: &'a GraphQueryService<'a>,
}

impl<'a> TraversalEngine<'a> {
    pub fn new(query_service: &'a GraphQueryService<'a>) -> Self {
        Self { query_service }
    }

    /// Recursively resolve "data model" dependencies up to a depth limit,
    /// abstracting away the complex 'fetch_data_model_dependencies' SQL from the old query layer.
    pub fn fetch_data_model_dependencies(&self, symbol_name: &str, file_id: i64, signature: Option<&str>) -> Vec<String> {
        let mut deps = Vec::new();
        let mut processed_words = HashSet::new();

        if let Some(sig) = signature {
            let words: Vec<&str> = sig.split(|c: char| !c.is_alphabetic()).collect();
            for word in words {
                if deps.len() >= 8 { break; } // MAX_DEPENDENCY_EXPANSIONS
                if word.is_empty() || word.chars().next().unwrap().is_lowercase() || word == "Depends" || word == "Session" || word == symbol_name {
                    continue;
                }
                if !processed_words.insert(word) { continue; }
                
                // Real DB lookup for this word...
                let mut check_stmt = self.query_service.conn.prepare("SELECT name, file_id FROM symbols WHERE name = ?1 LIMIT 1").unwrap();
                if let Ok((_name, found_file_id)) = check_stmt.query_row(rusqlite::params![word], |row| {
                    Ok((row.get::<_, String>(0).unwrap(), row.get::<_, i64>(1).unwrap()))
                }) {
                    deps.push(word.to_string());
                    
                    // Inherits edges
                    let mut inherits_stmt = self.query_service.conn.prepare(
                        "SELECT symbols.name FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1 AND edges.kind = 'inherits'"
                    ).unwrap();
                    let mut inherits_rows = inherits_stmt.query(rusqlite::params![found_file_id]).unwrap();
                    while let Some(row) = inherits_rows.next().unwrap_or(None) {
                        deps.push(row.get(0).unwrap());
                    }
                }
            }
        }

        let mut direct_edges_stmt = self.query_service.conn.prepare(
            "SELECT symbols.name FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1 AND (edges.kind = 'inherits' OR edges.kind = 'accepts_props')"
        ).unwrap();
        let mut direct_rows = direct_edges_stmt.query(rusqlite::params![file_id]).unwrap();
        while let Some(row) = direct_rows.next().unwrap_or(None) {
            deps.push(row.get(0).unwrap());
        }

        deps
    }
}
