use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus};
use std::time::Instant;

pub struct ResolutionQualityValidator;

impl PipelineValidator for ResolutionQualityValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::ResolutionQuality
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::PipelineHealth]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let findings = Vec::new(); // Metrics only, no errors unless severely degraded
        let mut metrics = HashMap::new();

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

        Ok(StageReport {
            stage: self.stage(),
            status: StageStatus::Pass,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
