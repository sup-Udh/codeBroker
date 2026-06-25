use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use rusqlite::Result;
use storage::Database;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ResponseProfile {
    Compact,
    #[default]
    Standard,
    Verbose,
}

impl From<&str> for ResponseProfile {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "compact" => ResponseProfile::Compact,
            "verbose" => ResponseProfile::Verbose,
            _ => ResponseProfile::Standard,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TokenBudget {
    pub max_items: usize,
    pub truncate_strings: bool,
    pub current_items: usize,
}

impl Default for TokenBudget {
    fn default() -> Self {
        Self {
            max_items: 100, // soft limit for total items
            truncate_strings: false,
            current_items: 0,
        }
    }
}

impl TokenBudget {
    pub fn new(profile: ResponseProfile) -> Self {
        match profile {
            ResponseProfile::Compact => Self { max_items: 10, truncate_strings: true, current_items: 0 },
            ResponseProfile::Standard => Self { max_items: 50, truncate_strings: false, current_items: 0 },
            ResponseProfile::Verbose => Self { max_items: 500, truncate_strings: false, current_items: 0 },
        }
    }

    pub fn consume(&mut self, cost: usize) -> bool {
        if self.current_items + cost > self.max_items {
            false
        } else {
            self.current_items += cost;
            true
        }
    }

    pub fn has_budget(&self) -> bool {
        self.current_items < self.max_items
    }
}

#[derive(Debug, Clone)]
pub struct CachedSymbol {
    pub id: i64,
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub file_id: i64,
    pub signature: Option<String>,
    pub metadata: Option<String>,
    pub content_hash: Option<String>,
    pub start_line: i64,
    pub end_line: i64,
}

pub struct SymbolCache {
    by_id: HashMap<i64, CachedSymbol>,
}

impl SymbolCache {
    pub fn new() -> Self {
        Self {
            by_id: HashMap::new(),
        }
    }

    pub fn get_by_id(&mut self, db: &Database, id: i64) -> Result<Option<CachedSymbol>> {
        if let Some(sym) = self.by_id.get(&id) {
            return Ok(Some(sym.clone()));
        }

        let mut stmt = db.conn.prepare(
            "SELECT symbols.name, symbols.kind, files.path, symbols.file_id, symbols.signature, files.metadata, files.content_hash, symbols.start_line, symbols.end_line
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.id = ?1 LIMIT 1"
        )?;

        let sym = stmt.query_row(rusqlite::params![id], |row| {
            Ok(CachedSymbol {
                id,
                name: row.get(0)?,
                kind: row.get(1)?,
                file_path: row.get(2)?,
                file_id: row.get(3)?,
                signature: row.get(4)?,
                metadata: row.get(5)?,
                content_hash: row.get(6)?,
                start_line: row.get(7)?,
                end_line: row.get(8).unwrap_or(row.get::<_, i64>(7)?),
            })
        });

        match sym {
            Ok(s) => {
                let s_clone = s.clone();
                self.by_id.insert(id, s);
                Ok(Some(s_clone))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
