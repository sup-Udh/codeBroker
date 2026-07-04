use crate::ir::ResolvedRelationshipIR;
use graph::models::ResolutionState;
use crate::graph_builder::types::GraphEdgeIR;
use crate::graph_builder::metrics::GraphBuilderMetrics;
use crate::graph_builder::accumulator::EdgeAccumulator;
use crate::graph_builder::result::GraphBuildResult;
use crate::graph_builder::validator::GraphBuilderValidator;

pub struct GraphBuilder;

impl GraphBuilder {
    /// Consumes resolved relationships and builds a validated, deduplicated graph representation in memory.
    /// Performs ZERO SQLite operations.
    pub fn build(relationships: Vec<ResolvedRelationshipIR>) -> GraphBuildResult {
        let mut metrics = GraphBuilderMetrics::default();
        let mut accumulator = EdgeAccumulator::new();
        let mut diagnostics = Vec::new();

        metrics.relationships_input = relationships.len();

        for resolved in relationships {
            metrics.relationships_filtered += 1;

            match resolved.state {
                ResolutionState::RepositorySymbol => {
                    metrics.repository_relationships += 1;
                    metrics.relationships_to_graph += 1;
                }
                ResolutionState::ExternalDependency | ResolutionState::WorkspaceModule => {
                    metrics.external_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::StandardLibrary | ResolutionState::Builtin => {
                    metrics.builtin_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::Dynamic => {
                    metrics.dynamic_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::Missing => {
                    metrics.missing_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::Ambiguous => {
                    metrics.ambiguous_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::Recursive => {
                    metrics.recursive_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                ResolutionState::Unknown => {
                    metrics.orphan_relationships += 1;
                    metrics.relationships_invalid += 1;
                    diagnostics.push(format!("Orphan relationship detected: {}", resolved.original.id));
                }
            }

            if resolved.state == ResolutionState::RepositorySymbol {
                // Every RepositorySymbol MUST produce exactly one edge candidate
                metrics.graph_edge_candidates += 1;
                
                if resolved.target_symbol_ids.is_empty() {
                    metrics.missing_target_symbols += 1;
                    metrics.relationships_invalid += 1;
                    diagnostics.push(format!("RepositorySymbol has no target_symbol_ids: {}", resolved.original.id));
                    continue; // Should not happen in a valid resolver pipeline, but let's prevent panic in builder
                }

                let target_id = resolved.target_symbol_ids[0];
                let source_file_id = resolved.original.source_file_id;
                let edge_kind = resolved.original.node.kind.clone().unwrap_or_else(|| "imports".to_string());
                let src_sym = resolved.original.enclosing_symbol_id;

                if Some(target_id) == src_sym {
                    // Self loops should be caught by resolver as Recursive, but we track just in case
                    metrics.self_loops += 1;
                    metrics.relationships_invalid += 1;
                } else {
                    let edge = GraphEdgeIR {
                        source_file_id,
                        source_symbol_id: src_sym,
                        target_symbol_id: target_id,
                        kind: edge_kind,
                        edge_type: "static".to_string(),
                        confidence: resolved.confidence,
                        relationship_id: resolved.original.id,
                    };
                    
                    metrics.edges_before_deduplication += 1;
                    
                    if accumulator.insert(edge) {
                        metrics.relationships_emitted += 1;
                    } else {
                        metrics.duplicates_collapsed += 1;
                    }
                }
            }
        }

        metrics.edges_after_deduplication = accumulator.len();

        // Enforce invariants on debug mode
        GraphBuilderValidator::assert_invariants(&metrics);

        let mut unique_edges: Vec<GraphEdgeIR> = accumulator.into_inner().into_iter().collect();

        // Sort for strict deterministic insertion order
        unique_edges.sort_by_key(|e| {
            (e.source_file_id, e.source_symbol_id, e.target_symbol_id, e.kind.clone(), e.edge_type.clone())
        });

        GraphBuildResult {
            edges: unique_edges,
            metrics,
            diagnostics,
        }
    }
}
