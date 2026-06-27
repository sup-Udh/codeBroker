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

        // Check for completely unresolved imports
        let mut unres_stmt = db.conn.prepare(
            "SELECT file_id, name, source 
             FROM raw_imports ri
             WHERE (kind = 'imports' OR kind IS NULL)
               AND NOT EXISTS (
                 SELECT 1 FROM edges e
                 WHERE e.source_file_id = ri.file_id
                   AND e.kind = COALESCE(ri.kind, 'imports')
               )"
        )?;
        
        let unres_rows = unres_stmt.query_map([], |row| {
            let f: i64 = row.get(0)?;
            let n: String = row.get(1)?;
            let s: Option<String> = row.get(2)?;
            Ok((f, n, s))
        })?;

        for row in unres_rows {
            if let Ok((f, n, s)) = row {
                // Heuristic failure classifications
                let (likely_cause, fix) = if let Some(src) = &s {
                    if src.starts_with('@') || src.starts_with('~') {
                        ("Alias failure: Path alias in config not matched.".to_string(), "Check alias_map in the linker and tsconfig/vite mappings.".to_string())
                    } else if src.contains("node_modules") || !src.starts_with('.') {
                        ("External/Library import: Standard library or third-party dependency.".to_string(), "Benign. Ignore or filter external imports.".to_string())
                    } else {
                        ("Missing file: Local path did not match an indexed file.".to_string(), "Check file inclusion patterns or path resolution logic.".to_string())
                    }
                } else if n == "*" {
                    ("Namespace import: '*' cannot be resolved to a single symbol.".to_string(), "Implement namespace/module-level edge tracking.".to_string())
                } else {
                    ("Unresolved Global: Name not found in any file.".to_string(), "Symbol not indexed or missing from codebase.".to_string())
                };

                let severity = if likely_cause.contains("External") {
                    Severity::Info
                } else {
                    Severity::Warning
                };

                findings.push(DiagnosticFinding {
                    severity,
                    title: format!("Unresolved Import: {}", n),
                    description: format!("Failed to link import '{}' from source '{:?}'.", n, s),
                    likely_cause,
                    suggested_fix: fix,
                    file_id: Some(f),
                    symbol_id: None,
                });
            }
        }

        Ok(findings)
    }
}
