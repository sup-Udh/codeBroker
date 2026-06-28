use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct PipelineHealthValidator;

impl PipelineValidator for PipelineHealthValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::PipelineHealth
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Receiver, PipelineStage::Method]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        let total_rel: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Relationships Analyzed".to_string(), total_rel as f64);

        // Validate invariants
        let invalid_repo_sym: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE state = 'RepositorySymbol' AND evidence IS NULL",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        
        if invalid_repo_sym > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Invalid RepositorySymbol State".to_string(),
                description: format!("Found {} relationships marked as RepositorySymbol but missing evidence.", invalid_repo_sym),
                likely_cause: "Pipeline Contract Violation".to_string(),
                suggested_fix: "Ensure all successful resolutions return a populated evidence context.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let missing_reason: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE state IN ('Missing', 'Dynamic') AND evidence IS NULL",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if missing_reason > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Missing Resolution Reason".to_string(),
                description: format!("Found {} relationships marked as Missing or Dynamic but without a reason logged in evidence.", missing_reason),
                likely_cause: "Pipeline Contract Violation".to_string(),
                suggested_fix: "Attach a fallback evidence string explaining why it was marked dynamic or missing.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }
        
        let invalid_builtin: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE state = 'Builtin' AND evidence != 'BuiltinClassification'",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        
        if invalid_builtin > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Invalid Classification Evidence".to_string(),
                description: format!("Found {} relationships marked as Builtin without BuiltinClassification evidence.", invalid_builtin),
                likely_cause: "Pipeline Contract Violation".to_string(),
                suggested_fix: "Ensure Builtin only comes from BuiltinClassification.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let invalid_ext: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE state = 'ExternalDependency' AND evidence != 'ExternalDependency'",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        
        if invalid_ext > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Invalid Classification Evidence".to_string(),
                description: format!("Found {} relationships marked as ExternalDependency without ExternalDependency evidence.", invalid_ext),
                likely_cause: "Pipeline Contract Violation".to_string(),
                suggested_fix: "Ensure ExternalDependency only comes from ExternalDependency evidence.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let has_critical = findings.iter().any(|f| matches!(f.severity, Severity::Critical));
        let status = if has_critical { StageStatus::Fail } else if !findings.is_empty() { StageStatus::Warning } else { StageStatus::Pass };

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
