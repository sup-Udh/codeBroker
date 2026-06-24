use storage::Database;

pub struct MetricsCollector<'a> {
    db: &'a Database,
}

impl<'a> MetricsCollector<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }

    pub fn log_comprehensive_event(
        &self,
        tool_name: &str,
        execution_time_ms: usize,
        delivered_token_count: usize,
        estimated_raw_context_tokens: usize,
        cache_hit: bool,
        model_used: &str,
    ) {
        let token_reduction = if estimated_raw_context_tokens > delivered_token_count {
            estimated_raw_context_tokens - delivered_token_count
        } else {
            0
        };

        let _ = self.db.conn.execute(
            "INSERT INTO mcp_analytics_events 
             (tool_name, execution_time_ms, delivered_token_count, estimated_raw_context_tokens, token_reduction, cache_hit, model_used) 
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                tool_name,
                execution_time_ms as i64,
                delivered_token_count as i64,
                estimated_raw_context_tokens as i64,
                token_reduction as i64,
                cache_hit,
                model_used
            ],
        );
    }
}
