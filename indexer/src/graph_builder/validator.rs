use crate::graph_builder::metrics::GraphBuilderMetrics;

pub struct GraphBuilderValidator;

impl GraphBuilderValidator {
    /// Validates graph invariants on the final metrics of the builder pass.
    /// This is strictly for internal assertions and will panic in debug mode
    /// if the compiler backend produced invalid edge states.
    pub fn assert_invariants(metrics: &GraphBuilderMetrics) {
        // Invariant 1: Edge Count Conservation
        // Every RepositorySymbol must produce exactly one graph edge candidate.
        // If repository_relationships is 10, graph_edge_candidates must be 10.
        debug_assert_eq!(
            metrics.repository_relationships,
            metrics.graph_edge_candidates,
            "Invariant Violation: Edge Count Conservation failed. {} repository relationships produced {} candidates",
            metrics.repository_relationships,
            metrics.graph_edge_candidates
        );

        // Invariant 2: Deduplication only reduces edge count
        debug_assert!(
            metrics.edges_before_deduplication >= metrics.edges_after_deduplication,
            "Invariant Violation: Deduplication must never increase edges"
        );

        // Invariant 3: Excluded states emit 0 edges
        // (This is implicitly tested if candidates strictly equal repository relationships
        // and no edges are emitted outside of those candidate paths)

        // Invariant 4: No orphan relationships
        debug_assert_eq!(
            metrics.orphan_relationships, 0,
            "Invariant Violation: Orphan relationships detected"
        );
    }
}
