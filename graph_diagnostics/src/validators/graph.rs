use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct GraphValidatorObj;

impl PipelineValidator for GraphValidatorObj {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Graph
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Resolver]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        let total_edges: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM edges",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Total Edges".to_string(), total_edges as f64);

        let total_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        let density = if total_symbols > 1 {
            total_edges as f64 / (total_symbols as f64 * (total_symbols as f64 - 1.0))
        } else {
            0.0
        };
        metrics.insert("Graph Density".to_string(), density);

        // Dangling edges
        let dangling_edges: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM edges 
             WHERE source_symbol_id NOT IN (SELECT id FROM symbols)
                OR target_symbol_id NOT IN (SELECT id FROM symbols)",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if dangling_edges > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Dangling Edges Detected".to_string(),
                description: format!("Found {} edges pointing to non-existent symbols.", dangling_edges),
                likely_cause: "Symbol deletion failed to cascade, or an invalid target was linked during resolution.".to_string(),
                suggested_fix: "Verify ON DELETE CASCADE triggers, or ensure resolver validates targets before insertion.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        // Duplicate edges
        let duplicate_edges: i64 = db.conn.query_row(
            "SELECT SUM(c - 1) FROM (
                 SELECT COUNT(*) as c FROM edges 
                 GROUP BY source_symbol_id, target_symbol_id, kind 
                 HAVING c > 1
             )",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if duplicate_edges > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Warning,
                title: "Duplicate Edges Detected".to_string(),
                description: format!("Found {} duplicate graph edges.", duplicate_edges),
                likely_cause: "Resolver process ran multiple times without deduplication, or multiple identical calls exist without grouping.".to_string(),
                suggested_fix: "Ensure graph edge insertions use INSERT OR IGNORE, or deduplicate relationships prior to linking.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        // Self-loops
        let self_loops: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE source_symbol_id = target_symbol_id",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if self_loops > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Info,
                title: "Self-Looping Edges Detected".to_string(),
                description: format!("Found {} self-looping graph edges.", self_loops),
                likely_cause: "Recursive functions, or incorrect scope resolution assigning an edge to the enclosing scope.".to_string(),
                suggested_fix: "Verify if self-loops are expected (e.g. recursive). If not, ensure resolution skips the immediate enclosing scope.".to_string(),
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
