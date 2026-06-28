#[derive(Debug, Default, Clone)]
pub struct GraphBuilderMetrics {
    // Relationship Coverage
    pub relationships_input: usize,
    pub relationships_filtered: usize,
    pub relationships_invalid: usize,
    pub relationships_to_graph: usize,
    pub relationships_emitted: usize,
    pub relationships_skipped: usize,

    // Core Metrics
    pub repository_relationships: usize,
    pub dynamic_relationships: usize,
    pub external_relationships: usize,
    pub builtin_relationships: usize,
    pub missing_relationships: usize,
    pub ambiguous_relationships: usize,
    pub recursive_relationships: usize,

    // Edge tracking
    pub graph_edge_candidates: usize,
    pub edges_before_deduplication: usize,
    pub edges_after_deduplication: usize,
    pub duplicates_collapsed: usize,
    
    // SQLite tracking
    pub sqlite_insertions: usize,
    pub sqlite_failures: usize,
    
    // Safety
    pub missing_source_symbols: usize,
    pub missing_target_symbols: usize,
    pub self_loops: usize,
    pub orphan_relationships: usize,
}
