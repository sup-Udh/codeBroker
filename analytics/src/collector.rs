use storage::Database;

pub struct MetricsCollector<'a> {
    db: &'a Database,
}

impl<'a> MetricsCollector<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn log_token_savings(&self, symbol_name: &str, raw_tokens_avoided: usize, context_tokens_used: usize, cost_saved_cents: f64) {
        let conn = self.db.get_connection();
        let _ = conn.execute(
            "INSERT INTO token_metrics (symbol_name, raw_tokens_avoided, context_tokens_used, cost_saved_cents) 
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![symbol_name, raw_tokens_avoided as i64, context_tokens_used as i64, cost_saved_cents],
        );
    }

    pub fn log_cache_metric(&self, symbol_name: &str, status: &str, latency_ms: usize) {
        let conn = self.db.get_connection();
        let _ = conn.execute(
            "INSERT INTO cache_metrics (symbol_name, status, latency_ms) VALUES (?1, ?2, ?3)",
            rusqlite::params![symbol_name, status, latency_ms as i64],
        );
    }
}