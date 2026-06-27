use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct ResolverValidator;

impl PipelineValidator for ResolverValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Resolver
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
        metrics.insert("Relationships Entering Resolver".to_string(), total_rel as f64);

        let mut state_stmt = db.conn.prepare("SELECT state, COUNT(*) FROM relationships GROUP BY state")?;
        let state_rows = state_stmt.query_map([], |row| {
            let state: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((state, count))
        })?;

        for row in state_rows {
            if let Ok((state, count)) = row {
                let state_str = state.unwrap_or_else(|| "Unknown".to_string());
                metrics.insert(format!("State: {}", state_str), count as f64);
            }
        }

        let mut ev_stmt = db.conn.prepare("SELECT evidence, COUNT(*) FROM relationships GROUP BY evidence")?;
        let ev_rows = ev_stmt.query_map([], |row| {
            let ev: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((ev, count))
        })?;

        for row in ev_rows {
            if let Ok((ev, count)) = row {
                if let Some(ev_str) = ev {
                    metrics.insert(format!("Evidence: {}", ev_str), count as f64);
                }
            }
        }

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
                likely_cause: "Resolver assigned RepositorySymbol state without supplying the resolution evidence.".to_string(),
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
                severity: Severity::Warning,
                title: "Missing Resolution Reason".to_string(),
                description: format!("Found {} relationships marked as Missing or Dynamic but without a reason logged in evidence.", missing_reason),
                likely_cause: "Pipeline stage returned Missing/Dynamic without specifying why the resolution was halted.".to_string(),
                suggested_fix: "Attach a fallback evidence string explaining why it was marked dynamic or missing.".to_string(),
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
