use rusqlite::Result;
use storage::Database;
use crate::traits::{DiagnosticFinding, GraphValidator, Severity};

pub struct SymbolValidator;

impl GraphValidator for SymbolValidator {
    fn name(&self) -> &'static str {
        "SymbolValidator"
    }

    fn validate(&self, db: &Database) -> Result<Vec<DiagnosticFinding>> {
        let mut findings = Vec::new();

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

        Ok(findings)
    }
}
