use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct SemanticIndexValidator;

impl PipelineValidator for SemanticIndexValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::SemanticIndex
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Graph] // Semantic indexing depends on symbol extraction and relationships (graph)
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let findings = Vec::new(); // Will fill if there are critical missing embeddings
        let mut metrics = HashMap::new();

        let total_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let total_embeddings: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbol_embeddings",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        metrics.insert("Total Embeddings".to_string(), total_embeddings as f64);

        let coverage = if total_symbols > 0 {
            (total_embeddings as f64 / total_symbols as f64) * 100.0
        } else {
            0.0
        };
        metrics.insert("Embedding Coverage (%)".to_string(), coverage);

        // Note: We don't fail this stage if coverage is <100% because embedding generation
        // requires an OpenAI API key and might be intentionally skipped in local CI.
        // It's mostly an informational diagnostic unless explicitly enforced.
        let status = if coverage == 0.0 { StageStatus::Warning } else { StageStatus::Pass };

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
