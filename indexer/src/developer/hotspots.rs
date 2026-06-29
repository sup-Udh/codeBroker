use crate::developer::manifest::Hotspots;
use crate::resolver::index::SymbolIndex;
use storage::Database;
use std::collections::{HashMap, HashSet};

pub struct HotspotDetector;

impl HotspotDetector {
    pub fn detect(db: &Database, symbol_index: &SymbolIndex) -> Result<Hotspots, String> {
        let mut highest_pagerank_symbol = None;
        let mut highest_fan_in_symbol = None;
        let mut highest_fan_out_symbol = None;
        let mut most_connected_symbol = None;
        
        let mut max_pagerank = 0.0;
        let mut max_fan_in = 0;
        let mut max_fan_out = 0;
        let mut max_connections = 0;

        if let Ok(mut stmt) = db.conn.prepare("SELECT symbol_id, pagerank, fan_in, fan_out FROM symbol_features") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?
                ))
            }) {
                for row in rows.flatten() {
                    let (sym_id, pagerank, fan_in, fan_out) = row;
                    if pagerank > max_pagerank {
                        max_pagerank = pagerank;
                        highest_pagerank_symbol = Some(sym_id);
                    }
                    if fan_in > max_fan_in {
                        max_fan_in = fan_in;
                        highest_fan_in_symbol = Some(sym_id);
                    }
                    if fan_out > max_fan_out {
                        max_fan_out = fan_out;
                        highest_fan_out_symbol = Some(sym_id);
                    }
                    let connections = fan_in + fan_out;
                    if connections > max_connections {
                        max_connections = connections;
                        most_connected_symbol = Some(sym_id);
                    }
                }
            }
        }
        
        let mut file_imports: HashMap<i64, i32> = HashMap::new();
        if let Ok(relationships) = db.get_all_relationships_with_lines() {
            for (_, source_file_id, _, _, kind, _) in relationships {
                if kind.as_deref() == Some("imports") {
                    // We really want the target file id for most imported file, but we only have source_file_id + import source string.
                    // For now, this is a proxy for "most importing file", or we can look at edges.
                }
            }
        }

        let mut most_imported_file = None;
        let mut target_file_imports = HashMap::new();
        if let Ok(mut stmt) = db.conn.prepare("SELECT source_file_id, target_symbol_id FROM edges") {
            if let Ok(rows) = stmt.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
            }) {
                for row in rows.flatten() {
                    let (_, target_sym_id) = row;
                    if let Some(sym) = symbol_index.get_symbol(target_sym_id) {
                        *target_file_imports.entry(sym.file_id).or_insert(0) += 1;
                    }
                }
            }
        }
        
        let mut max_imports = 0;
        for (file_id, count) in target_file_imports {
            if count > max_imports {
                max_imports = count;
                most_imported_file = Some(file_id);
            }
        }

        Ok(Hotspots {
            highest_pagerank_symbol,
            highest_fan_in_symbol,
            highest_fan_out_symbol,
            most_imported_file,
            most_central_module: None,
            largest_dependency_cluster: None,
            most_connected_symbol,
            architectural_bottleneck: None,
        })
    }
}
