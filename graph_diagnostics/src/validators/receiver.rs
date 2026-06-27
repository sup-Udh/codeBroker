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

        // Group by evidence to see receiver sources
        let mut evidence_stmt = db.conn.prepare(
            "SELECT evidence, COUNT(*) FROM relationships WHERE kind = 'method_call' GROUP BY evidence"
        )?;

        let mut resolved = 0;
        let mut unresolved = 0;

        let evidence_rows = evidence_stmt.query_map([], |row| {
            let ev: Option<String> = row.get(0)?;
            let count: i64 = row.get(1)?;
            Ok((ev, count))
        })?;

        for row in evidence_rows {
            if let Ok((ev, count)) = row {
                if let Some(ev_str) = ev {
                    metrics.insert(format!("Evidence: {}", ev_str), count as f64);
                    resolved += count;
                } else {
                    unresolved += count;
                }
            }
        }

        metrics.insert("Receivers Resolved".to_string(), resolved as f64);
        metrics.insert("Receivers Unresolved".to_string(), unresolved as f64);

        let success_rate = if total_method_calls > 0 {
            (resolved as f64 / total_method_calls as f64) * 100.0
        } else {
            0.0
        };
        metrics.insert("Receiver Resolution Success (%)".to_string(), success_rate);

        let status = if total_method_calls > 0 && success_rate < 50.0 {
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
