use rusqlite::Result;
use std::collections::HashMap;
use storage::Database;

/// Graph completeness metrics computed after every indexing pass and stored
/// in the `metadata` table under the key `"graph_metrics"`. Any regression in
/// connectivity or resolution rate across reruns is immediately visible here
/// without manually inspecting the edge table.
#[derive(Debug, Clone)]
pub struct GraphMetrics {
    pub total_files: i64,
    pub total_symbols: i64,
    pub total_edges: i64,
    pub orphan_symbols: i64,
    pub isolated_files: i64,
    /// Fraction of symbols with at least one edge.
    pub graph_connectivity: f64,
    /// Average edges per symbol.
    pub graph_density: f64,
    /// Fraction of relationships for which the linker produced an edge.
    pub import_resolution_rate: f64,
    /// Edge count per edge kind ("calls", "imports", "extends", …).
    pub edge_distribution: HashMap<String, i64>,
    /// Symbol count per symbol kind ("function", "class", "struct", …).
    pub symbol_distribution: HashMap<String, i64>,
}

impl GraphMetrics {
    pub fn to_markdown(&self) -> String {
        let mut md = String::new();
        md.push_str("## Graph Completeness Metrics\n\n");
        md.push_str("| Metric | Value |\n|---|---|\n");
        md.push_str(&format!("| Files | {} |\n", self.total_files));
        md.push_str(&format!("| Symbols | {} |\n", self.total_symbols));
        md.push_str(&format!("| Edges | {} |\n", self.total_edges));
        md.push_str(&format!("| Orphan symbols | {} |\n", self.orphan_symbols));
        md.push_str(&format!("| Isolated files | {} |\n", self.isolated_files));
        md.push_str(&format!(
            "| Graph connectivity | {:.1}% |\n",
            self.graph_connectivity * 100.0
        ));
        md.push_str(&format!(
            "| Average edges per symbol | {:.2} |\n",
            self.graph_density
        ));
        md.push_str(&format!(
            "| Import resolution rate | {:.1}% |\n",
            self.import_resolution_rate * 100.0
        ));

        md.push_str("\n### Edge Distribution\n\n| Kind | Count |\n|---|---|\n");
        let mut kinds: Vec<_> = self.edge_distribution.iter().collect();
        kinds.sort_by(|a, b| b.1.cmp(a.1));
        for (kind, count) in &kinds {
            md.push_str(&format!("| {} | {} |\n", kind, count));
        }

        md.push_str("\n### Symbol Distribution\n\n| Kind | Count |\n|---|---|\n");
        let mut sym_kinds: Vec<_> = self.symbol_distribution.iter().collect();
        sym_kinds.sort_by(|a, b| b.1.cmp(a.1));
        for (kind, count) in &sym_kinds {
            md.push_str(&format!("| {} | {} |\n", kind, count));
        }

        md
    }
}

pub fn compute_metrics(db: &Database) -> Result<GraphMetrics> {
    let total_files: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap_or(0);

    let total_symbols: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0))
        .unwrap_or(0);

    let total_edges: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0))
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

    let graph_connectivity = if total_symbols == 0 {
        1.0
    } else {
        (total_symbols - orphan_symbols) as f64 / total_symbols as f64
    };

    let graph_density = if total_symbols == 0 {
        0.0
    } else {
        total_edges as f64 / total_symbols as f64
    };

    let total_relationships: i64 = db
        .conn
        .query_row("SELECT COUNT(*) FROM relationships", [], |r| r.get(0))
        .unwrap_or(0);

    let unresolved: i64 = db
        .conn
        .query_row(
            "SELECT COUNT(*) FROM relationships ri
             WHERE NOT EXISTS (
                 SELECT 1 FROM edges e
                 WHERE e.source_file_id = ri.file_id
                   AND e.kind = COALESCE(ri.kind, 'imports')
             )",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);

    let import_resolution_rate = if total_relationships == 0 {
        1.0
    } else {
        (total_relationships - unresolved) as f64 / total_relationships as f64
    };

    let mut edge_distribution = HashMap::new();
    let mut estmt = db
        .conn
        .prepare("SELECT kind, COUNT(*) FROM edges GROUP BY kind ORDER BY COUNT(*) DESC")?;
    let mut erows = estmt.query([])?;
    while let Some(row) = erows.next()? {
        edge_distribution.insert(row.get::<_, String>(0)?, row.get::<_, i64>(1)?);
    }

    let mut symbol_distribution = HashMap::new();
    let mut sstmt = db
        .conn
        .prepare("SELECT kind, COUNT(*) FROM symbols GROUP BY kind ORDER BY COUNT(*) DESC")?;
    let mut srows = sstmt.query([])?;
    while let Some(row) = srows.next()? {
        symbol_distribution.insert(row.get::<_, String>(0)?, row.get::<_, i64>(1)?);
    }

    Ok(GraphMetrics {
        total_files,
        total_symbols,
        total_edges,
        orphan_symbols,
        isolated_files,
        graph_connectivity,
        graph_density,
        import_resolution_rate,
        edge_distribution,
        symbol_distribution,
    })
}

/// Serializes the metrics snapshot into the `metadata` table under
/// `"graph_metrics"`. The previous snapshot (if any) is overwritten so the
/// table always holds exactly one current snapshot, not an unbounded history.
pub fn save_metrics(db: &Database, metrics: &GraphMetrics) -> Result<()> {
    let json = serde_json::json!({
        "total_files": metrics.total_files,
        "total_symbols": metrics.total_symbols,
        "total_edges": metrics.total_edges,
        "orphan_symbols": metrics.orphan_symbols,
        "isolated_files": metrics.isolated_files,
        "graph_connectivity": metrics.graph_connectivity,
        "graph_density": metrics.graph_density,
        "import_resolution_rate": metrics.import_resolution_rate,
        "edge_distribution": metrics.edge_distribution,
        "symbol_distribution": metrics.symbol_distribution,
    });
    db.conn.execute(
        "INSERT OR REPLACE INTO metadata (key, value) VALUES ('graph_metrics', ?1)",
        rusqlite::params![json.to_string()],
    )?;
    Ok(())
}

/// Reads the most-recently saved metrics snapshot from the database.
/// Returns `None` if no snapshot has been saved yet (first-time index).
pub fn load_metrics(db: &Database) -> Option<serde_json::Value> {
    db.conn
        .query_row(
            "SELECT value FROM metadata WHERE key = 'graph_metrics'",
            [],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}
