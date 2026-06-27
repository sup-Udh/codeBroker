use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;

use crate::traits::{DiagnosticFinding, GraphValidator, Severity};
use crate::report::{DiagnosticsReport, DiscoveryStats, GraphHealth, ResolutionStats};

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
        let total_relationships: i64 = db.conn.query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0)).unwrap_or(0);

        // Discovery stats: count relationships by kind
        let discovery = compute_discovery_stats(db);

        // Resolution stats: count relationships by resolution state
        let resolution = compute_resolution_stats(db);

        let mut findings = Vec::new();
        for validator in &self.validators {
            if let Ok(mut v_findings) = validator.validate(db) {
                findings.append(&mut v_findings);
            }
        }

        findings.sort_by_key(|f| match f.severity {
            Severity::Critical => 0,
            Severity::Error => 1,
            Severity::Warning => 2,
            Severity::Info => 3,
        });

        let mut metrics = HashMap::new();

        let dangling_edges = findings.iter().filter(|f| f.title.contains("Dangling Edge")).count() as i64;
        let duplicate_edges = findings.iter().filter(|f| f.title.contains("Duplicate Edge")).count() as i64;

        // Compute success metrics
        let compute_rate = |query: &str| -> f64 {
            db.conn.query_row(query, [], |r| r.get::<_, Option<f64>>(0))
                .unwrap_or(None)
                .unwrap_or(0.0)
        };

        let import_success = compute_rate(
            "SELECT SUM(CASE WHEN state IN ('RepositorySymbol', 'ExternalDependency', 'StandardLibrary') THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships WHERE kind = 'imports' OR kind IS NULL"
        );
        
        let method_success = compute_rate(
            "SELECT SUM(CASE WHEN state IN ('RepositorySymbol', 'ExternalDependency', 'Builtin', 'StandardLibrary') THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships WHERE kind = 'method_call'"
        );
        
        let type_success = compute_rate(
            "SELECT SUM(CASE WHEN state IN ('RepositorySymbol', 'ExternalDependency', 'Builtin', 'StandardLibrary') THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships WHERE kind = 'type_ref'"
        );

        let ambiguous_rate = compute_rate(
            "SELECT SUM(CASE WHEN state = 'Ambiguous' THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships"
        );

        let missing_rate = compute_rate(
            "SELECT SUM(CASE WHEN state = 'Missing' THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships"
        );

        let dynamic_rate = compute_rate(
            "SELECT SUM(CASE WHEN state = 'Dynamic' THEN 1 ELSE 0 END) * 100.0 / NULLIF(COUNT(*), 0) FROM relationships"
        );

        metrics.insert("Import Resolution Success (%)".to_string(), import_success);
        metrics.insert("Method Resolution Success (%)".to_string(), method_success);
        metrics.insert("Type Resolution Success (%)".to_string(), type_success);
        metrics.insert("Ambiguous Resolution Rate (%)".to_string(), ambiguous_rate);
        metrics.insert("Missing Resolution Rate (%)".to_string(), missing_rate);
        metrics.insert("Dynamic Fallback Rate (%)".to_string(), dynamic_rate);

        let health_score = (import_success / 100.0) - (dangling_edges as f64 * 0.1) - (duplicate_edges as f64 * 0.05);
        let health_score = health_score.clamp(0.0, 1.0);

        let passed = findings.iter().all(|f| match f.severity {
            Severity::Critical | Severity::Error => false,
            _ => true,
        });

        Ok(DiagnosticsReport {
            total_files,
            total_symbols,
            total_edges,
            total_relationships,
            discovery,
            resolution,
            findings,
            health: GraphHealth {
                score: health_score,
                metrics,
            },
            passed,
        })
    }
}

fn compute_discovery_stats(db: &Database) -> DiscoveryStats {
    let mut by_kind: HashMap<String, i64> = HashMap::new();
    let query = "SELECT COALESCE(kind, 'imports') as k, COUNT(*) as n FROM relationships GROUP BY k";
    if let Ok(mut stmt) = db.conn.prepare(query) {
        let _ = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }).map(|rows| {
            for row in rows.flatten() {
                by_kind.insert(row.0, row.1);
            }
        });
    }
    let total = by_kind.values().sum();
    DiscoveryStats { by_kind, total }
}

fn compute_resolution_stats(db: &Database) -> ResolutionStats {
    let mut by_state: HashMap<String, i64> = HashMap::new();
    let query = "SELECT COALESCE(state, 'Unknown') as s, COUNT(*) as n FROM relationships GROUP BY s";
    if let Ok(mut stmt) = db.conn.prepare(query) {
        let _ = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }).map(|rows| {
            for row in rows.flatten() {
                by_state.insert(row.0, row.1);
            }
        });
    }
    
    let mut by_evidence: HashMap<String, i64> = HashMap::new();
    let query_ev = "SELECT COALESCE(evidence, 'None') as e, COUNT(*) as n FROM relationships WHERE state != 'Unknown' AND state != 'Missing' GROUP BY e";
    if let Ok(mut stmt) = db.conn.prepare(query_ev) {
        let _ = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        }).map(|rows| {
            for row in rows.flatten() {
                by_evidence.insert(row.0, row.1);
            }
        });
    }
    
    let total = by_state.values().sum();
    ResolutionStats { by_state, by_evidence, total }
}
