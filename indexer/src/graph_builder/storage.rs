use storage::Database;
use crate::graph_builder::result::GraphBuildResult;
use crate::graph_builder::metrics::GraphBuilderMetrics;

pub struct StorageWriter;

impl StorageWriter {
    /// Consumes a GraphBuildResult and performs the actual SQL batch insertion into the database.
    /// Returns the updated metrics containing SQLite operation telemetry.
    pub fn flush_edges(db: &Database, mut result: GraphBuildResult) -> Result<GraphBuilderMetrics, String> {
        for edge in result.edges {
            if let Err(_) = db.insert_edge_attributed(
                edge.source_file_id,
                edge.source_symbol_id,
                edge.target_symbol_id,
                &edge.kind,
            ) {
                result.metrics.sqlite_failures += 1;
            } else {
                result.metrics.sqlite_insertions += 1;
            }
        }
        
        Ok(result.metrics)
    }
}
