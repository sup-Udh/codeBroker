use rusqlite::Result;
use storage::Database;
use crate::traits::{DiagnosticFinding, GraphValidator, Severity};

pub struct CallValidator;

impl GraphValidator for CallValidator {
    fn name(&self) -> &'static str {
        "CallValidator"
    }

    fn validate(&self, db: &Database) -> Result<Vec<DiagnosticFinding>> {
        let mut findings = Vec::new();

        // Check for unresolved calls. 
        // We look for raw_imports of kind 'calls' or 'method_call' that have no corresponding edges.
        let mut unres_stmt = db.conn.prepare(
            "SELECT file_id, name, kind 
             FROM raw_imports ri
             WHERE (kind = 'calls' OR kind = 'method_call' OR kind = 'new_call')
               AND NOT EXISTS (
                 SELECT 1 FROM edges e
                 WHERE e.source_file_id = ri.file_id
                   AND e.kind = ri.kind
               )"
        )?;
        
        let unres_rows = unres_stmt.query_map([], |row| {
            let f: i64 = row.get(0)?;
            let n: String = row.get(1)?;
            let k: String = row.get(2)?;
            Ok((f, n, k))
        })?;

        for row in unres_rows {
            if let Ok((f, n, k)) = row {
                let (cause, severity, fix) = if k == "method_call" {
                    ("Dynamic/Method call: Cannot resolve method calls without type inference.".to_string(), Severity::Info, "Implement basic type resolution or accept as dynamic.".to_string())
                } else {
                    ("Unresolved free call: Target function not found in index.".to_string(), Severity::Warning, "Check if the function is defined in a missing/ignored file, or is a builtin.".to_string())
                };

                findings.push(DiagnosticFinding {
                    severity,
                    title: format!("Unresolved Call: {} ({})", n, k),
                    description: format!("No target found for call expression '{}'.", n),
                    likely_cause: cause,
                    suggested_fix: fix,
                    file_id: Some(f),
                    symbol_id: None,
                });
            }
        }

        Ok(findings)
    }
}
