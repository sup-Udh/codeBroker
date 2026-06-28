use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus};
use std::time::Instant;

pub struct ReceiverValidator;

impl PipelineValidator for ReceiverValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Receiver
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Flow]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let findings = Vec::new();
        let mut metrics = HashMap::new();

        // Total method calls
        let total_method_calls: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE kind = 'method_call'",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Total Receiver Expressions".to_string(), total_method_calls as f64);

        // Group by state instead of evidence to get the correct success categories
        let mut state_stmt = db.conn.prepare(
            "SELECT state, COUNT(*) FROM relationships WHERE kind = 'method_call' GROUP BY state"
        )?;

        let mut repo_resolved = 0;
        let mut ext_resolved = 0;
        let mut builtin_resolved = 0;

        let state_rows = state_stmt.query_map([], |row| {
            let state: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((state, count))
        })?;

        for row in state_rows {
            if let Ok((state, count)) = row {
                let state_str = state.unwrap_or_else(|| "Unknown".to_string());
                if state_str == "RepositorySymbol" {
                    repo_resolved += count;
                } else if state_str == "ExternalDependency" {
                    ext_resolved += count;
                } else if state_str == "Builtin" || state_str == "StandardLibrary" {
                    builtin_resolved += count;
                }
            }
        }
        
        let mut evidence_stmt = db.conn.prepare(
            "SELECT evidence, COUNT(*) FROM relationships WHERE kind = 'method_call' GROUP BY evidence"
        )?;

        let evidence_rows = evidence_stmt.query_map([], |row| {
            let ev: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((ev, count))
        })?;

        for row in evidence_rows {
            if let Ok((ev, count)) = row {
                if let Some(ev_str) = ev {
                    metrics.insert(format!("Evidence: {}", ev_str), count as f64);
                }
            }
        }

        let repo_success = if total_method_calls > 0 {
            (repo_resolved as f64 / total_method_calls as f64) * 100.0
        } else {
            0.0
        };
        let ext_success = if total_method_calls > 0 {
            (ext_resolved as f64 / total_method_calls as f64) * 100.0
        } else {
            0.0
        };
        let builtin_success = if total_method_calls > 0 {
            (builtin_resolved as f64 / total_method_calls as f64) * 100.0
        } else {
            0.0
        };

        metrics.insert("Repository Success (%)".to_string(), repo_success);
        metrics.insert("External Success (%)".to_string(), ext_success);
        metrics.insert("Builtin Success (%)".to_string(), builtin_success);

        let status = if total_method_calls > 0 && repo_success < 50.0 {
            StageStatus::Warning
        } else {
            StageStatus::Pass
        };

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
