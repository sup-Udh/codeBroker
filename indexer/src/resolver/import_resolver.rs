use std::collections::HashMap;
use storage::Database;
use crate::resolver::index::SymbolIndex;

#[derive(Debug, Clone)]
pub struct ImportedSymbol {
    pub name: String,
    pub source: String,
    pub symbol_id: Option<i64>,
}

/// ImportResolver subsystem for resolving cross-boundary symbol sharing.
/// Handles TS path aliases, crates, super::, relative imports, barrel files, etc.
pub struct ImportResolver {
    // Map of (file_id, imported_alias) -> target_symbol_id
    pub resolved_imports: HashMap<(i64, String), ImportedSymbol>,
}

impl ImportResolver {
    pub fn new() -> Self {
        Self {
            resolved_imports: HashMap::new(),
        }
    }
    
    pub fn build(db: &Database, index: &SymbolIndex) -> Result<Self, String> {
        let mut resolved_imports = HashMap::new();
        
        let stmt = "SELECT file_id, name, source FROM relationships WHERE kind = 'imports'";
        if let Ok(mut stmt) = db.conn.prepare(stmt) {
            if let Ok(rows) = stmt.query_map([], |row| {
                let file_id: i64 = row.get(0)?;
                let name: String = row.get(1)?;
                let source: Option<String> = row.get(2)?;
                Ok((file_id, name, source.unwrap_or_default()))
            }) {
                for row in rows.flatten() {
                    let (file_id, name, source) = row;
                    
                    let symbol_id = if source.starts_with(".") || source.starts_with("/") || source.starts_with("~") || source.starts_with("crate::") || source.starts_with("super::") || source.starts_with("self::") {
                        index.find_by_name(&name).and_then(|ids| ids.first().copied())
                    } else {
                        None
                    };

                    resolved_imports.insert((file_id, name.clone()), ImportedSymbol {
                        name,
                        source,
                        symbol_id,
                    });
                }
            }
        }
        
        Ok(Self { resolved_imports })
    }
    
    pub fn resolve(&self, file_id: i64, alias: &str) -> Option<ImportedSymbol> {
        self.resolved_imports.get(&(file_id, alias.to_string())).cloned()
    }
}
