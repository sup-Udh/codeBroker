use rusqlite::Result;
use storage::Database;

/// Structural correctness report produced after every indexing pass.
/// Detects impossible graph states — dangling edges, duplicates, self-loops —
/// and provides directional metrics (import resolution rate, connectivity) that
/// expose regressions in the graph builder without requiring manual inspection.
#[derive(Debug, Clone)]
pub struct ValidationReport {
    pub total_symbols: i64,
    pub total_files: i64,
    pub total_edges: i64,
    pub total_raw_imports: i64,
    /// Edges whose source_symbol_id or target_symbol_id no longer exist in
    /// the symbols table. Should always be zero; a non-zero value indicates a
    /// reindex that deleted symbols without cascading the delete to edges.
    pub dangling_edges: i64,
    /// (source_file_id, source_symbol_id, target_symbol_id, kind) tuples
    /// appearing more than once. Should always be zero given the dedup in
    /// `insert_edge_attributed`.
    pub duplicate_edges: i64,
    /// Edges where source_symbol_id == target_symbol_id. May be benign
    /// recursion but should be tracked; phantom self-loops indicate bad
    /// enclosing-symbol attribution.
    pub self_loops: i64,
    /// Symbols with no incoming or outgoing edge. High orphan count means
    /// the graph builder is failing to resolve most references.
    pub orphan_symbols: i64,
    /// Files from which no edge crosses a file boundary. A file with only
    /// self-contained definitions and no imports resolves correctly; this count
    /// surfaces files that should have external edges but don't.
    pub isolated_files: i64,
    /// Raw imports for which the linker created no edge of the matching kind
    /// from the same file. Over-counts stdlib/builtin names that legitimately
    /// don't resolve, but gives a useful directional measure.
    pub unresolved_raw_imports: i64,
    /// Human-readable descriptions of each non-zero issue found.
    pub issues: Vec<String>,
}

impl ValidationReport {
    /// Fraction of raw_imports for which at least one edge was created.
    pub fn import_resolution_rate(&self) -> f64 {
        if self.total_raw_imports == 0 {
            return 1.0;
        }
        (self.total_raw_imports - self.unresolved_raw_imports) as f64
            / self.total_raw_imports as f64
    }

    /// Fraction of symbols that participate in at least one edge.
    pub fn graph_connectivity(&self) -> f64 {
        if self.total_symbols == 0 {
            return 1.0;
        }
        (self.total_symbols - self.orphan_symbols) as f64 / self.total_symbols as f64
    }

    pub fn is_valid(&self) -> bool {
        self.dangling_edges == 0 && self.duplicate_edges == 0
    }

    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("## Graph Validation Report\n\n");
        md.push_str("| Metric | Value |\n|---|---|\n");
        md.push_str(&format!("| Files indexed | {} |\n", self.total_files));
        md.push_str(&format!("| Symbols indexed | {} |\n", self.total_symbols));
        md.push_str(&format!("| Edges | {} |\n", self.total_edges));
        md.push_str(&format!("| Raw imports staged | {} |\n", self.total_raw_imports));
        md.push_str(&format!("| Dangling edges | {} |\n", self.dangling_edges));
        md.push_str(&format!("| Duplicate edges | {} |\n", self.duplicate_edges));
        md.push_str(&format!("| Self-loops | {} |\n", self.self_loops));
        md.push_str(&format!("| Orphan symbols | {} |\n", self.orphan_symbols));
        md.push_str(&format!("| Isolated files | {} |\n", self.isolated_files));
        md.push_str(&format!(
            "| Import resolution rate | {:.1}% |\n",
            self.import_resolution_rate() * 100.0
        ));
        md.push_str(&format!(
            "| Graph connectivity | {:.1}% |\n",
            self.graph_connectivity() * 100.0
        ));
        if self.issues.is_empty() {
            md.push_str("\nNo structural issues detected.\n");
        } else {
            md.push_str("\n### Issues\n");
            for issue in &self.issues {
                md.push_str(&format!("- {}\n", issue));
            }
        }
        md
    }
}

