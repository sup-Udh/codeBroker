use rusqlite::Result;
use storage::Database;
use crate::traits::{DiagnosticFinding, GraphValidator, Severity};

pub struct ImportValidator;

impl GraphValidator for ImportValidator {
    fn name(&self) -> &'static str {
        "ImportValidator"
    }

    fn validate(&self, db: &Database) -> Result<Vec<DiagnosticFinding>> {
        let mut findings = Vec::new();

        let mut unres_stmt = db.conn.prepare(
            "SELECT file_id, name, source, state, evidence 
             FROM relationships 
             WHERE (kind = 'imports' OR kind IS NULL)
               AND state != 'RepositorySymbol'
               AND state != 'ExternalDependency'
               AND state != 'StandardLibrary'
               AND state IS NOT NULL"
        )?;
        
        let unres_rows = unres_stmt.query_map([], |row| {
            let f: i64 = row.get(0)?;
            let n: String = row.get(1)?;
            let s: Option<String> = row.get(2)?;
            let state: String = row.get(3)?;
            let ev: Option<String> = row.get(4)?;
            Ok((f, n, s, state, ev))
        })?;

        for row in unres_rows {
            if let Ok((f, n, s, state, ev)) = row {
                let severity = match state.as_str() {
                    "Missing" => Severity::Warning,
                    "Unknown" => Severity::Warning,
                    _ => Severity::Info,
                };

                findings.push(DiagnosticFinding {
                    severity,
                    title: format!("Unresolved Import: {}", n),
                    description: format!("Failed to link import '{}' from source '{:?}'. State: {}", n, s, state),
                    likely_cause: ev.unwrap_or_else(|| "Unknown".to_string()),
                    suggested_fix: "Check file inclusion patterns or verify dependency existence.".to_string(),
                    file_id: Some(f),
                    symbol_id: None,
                });
            }
        }

        Ok(findings)
    }
}
