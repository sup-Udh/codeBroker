use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct ConsistencyValidator;

impl PipelineValidator for ConsistencyValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Consistency
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![
            PipelineStage::Parser,
            PipelineStage::Semantic,
            PipelineStage::Flow,
            PipelineStage::Receiver,
            PipelineStage::Method,
            PipelineStage::Resolver,
            PipelineStage::Graph,
        ]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        // Validate complete data flow path:
        // relationships of kind = 'method_call' -> must have a state. If state == 'RepositorySymbol', 
        // the target must exist in symbols and edges.
        
        let invalid_method_calls: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships 
             WHERE kind = 'method_call' 
               AND state = 'RepositorySymbol'
               AND NOT EXISTS (
                   SELECT 1 FROM edges 
                   WHERE edges.source_symbol_id = (SELECT id FROM symbols s WHERE s.file_id = relationships.file_id AND s.start_line <= relationships.line_number ORDER BY s.start_line DESC LIMIT 1)
                     AND edges.kind = 'method_call'
               )",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if invalid_method_calls > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "End-to-End Pipeline Breakdown".to_string(),
                description: format!("Found {} method_call relationships marked as successfully resolved (RepositorySymbol) but no corresponding edge exists in the graph.", invalid_method_calls),
                likely_cause: "The resolver correctly determined the target, but the graph builder failed to link it to the enclosing symbol scope (perhaps due to missing enclosing symbol).".to_string(),
                suggested_fix: "Check the enclosing_symbol lookup logic in graph builder for these file lines.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let has_errors = findings.iter().any(|f| matches!(f.severity, Severity::Error | Severity::Critical));
        let status = if has_errors { StageStatus::Fail } else if !findings.is_empty() { StageStatus::Warning } else { StageStatus::Pass };

        metrics.insert("Pipeline Breakdown Errors".to_string(), invalid_method_calls as f64);

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
