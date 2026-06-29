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

        let mut origin_stmt = db.conn.prepare(
            "SELECT evidence, state, COUNT(*) FROM relationships WHERE kind = 'method_call' GROUP BY evidence, state"
        )?;

        let mut local_var_count = 0;
        let mut repo_import_count = 0;
        let mut external_count = 0;
        let mut builtin_count = 0;
        let mut stdlib_count = 0;
        let mut unknown_count = 0;

        let origin_rows = origin_stmt.query_map([], |row| {
            let ev: Option<String> = row.get(0)?;
            let state: Option<String> = row.get(1)?;
            let count: i64 = row.get(2)?;
            Ok((ev, state, count))
        })?;

        for row in origin_rows {
            if let Ok((ev, state, count)) = row {
                let ev_str = ev.unwrap_or_else(|| "None".to_string());
                let state_str = state.unwrap_or_else(|| "Unknown".to_string());

                if state_str == "RepositorySymbol" {
                    if ev_str == "VariableAssignment" || ev_str == "LexicalScopeMatch" || ev_str == "ReturnFlow" || ev_str == "ConstructorCall" {
                        local_var_count += count;
                    } else if ev_str == "ImportMatch" || ev_str == "ImportFlow" {
                        repo_import_count += count;
                    } else {
                        local_var_count += count;
                    }
                } else if state_str == "ExternalDependency" {
                    external_count += count;
                } else if state_str == "Builtin" {
                    builtin_count += count;
                } else if state_str == "StandardLibrary" {
                    stdlib_count += count;
                } else {
                    unknown_count += count;
                }
            }
        }

        metrics.insert("Origin: Local Variables".to_string(), local_var_count as f64);
        metrics.insert("Origin: Repository Imports".to_string(), repo_import_count as f64);
        metrics.insert("Origin: External Imports".to_string(), external_count as f64);
        metrics.insert("Origin: Builtin".to_string(), builtin_count as f64);
        metrics.insert("Origin: Standard Library".to_string(), stdlib_count as f64);
        metrics.insert("Origin: Unknown".to_string(), unknown_count as f64);

        let deterministic_resolved = local_var_count + repo_import_count + external_count + builtin_count + stdlib_count;
        let success_rate = if total_method_calls > 0 {
            (deterministic_resolved as f64 / total_method_calls as f64) * 100.0
        } else {
            0.0
        };

        metrics.insert("Deterministic Success (%)".to_string(), success_rate);

        let status = if total_method_calls > 0 && success_rate < 20.0 {
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
