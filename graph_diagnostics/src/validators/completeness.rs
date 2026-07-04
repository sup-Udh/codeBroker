use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct GraphCompletenessValidator;

impl PipelineValidator for GraphCompletenessValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Completeness
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::SemanticIndex, PipelineStage::Graph]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();
        let mut status = StageStatus::Pass;

        // Verify every RepositorySymbol has required attributes.
        // 1. Total Symbols
        let total_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        metrics.insert("Total Symbols Checked".to_string(), total_symbols as f64);

        if total_symbols == 0 {
            return Ok(StageReport {
                stage: self.stage(),
                status: StageStatus::Pass,
                execution_time_ms: start.elapsed().as_millis(),
                metrics,
                findings,
            });
        }

        // 2. Symbols with parent files
        let symbols_without_file: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if symbols_without_file > 0 {
            status = StageStatus::Fail;
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Symbols Missing Parent Files".to_string(),
                description: format!("{} symbols are missing valid parent files", symbols_without_file),
                likely_cause: "Parser failure during file assignment".to_string(),
                suggested_fix: "Ensure the parser correctly maps file IDs when emitting symbols.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        // 3. Symbols with Source Spans
        let symbols_without_spans: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols WHERE start_byte IS NULL OR end_byte IS NULL",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if symbols_without_spans > 0 {
            status = StageStatus::Fail;
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Symbols Missing Source Spans".to_string(),
                description: format!("{} symbols are missing start_byte or end_byte", symbols_without_spans),
                likely_cause: "Parser AST omission".to_string(),
                suggested_fix: "Parser MUST emit precise byte boundaries for all AST nodes.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        // 4. Edges (Incoming/Outgoing relationships)
        let symbols_with_edges: i64 = db.conn.query_row(
            "SELECT COUNT(DISTINCT source_symbol_id) FROM edges",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let edge_coverage = (symbols_with_edges as f64 / total_symbols as f64) * 100.0;
        metrics.insert("Edge Coverage (%)".to_string(), edge_coverage);

        if edge_coverage < 95.0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Warning,
                title: "Low Edge Coverage".to_string(),
                description: format!("Only {:.1}% of symbols have outgoing edges. Expected >= 95%.", edge_coverage),
                likely_cause: "Missing relationship extraction logic".to_string(),
                suggested_fix: "Check flow engine relationship extraction logic.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
