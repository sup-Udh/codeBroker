use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus, Severity};
use std::time::Instant;

pub struct ParserValidator;

impl PipelineValidator for ParserValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Parser
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let mut findings = Vec::new();
        let mut metrics = HashMap::new();

        // 1. Files parsed
        let files_parsed: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM files",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Files Parsed".to_string(), files_parsed as f64);

        // 2. Symbols extracted
        let symbols_extracted: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM symbols",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Symbols Extracted".to_string(), symbols_extracted as f64);

        // 3. Relationships extracted (raw)
        let relationships_extracted: i64 = db.conn.query_row(
            "SELECT COUNT(*) FROM relationships",
            [],
            |row| row.get(0)
        ).unwrap_or(0);
        metrics.insert("Relationships Extracted".to_string(), relationships_extracted as f64);

        // Check for duplicate definitions
        let mut dup_stmt = db.conn.prepare(
            "SELECT file_id, name, COUNT(*) as c
             FROM symbols
             GROUP BY file_id, name, kind, start_line
             HAVING c > 1"
        )?;

        let dup_rows = dup_stmt.query_map([], |row| {
            let file_id: i64 = row.get(0)?;
            let name: String = row.get(1)?;
            let count: i64 = row.get(2)?;
            Ok((file_id, name, count))
        })?;

        for dup in dup_rows {
            if let Ok((file_id, name, _count)) = dup {
                findings.push(DiagnosticFinding {
                    severity: Severity::Warning,
                    title: format!("Duplicate Symbol Definition: {}", name),
                    description: format!("Symbol '{}' is defined multiple times at the same location.", name),
                    likely_cause: "Tree-sitter queries are matching the same syntax node multiple times without predicates, or overlapping captures.".to_string(),
                    suggested_fix: "Refine Tree-sitter query predicates (e.g. #not-eq) to prevent multiple captures for the same node.".to_string(),
                    file_id: Some(file_id),
                    symbol_id: None,
                });
            }
        }

        // Check for unnamed symbols
        let mut unnamed_stmt = db.conn.prepare("SELECT id, file_id FROM symbols WHERE name = '' OR name IS NULL")?;
        let unnamed_rows = unnamed_stmt.query_map([], |row| {
            let id: i64 = row.get(0)?;
            let file_id: i64 = row.get(1)?;
            Ok((id, file_id))
        })?;

        for row in unnamed_rows {
            if let Ok((id, file_id)) = row {
                findings.push(DiagnosticFinding {
                    severity: Severity::Error,
                    title: "Unnamed Symbol".to_string(),
                    description: "A symbol was indexed with an empty or null name.".to_string(),
                    likely_cause: "Missing `@name` capture in Tree-sitter query, or extracting an unnamed anonymous block as a named symbol.".to_string(),
                    suggested_fix: "Ensure all symbol extractions capture a valid identifier for `@name`.".to_string(),
                    file_id: Some(file_id),
                    symbol_id: Some(id),
                });
            }
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