/// Validates the graph after an indexing pass. Runs purely via SQL on the
/// existing database; does not modify any data.
pub fn validate(db: &Database) -> Result<ValidationReport> {
    let total_symbols: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
        .unwrap_or(0);

    let total_files: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap_or(0);

    let total_edges: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
        .unwrap_or(0);

    let total_raw_imports: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM raw_imports", [], |r| r.get(0))
        .unwrap_or(0);

    let dangling_edges: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM edges
             WHERE (source_symbol_id IS NOT NULL
                    AND source_symbol_id NOT IN (SELECT id FROM symbols))
                OR target_symbol_id NOT IN (SELECT id FROM symbols)",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let duplicate_edges: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM (
                 SELECT source_file_id, source_symbol_id, target_symbol_id, kind
                 FROM edges
                 GROUP BY source_file_id, source_symbol_id, target_symbol_id, kind
                 HAVING COUNT(*) > 1
             )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let self_loops: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM edges
             WHERE source_symbol_id IS NOT NULL AND source_symbol_id = target_symbol_id",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let orphan_symbols: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM symbols
             WHERE id NOT IN (SELECT target_symbol_id FROM edges)
               AND id NOT IN (
                   SELECT source_symbol_id FROM edges WHERE source_symbol_id IS NOT NULL
               )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Files from which no edge leaves to a different file.
    let isolated_files: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM files f
             WHERE NOT EXISTS (
                 SELECT 1 FROM edges e
                 JOIN symbols s ON e.target_symbol_id = s.id
                 WHERE e.source_file_id = f.id AND s.file_id != f.id
             )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    // Raw imports for which no edge of the same kind was created from the same
    // file. This over-estimates unresolved for stdlib names, but gives a
    // consistent directional signal across reindexes.
    let unresolved_raw_imports: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM raw_imports ri
             WHERE NOT EXISTS (
                 SELECT 1 FROM edges e
                 WHERE e.source_file_id = ri.file_id
                   AND e.kind = COALESCE(ri.kind, 'imports')
             )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let mut issues = Vec::new();
    if dangling_edges > 0 {
        issues.push(format!(
            "{} dangling edges reference deleted symbols — indicates incomplete cascade on delete_file_data",
            dangling_edges
        ));
    }
    if duplicate_edges > 0 {
        issues.push(format!(
            "{} duplicate (source, target, kind) triples — dedup in insert_edge_attributed is not working",
            duplicate_edges
        ));
    }
    if self_loops > 0 {
        issues.push(format!(
            "{} self-loop edges — verify these are genuine recursive functions, not bad enclosing-symbol attribution",
            self_loops
        ));
    }

    Ok(ValidationReport {
        total_symbols,
        total_files,
        total_edges,
        total_raw_imports,
        dangling_edges,
        duplicate_edges,
        self_loops,
        orphan_symbols,
        isolated_files,
        unresolved_raw_imports,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use graph::SymbolNode;

    fn sym(db: &Database, file_id: i64, name: &str) -> i64 {
        db.insert_symbol(
            file_id,
            &SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 3,
                start_byte: 0,
                end_byte: 100,
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn empty_db_is_valid() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let report = validate(&db).unwrap();
        assert!(report.is_valid());
        assert_eq!(report.dangling_edges, 0);
        assert_eq!(report.duplicate_edges, 0);
    }

    #[test]
    fn connected_symbols_have_zero_orphans() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f1 = db.insert_file("a.ts", "h").unwrap();
        let f2 = db.insert_file("b.ts", "h").unwrap();
        let a = sym(&db, f1, "a");
        let b = sym(&db, f2, "b");
        db.insert_edge_attributed(f1, Some(a), b, "calls").unwrap();
        let report = validate(&db).unwrap();
        assert_eq!(report.orphan_symbols, 0);
        assert_eq!(report.isolated_files, 1); // b.ts has no outgoing cross-file edges
    }

    #[test]
    fn graph_connectivity_reflects_connected_fraction() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("x.ts", "h").unwrap();
        let a = sym(&db, f, "a");
        let b = sym(&db, f, "b");
        let _c = sym(&db, f, "c"); // orphan
        db.insert_edge_attributed(f, Some(a), b, "calls").unwrap();
        let report = validate(&db).unwrap();
        // 2 connected, 1 orphan, total 3
        assert_eq!(report.orphan_symbols, 1);
        assert!((report.graph_connectivity() - 2.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn import_resolution_rate_zero_when_no_edges() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let f = db.insert_file("a.py", "h").unwrap();
        db.insert_raw_import(
            f,
            &graph::ImportNode {
                name: "something".to_string(),
                source: None,
                line_number: 1,
                kind: Some("imports".to_string()),
            },
        )
        .unwrap();
        let report = validate(&db).unwrap();
        // 1 raw import, 0 edges → 0% resolution
        assert_eq!(report.total_raw_imports, 1);
        assert_eq!(report.unresolved_raw_imports, 1);
        assert!((report.import_resolution_rate() - 0.0).abs() < 1e-9);
    }
}
