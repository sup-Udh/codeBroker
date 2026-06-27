use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus};
use std::time::Instant;

pub struct RetrievalValidator;

impl PipelineValidator for RetrievalValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Retrieval
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::FeatureExtraction, PipelineStage::SemanticIndex]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let findings = Vec::new();
        let mut metrics = HashMap::new();

        let entrypoints: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbol_features WHERE is_entrypoint = 1",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let total_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let entrypoint_rate = if total_symbols > 0 {
            (entrypoints as f64 / total_symbols as f64) * 100.0
        } else {
            0.0
        };
        
        metrics.insert("Entrypoint Discovery Rate (%)".to_string(), entrypoint_rate);

        let status = StageStatus::Pass;

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
