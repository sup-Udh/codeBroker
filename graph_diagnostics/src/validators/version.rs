use rusqlite::Result;
use storage::Database;
use crate::traits::{PipelineValidator, PipelineStage, StageStatus, StageReport, DiagnosticFinding, Severity};

pub struct VersionValidator;

impl PipelineValidator for VersionValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Parser // We'll just run it early, or maybe create a new stage enum for Meta
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let mut report = StageReport {
            stage: PipelineStage::Parser,
            status: StageStatus::Pass,
            execution_time_ms: 0,
            metrics: Default::default(),
            findings: vec![],
        };

        let start = std::time::Instant::now();

        // Fetch stored versions
        let manifest_query = "SELECT parser_version, semantic_version, resolver_version, graph_version, embedding_version FROM pipeline_manifests ORDER BY id DESC LIMIT 1";
        let mut stmt = match db.conn.prepare(manifest_query) {
            Ok(stmt) => stmt,
            Err(_) => return Ok(report), // Table might not exist in old versions
        };

        let row = stmt.query_row([], |row| {
            let p: String = row.get(0)?;
            let s: String = row.get(1)?;
            let r: String = row.get(2)?;
            let g: String = row.get(3)?;
            let e: String = row.get(4)?;
            Ok((p, s, r, g, e))
        });

        if let Ok((p, s, r, g, e)) = row {
            let current_version = "1.0"; // For now, we hardcode. In reality, read from env or something.
            
            if p != current_version || s != current_version || r != current_version || g != current_version || e != current_version {
                report.status = StageStatus::Warning;
                report.findings.push(DiagnosticFinding {
                    severity: Severity::Warning,
                    title: "Compiler Version Mismatch".to_string(),
                    description: format!("Workspace indexed using older versions. Parser: {}, Semantic: {}, Resolver: {}, Graph: {}, Embedding: {}. Current is {}. Reindex Required.", p, s, r, g, e, current_version),
                    likely_cause: "The CodeBroker binary was updated after this workspace was indexed.".to_string(),
                    suggested_fix: "Run `codebroker init` to rebuild the workspace with the current compiler pipeline.".to_string(),
                    file_id: None,
                    symbol_id: None,
                });
            }
        }

        report.execution_time_ms = start.elapsed().as_millis();
        Ok(report)
    }
}
