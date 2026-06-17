use storage::Database;

pub struct TokenAccounting;

impl TokenAccounting {
    /// Extremely rough heuristic: 1 token is roughly 4 bytes of English text/code.
    pub fn estimate_tokens(bytes: usize) -> usize {
        bytes / 4
    }

    pub fn estimate_search_context(db: &Database) -> usize {
        let total_symbols: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
        Self::estimate_tokens((total_symbols as usize) * 50)
    }

    pub fn estimate_find_symbol_context(db: &Database, symbol: &str) -> usize {
        let file_path: Result<String, _> = db.conn.query_row(
            "SELECT files.path FROM symbols JOIN files ON symbols.file_id = files.id WHERE symbols.name = ?1 LIMIT 1",
            rusqlite::params![symbol],
            |r| r.get(0)
        );
        match file_path {
            Ok(path) => {
                if let Ok(metadata) = std::fs::metadata(&path) {
                    Self::estimate_tokens(metadata.len() as usize)
                } else {
                    1000 
                }
            }
            Err(_) => 0
        }
    }

    pub fn estimate_graph_context(db: &Database) -> usize {
        let total_symbols: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
        let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
        Self::estimate_tokens((total_symbols as usize * 50) + (total_edges as usize * 100))
    }
}

pub struct CostAccounting;

impl CostAccounting {
    /// Converts token savings into estimated US Cents.
    pub fn calculate_cents_saved(tokens_saved: usize, model: &str) -> f64 {
        let cost_per_million_cents = match model.to_lowercase().as_str() {
            m if m.contains("claude") => 300.0,
            m if m.contains("gpt") => 250.0,
            m if m.contains("gemini") => 150.0,
            m if m.contains("qwen") => 50.0,
            _ => 300.0,
        };
        (tokens_saved as f64 / 1_000_000.0) * cost_per_million_cents
    }
}