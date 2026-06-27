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

        let mut unres_stmt = db.conn.prepare(
            "SELECT file_id, name, kind, state, evidence 
             FROM relationships 
             WHERE (kind = 'calls' OR kind = 'method_call' OR kind = 'new_call')
               AND state != 'RepositorySymbol'
               AND state IS NOT NULL"
        )?;
        
        let unres_rows = unres_stmt.query_map([], |row| {
            let f: i64 = row.get(0)?;
            let n: String = row.get(1)?;
            let k: String = row.get(2)?;
            let state: String = row.get(3)?;
            let ev: Option<String> = row.get(4)?;
            Ok((f, n, k, state, ev))
        })?;

        for row in unres_rows {
            if let Ok((f, n, k, state, ev)) = row {
                let severity = if state == "Dynamic" {
                    Severity::Info
                } else {
                    Severity::Warning
                };

                let fix = if state == "Dynamic" {
                    "Implement basic type resolution or accept as dynamic.".to_string()
                } else {
                    "Check if the function is defined in a missing/ignored file, or is a builtin.".to_string()
                };

                findings.push(DiagnosticFinding {
                    severity,
                    title: format!("Unresolved Call: {} ({})", n, k),
                    description: format!("No target found for call expression '{}'. State: {}", n, state),
                    likely_cause: ev.unwrap_or_else(|| "Unknown".to_string()),
                    suggested_fix: fix,
                    file_id: Some(f),
                    symbol_id: None,
                });
            }
        }

        Ok(findings)
    }
}
