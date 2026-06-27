use rusqlite::Result;
use storage::Database;
use crate::traits::{DiagnosticFinding, GraphValidator, Severity};

pub struct EdgeValidator;

impl GraphValidator for EdgeValidator {
    fn name(&self) -> &'static str {
        "EdgeValidator"
    }

    fn validate(&self, db: &Database) -> Result<Vec<DiagnosticFinding>> {
        let mut findings = Vec::new();

        // Check for dangling edges
        let mut dangling_stmt = db.conn.prepare(
            "SELECT source_file_id, source_symbol_id, target_symbol_id 
             FROM edges 
             WHERE (source_symbol_id IS NOT NULL AND source_symbol_id NOT IN (SELECT id FROM symbols))
                OR target_symbol_id NOT IN (SELECT id FROM symbols)"
        )?;
        
        let dangling_rows = dangling_stmt.query_map([], |row| {
            let sf: i64 = row.get(0)?;
            let ss: Option<i64> = row.get(1)?;
            let ts: i64 = row.get(2)?;
            Ok((sf, ss, ts))
        })?;

        for row in dangling_rows {
            if let Ok((sf, ss, ts)) = row {
                findings.push(DiagnosticFinding {
                    severity: Severity::Critical,
                    title: "Dangling Edge".to_string(),
                    description: format!("An edge references a non-existent symbol. (source_symbol_id: {:?}, target_symbol_id: {})", ss, ts),
                    likely_cause: "A symbol was deleted during an incremental reindex, but edges pointing to it were not cascaded/cleaned up.".to_string(),
                    suggested_fix: "Ensure `delete_file_data` or foreign keys cascade deletes to the `edges` table.".to_string(),
                    file_id: Some(sf),
                    symbol_id: ss,
                });
            }
        }

        // Check for duplicate edges
        let mut dup_stmt = db.conn.prepare(
            "SELECT source_file_id, source_symbol_id, target_symbol_id, kind, COUNT(*) as c
             FROM edges
             GROUP BY source_file_id, source_symbol_id, target_symbol_id, kind
             HAVING c > 1"
        )?;

        let dup_rows = dup_stmt.query_map([], |row| {
            let sf: i64 = row.get(0)?;
            let ss: Option<i64> = row.get(1)?;
            let ts: i64 = row.get(2)?;
            let k: String = row.get(3)?;
            Ok((sf, ss, ts, k))
        })?;

        for row in dup_rows {
            if let Ok((sf, ss, _ts, k)) = row {
                findings.push(DiagnosticFinding {
                    severity: Severity::Error,
                    title: format!("Duplicate Edge ({})", k),
                    description: "The same edge was inserted multiple times.".to_string(),
                    likely_cause: "insert_edge_attributed dedup logic failed, or multiple relationships resolved to the same target.".to_string(),
                    suggested_fix: "Apply UNIQUE constraint on edges or filter duplicates at resolution time.".to_string(),
                    file_id: Some(sf),
                    symbol_id: ss,
                });
            }
        }
        
        // Self-loops
        let mut loop_stmt = db.conn.prepare(
            "SELECT source_file_id, source_symbol_id 
             FROM edges
             WHERE source_symbol_id IS NOT NULL AND source_symbol_id = target_symbol_id"
        )?;
        
        let loop_rows = loop_stmt.query_map([], |row| {
            let sf: i64 = row.get(0)?;
            let ss: i64 = row.get(1)?;
            Ok((sf, ss))
        })?;

        for row in loop_rows {
            if let Ok((sf, ss)) = row {
                findings.push(DiagnosticFinding {
                    severity: Severity::Warning,
                    title: "Self Loop Edge".to_string(),
                    description: "A symbol contains an edge pointing to itself.".to_string(),
                    likely_cause: "Usually benign recursion, but can indicate bad enclosing-symbol attribution where a local reference resolves globally to itself.".to_string(),
                    suggested_fix: "Skip self-referential edges in the linker or verify recursion is intended.".to_string(),
                    file_id: Some(sf),
                    symbol_id: Some(ss),
                });
            }
        }

        Ok(findings)
    }
}
