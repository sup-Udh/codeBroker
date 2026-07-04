use storage::Database;
use crate::graph_builder::result::GraphBuildResult;
use crate::graph_builder::metrics::GraphBuilderMetrics;

pub struct StorageWriter;

impl StorageWriter {
    /// Consumes a GraphBuildResult and performs the actual SQL batch insertion into the database.
    /// Returns the updated metrics containing SQLite operation telemetry.
    ///
    /// `result.edges` comes out of a `HashSet`-backed accumulator (dedup-only,
    /// no ordering guarantee), so its iteration order is not deterministic
    /// across runs. Sorted here — before the one transaction that inserts
    /// them all — so which edge lands on which auto-increment id is stable
    /// run over run regardless of the accumulator's internal hashing.
    pub fn flush_edges(db: &Database, mut result: GraphBuildResult) -> Result<GraphBuilderMetrics, String> {
        result.edges.sort_by(|a, b| {
            (a.source_file_id, a.source_symbol_id, a.target_symbol_id, &a.kind).cmp(&(
                b.source_file_id,
                b.source_symbol_id,
                b.target_symbol_id,
                &b.kind,
            ))
        });

        db.conn
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|e| e.to_string())?;
        for edge in result.edges {
            if let Err(_) = db.insert_edge_attributed_with_confidence(
                edge.source_file_id,
                edge.source_symbol_id,
                edge.target_symbol_id,
                &edge.kind,
                edge.confidence,
            ) {
                result.metrics.sqlite_failures += 1;
            } else {
                result.metrics.sqlite_insertions += 1;
            }
        }
        db.conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;

        Ok(result.metrics)
    }
}
