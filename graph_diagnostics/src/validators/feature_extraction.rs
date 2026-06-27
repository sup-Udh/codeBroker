use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct FeatureExtractionValidator;

impl PipelineValidator for FeatureExtractionValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::FeatureExtraction
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Graph]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        let total_features: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbol_features",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Extracted Features".to_string(), total_features as f64);

        let total_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let coverage = if total_symbols > 0 {
            (total_features as f64 / total_symbols as f64) * 100.0
        } else {
            0.0
        };
        metrics.insert("Feature Coverage (%)".to_string(), coverage);

        let entrypoints: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbol_features WHERE is_entrypoint = 1",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Entrypoints Detected".to_string(), entrypoints as f64);

        if coverage < 100.0 && total_symbols > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Warning,
                title: "Incomplete Feature Coverage".to_string(),
                description: format!("Only {:.1}% of symbols have extracted graph features (pagerank, community).", coverage),
                likely_cause: "Feature extraction failed or skipped some symbols, potentially due to isolation.".to_string(),
                suggested_fix: "Investigate graph extraction logic to ensure every symbol receives a feature row.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let has_errors = findings.iter().any(|f| matches!(f.severity, Severity::Error | Severity::Critical));
        let status = if has_errors { StageStatus::Fail } else if !findings.is_empty() { StageStatus::Warning } else { StageStatus::Pass };

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
