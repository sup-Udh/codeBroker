use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;

use crate::traits::{DiagnosticFinding, GraphValidator, Severity};
use crate::report::{DiagnosticsReport, GraphHealth};

pub struct DiagnosticsEngine {
    validators: Vec<Box<dyn GraphValidator>>,
}

impl DiagnosticsEngine {
    pub fn new() -> Self {
        Self {
            validators: Vec::new(),
        }
    }

    pub fn register_validator(&mut self, validator: Box<dyn GraphValidator>) {
        self.validators.push(validator);
    }

    pub fn run(&self, db: &Database) -> Result<DiagnosticsReport> {
        let total_files: i64 = db.conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0)).unwrap_or(0);
        let total_symbols: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
        let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
        let total_raw_imports: i64 = db.conn.query_row("SELECT COUNT(*) FROM raw_imports", [], |r| r.get(0)).unwrap_or(0);

        let mut findings = Vec::new();
        for validator in &self.validators {
            if let Ok(mut v_findings) = validator.validate(db) {
                findings.append(&mut v_findings);
            }
        }

        // Sort findings by severity (Critical first)
        findings.sort_by_key(|f| match f.severity {
            Severity::Critical => 0,
            Severity::Error => 1,
            Severity::Warning => 2,
            Severity::Info => 3,
        });

        // Calculate configurable health score
        let mut metrics = HashMap::new();
        
        let dangling_edges = findings.iter().filter(|f| f.title.contains("Dangling Edge")).count() as i64;
        let duplicate_edges = findings.iter().filter(|f| f.title.contains("Duplicate Edge")).count() as i64;
        let unresolved_imports = findings.iter().filter(|f| f.title.contains("Unresolved Import")).count() as i64;
        
        let import_resolution_rate = if total_raw_imports > 0 {
            (total_raw_imports - unresolved_imports).max(0) as f64 / total_raw_imports as f64
        } else {
            1.0
        };
        metrics.insert("import_resolution_rate".to_string(), import_resolution_rate);
        
        let dangling_penalty = (dangling_edges as f64 * 0.1).min(1.0);
        let duplicate_penalty = (duplicate_edges as f64 * 0.05).min(1.0);
        
        let health_score = (import_resolution_rate - dangling_penalty - duplicate_penalty).max(0.0);
        
        // Pass if no criticals or errors
        let passed = findings.iter().all(|f| match f.severity {
            Severity::Critical | Severity::Error => false,
            _ => true,
        });

        Ok(DiagnosticsReport {
            total_files,
            total_symbols,
            total_edges,
            total_raw_imports,
            findings,
            health: GraphHealth {
                score: health_score,
                metrics,
            },
            passed,
        })
    }
}
