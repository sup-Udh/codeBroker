use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct MethodValidator;

impl PipelineValidator for MethodValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Method
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Receiver]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        let total_method_calls: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships WHERE kind = 'method_call'",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Method Calls".to_string(), total_method_calls as f64);

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
                metrics.insert(format!("State: {}", state_str), count as f64);
                if state_str == "RepositorySymbol" {
                    repo_resolved += count;
                } else if state_str == "ExternalDependency" {
                    ext_resolved += count;
                } else if state_str == "Builtin" || state_str == "StandardLibrary" {
                    builtin_resolved += count;
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

        // Find unresolved methods to report as findings
        let mut unres_stmt = db.conn.prepare(
            "SELECT file_id, name, state, evidence
             FROM relationships
             WHERE kind = 'method_call'
               AND state != 'RepositorySymbol'
               AND state IS NOT NULL"
        )?;

        let unres_rows = unres_stmt.query_map([], |row| {
            let f: i64 = row.get(0)?;
            let n: String = row.get(1)?;
            let state: String = row.get(2)?;
            let ev: Option<String> = row.get(3)?;
            Ok((f, n, state, ev))
        })?;

        let mut sample_count = 0;
        for row in unres_rows {
            if let Ok((f, n, state, ev)) = row {
                if sample_count < 10 { // Limit findings to avoid overwhelming report
                    let severity = if state == "Dynamic" { Severity::Info } else { Severity::Warning };
                    findings.push(DiagnosticFinding {
                        severity,
                        title: format!("Unresolved Method Call: {}", n),
                        description: format!("No target found for method call '{}'. State: {}", n, state),
                        likely_cause: ev.unwrap_or_else(|| "Unknown".to_string()),
                        suggested_fix: "Check if the receiver type was resolved, or if the method is external/dynamic.".to_string(),
                        file_id: Some(f),
                        symbol_id: None,
                    });
                    sample_count += 1;
                }
            }
        }

        let status = if total_method_calls > 0 && repo_success < 20.0 {
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
