use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct SemanticValidator;

impl PipelineValidator for SemanticValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Semantic
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Parser]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        // 1. Semantic Binding counts by kind
        let mut counts_stmt = db.conn.prepare("SELECT kind, COUNT(*) FROM semantic_bindings GROUP BY kind")?;
        let counts_rows = counts_stmt.query_map([], |row| {
            let kind: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((kind, count))
        })?;

        let mut total_bindings = 0;
        for row in counts_rows {
            if let Ok((kind, count)) = row {
                metrics.insert(format!("{} Count", kind), count as f64);
                total_bindings += count;
            }
        }
        metrics.insert("Total Semantic Bindings".to_string(), total_bindings as f64);

        // 2. Orphaned semantic bindings (invalid file_id)
        let orphaned_bindings: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM semantic_bindings WHERE file_id NOT IN (SELECT id FROM files)",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        
        if orphaned_bindings > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Error,
                title: "Orphaned Semantic Bindings".to_string(),
                description: format!("Found {} semantic bindings that point to non-existent files.", orphaned_bindings),
                likely_cause: "Foreign key constraints are missing or files were deleted without cascading to semantic_bindings.".to_string(),
                suggested_fix: "Enable PRAGMA foreign_keys = ON; or add ON DELETE CASCADE to semantic_bindings.".to_string(),
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
