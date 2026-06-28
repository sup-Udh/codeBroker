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
            PipelineStage::ResolutionQuality,
            PipelineStage::Graph,
        ]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        // 1. Any relationship marked RepositorySymbol MUST have a corresponding edge
        let missing_edges_for_repo_symbols: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships 
             WHERE state = 'RepositorySymbol'
               AND NOT EXISTS (
                   SELECT 1 FROM edges 
                   WHERE edges.source_file_id = relationships.file_id 
                     AND edges.kind = COALESCE(relationships.kind, 'imports')
               )",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if missing_edges_for_repo_symbols > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "RepositorySymbol without Graph Edge".to_string(),
                description: format!("Found {} relationships marked RepositorySymbol but no corresponding edge exists in the graph.", missing_edges_for_repo_symbols),
                likely_cause: "The resolver correctly determined the target, but graph builder failed to link it, possibly due to a missing enclosing symbol or duplicate collapse error.".to_string(),
                suggested_fix: "Check the enclosing_symbol lookup logic and GraphBuilderMetrics.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }
        
        // 2. All symbols in edges must exist in symbol_features (Feature Extraction processed)
        let unfeatured_nodes: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT source_symbol_id as sid FROM edges WHERE source_symbol_id IS NOT NULL
                UNION
                SELECT target_symbol_id as sid FROM edges
             ) AS nodes
             WHERE NOT EXISTS (SELECT 1 FROM symbol_features sf WHERE sf.symbol_id = nodes.sid)",
             [],
             |row| row.get(0)
        ).unwrap_or(0);
        
        if unfeatured_nodes > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Error,
                title: "Nodes Missing Feature Extraction".to_string(),
                description: format!("Found {} symbols participating in graph edges that have no feature rows.", unfeatured_nodes),
                likely_cause: "Feature extraction stage crashed, skipped nodes, or ran out of order.".to_string(),
                suggested_fix: "Ensure feature extraction runs after graph building completes.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        let has_errors = findings.iter().any(|f| matches!(f.severity, Severity::Error | Severity::Critical));
        let status = if has_errors { StageStatus::Fail } else if !findings.is_empty() { StageStatus::Warning } else { StageStatus::Pass };

        metrics.insert("Repository Symbols Missing Edges".to_string(), missing_edges_for_repo_symbols as f64);
        metrics.insert("Graph Nodes Missing Features".to_string(), unfeatured_nodes as f64);

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
