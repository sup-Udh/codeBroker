use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;
use indexer::resolver::run_resolver;
use indexer::graph_builder::GraphBuilder;

pub struct GraphValidatorObj;

impl PipelineValidator for GraphValidatorObj {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Graph
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::ResolutionQuality]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        // Run the resolver pipeline in memory, then GraphBuilder in dry-run mode
        let resolved_ir = run_resolver(db, None).unwrap_or_else(|_| Vec::new());
        let build_result = GraphBuilder::build(resolved_ir);
        let gbm = build_result.metrics;

        // Metrics the user specifically requested:
        metrics.insert("Duplicate Relationships Collapsed".to_string(), gbm.duplicates_collapsed as f64);
        metrics.insert("Duplicate SQL Insert Attempts".to_string(), gbm.sqlite_failures as f64);
        
        let dedup_efficiency = if gbm.edges_before_deduplication > 0 {
            (gbm.duplicates_collapsed as f64 / gbm.edges_before_deduplication as f64) * 100.0
        } else {
            100.0
        };
        metrics.insert("GraphBuilder Dedup Efficiency".to_string(), dedup_efficiency);
        metrics.insert("Repository Relationships".to_string(), gbm.repository_relationships as f64);
        metrics.insert("Graph Edges Produced".to_string(), gbm.relationships_emitted as f64);
        metrics.insert("Repository Relationships Missing Graph Edge".to_string(), gbm.missing_target_symbols as f64);

        // SQL counts for comparison
        let total_edges: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM edges WHERE edge_type = 'static'",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Total Static Edges in DB".to_string(), total_edges as f64);

        let integrity_score = if (gbm.edges_after_deduplication as i64) == total_edges {
            100.0
        } else {
            0.0
        };
        metrics.insert("Graph Integrity Score".to_string(), integrity_score);

        if integrity_score < 100.0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Critical,
                title: "Graph Integrity Violation".to_string(),
                description: format!("GraphBuilder produced {} edges but database contains {} static edges.", gbm.edges_after_deduplication, total_edges),
                likely_cause: "Edges were inserted outside of GraphBuilder or database constraint failed.".to_string(),
                suggested_fix: "Ensure all edge insertions route through GraphBuilder.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        // Check for duplicates directly in SQLite just in case something bypassed GraphBuilder
        let duplicate_edges: i64 = db.conn.query_row(
            "SELECT SUM(c - 1) FROM (
                 SELECT COUNT(*) as c FROM edges 
                 WHERE edge_type = 'static'
                 GROUP BY source_file_id, source_symbol_id, target_symbol_id, kind 
                 HAVING c > 1
             )",
            [],
            |row| row.get(0)
        ).unwrap_or(0);

        if duplicate_edges > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Error,
                title: "Duplicate Edges Detected".to_string(),
                description: format!("Found {} duplicate graph edges in SQLite.", duplicate_edges),
                likely_cause: "GraphBuilder deduplication failed, or edges inserted manually bypassed it.".to_string(),
                suggested_fix: "Check EdgeAccumulator logic.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        if gbm.missing_target_symbols > 0 {
            findings.push(DiagnosticFinding {
                severity: Severity::Error,
                title: "Repository Relationships Missing Graph Edge".to_string(),
                description: format!("{} relationships missing targets.", gbm.missing_target_symbols),
                likely_cause: "Resolver produced RepositorySymbol without IDs.".to_string(),
                suggested_fix: "Fix resolver target emission.".to_string(),
                file_id: None,
                symbol_id: None,
            });
        }

        if gbm.orphan_relationships > 0 {
             findings.push(DiagnosticFinding {
                severity: Severity::Error,
                title: "Orphan Edges Detected".to_string(),
                description: format!("{} orphan edges detected in GraphBuilder.", gbm.orphan_relationships),
                likely_cause: "Unknown states leaking into builder.".to_string(),
                suggested_fix: "Fix resolver states.".to_string(),
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
