use std::collections::HashMap;
use storage::Database;

#[derive(Debug, Clone)]
pub struct IndexSymbol {
    pub id: i64,
    pub file_id: i64,
    pub name: String,
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

pub struct SymbolIndex {
    pub symbols: HashMap<i64, IndexSymbol>,
    pub symbols_by_name: HashMap<String, Vec<i64>>,
    pub symbols_by_file: HashMap<i64, Vec<i64>>,
    pub parent_map: HashMap<i64, i64>,
    /// Maps file_id → file path (e.g. "./src/main.rs") for language detection.
    pub file_paths: HashMap<i64, String>,
    /// Maps parent symbol id → child symbol ids for method resolution.
    pub methods_by_parent: HashMap<i64, Vec<i64>>,
}

impl SymbolIndex {
    pub fn build(db: &Database) -> Result<Self, String> {
        let mut symbols = HashMap::new();
        let mut symbols_by_name: HashMap<String, Vec<i64>> = HashMap::new();
        let mut symbols_by_file: HashMap<i64, Vec<i64>> = HashMap::new();
        let mut parent_map = HashMap::new();

        let mut stmt = db.conn.prepare(
            "SELECT id, file_id, name, kind, start_byte, end_byte FROM symbols"
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([], |row| {
            Ok(IndexSymbol {
                id: row.get(0)?,
                file_id: row.get(1)?,
                name: row.get(2)?,
                kind: row.get(3)?,
                start_byte: row.get::<_, i64>(4)? as usize,
                end_byte: row.get::<_, i64>(5)? as usize,
            })
        }).map_err(|e| e.to_string())?;

        for row in rows {
            if let Ok(sym) = row {
                symbols_by_name.entry(sym.name.clone()).or_default().push(sym.id);
                symbols_by_file.entry(sym.file_id).or_default().push(sym.id);
                symbols.insert(sym.id, sym);
            }
        }

        // Build parent_map: for each symbol, find the tightest enclosing symbol in the same file
        for file_syms in symbols_by_file.values() {
            for &child_id in file_syms {
                let child = &symbols[&child_id];
                let mut best_parent: Option<i64> = None;
                let mut best_size = usize::MAX;

                for &parent_id in file_syms {
                    if child_id == parent_id { continue; }
                    let parent = &symbols[&parent_id];
                    if parent.start_byte <= child.start_byte && parent.end_byte >= child.end_byte {
                        let size = parent.end_byte - parent.start_byte;
                        if size < best_size {
                            best_size = size;
                            best_parent = Some(parent_id);
                        }
                    }
                }

                if let Some(parent) = best_parent {
                    parent_map.insert(child_id, parent);
                }
            }
        }

        // Build methods_by_parent from parent_map
        let mut methods_by_parent: HashMap<i64, Vec<i64>> = HashMap::new();
        for (&child_id, &parent_id) in &parent_map {
            methods_by_parent.entry(parent_id).or_default().push(child_id);
        }

        // Load file paths for language detection
        let mut file_paths: HashMap<i64, String> = HashMap::new();
        if let Ok(mut stmt2) = db.conn.prepare("SELECT id, path FROM files") {
            let _ = stmt2.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            }).map(|rows| {
                for row in rows.flatten() {
                    file_paths.insert(row.0, row.1);
                }
            });
        }

        Ok(Self {
            symbols,
            symbols_by_name,
            symbols_by_file,
            parent_map,
            file_paths,
            methods_by_parent,
        })
    }

    pub fn get_symbol(&self, id: i64) -> Option<&IndexSymbol> {
        self.symbols.get(&id)
    }

    pub fn find_by_name(&self, name: &str) -> Option<&Vec<i64>> {
        self.symbols_by_name.get(name)
    }

    /// Find all symbols named `method_name` that are direct children of any symbol
    /// named `type_name`. Used for receiver-based method resolution.
    pub fn find_method_in_type(&self, method_name: &str, type_name: &str) -> Vec<i64> {
        let Some(method_ids) = self.find_by_name(method_name) else {
            return Vec::new();
        };
        method_ids.iter().copied().filter(|&mid| {
            self.parent_map.get(&mid)
                .and_then(|&pid| self.get_symbol(pid))
                .map(|parent| parent.name == type_name)
                .unwrap_or(false)
        }).collect()
    }
}
