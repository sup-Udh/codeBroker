use graph::models::{RelationshipNode, ResolutionState};
use indexer::graph_builder::{GraphBuilder, GraphEdgeIR};
use indexer::ir::{RelationshipIR, ResolvedRelationshipIR};

fn mock_rel(id: i64, state: ResolutionState, targets: Vec<i64>) -> ResolvedRelationshipIR {
    ResolvedRelationshipIR {
        original: RelationshipIR {
            id,
            source_file_id: 1,
            node: RelationshipNode {
                name: "test".to_string(),
                source: Some("test".to_string()),
                line_number: 1,
                kind: Some("imports".to_string()),
            },
            enclosing_symbol_id: Some(10),
        },
        state,
        target_symbol_ids: targets,
        confidence: 1.0,
        decisions: vec![],
    }
}

#[test]
fn test_duplicate_relationships_collapse() {
    let rels = vec![
        mock_rel(1, ResolutionState::RepositorySymbol, vec![100]),
        mock_rel(2, ResolutionState::RepositorySymbol, vec![100]),
    ];

    let result = GraphBuilder::build(rels);
    assert_eq!(result.edges.len(), 1, "Two identical relationships must collapse into one edge");
    assert_eq!(result.metrics.relationships_input, 2);
    assert_eq!(result.metrics.relationships_emitted, 1);
    assert_eq!(result.metrics.duplicates_collapsed, 1);
}

#[test]
fn test_recursive_function_self_loop() {
    let mut rel = mock_rel(1, ResolutionState::RepositorySymbol, vec![10]);
    rel.original.enclosing_symbol_id = Some(10); // same as target

    let result = GraphBuilder::build(vec![rel]);
    assert_eq!(result.edges.len(), 0, "Self-loops should not emit edges");
    assert_eq!(result.metrics.self_loops, 1);
}

#[test]
fn test_excluded_states_emit_zero_edges() {
    let rels = vec![
        mock_rel(1, ResolutionState::Dynamic, vec![]),
        mock_rel(2, ResolutionState::Missing, vec![]),
        mock_rel(3, ResolutionState::Builtin, vec![]),
        mock_rel(4, ResolutionState::ExternalDependency, vec![]),
        mock_rel(5, ResolutionState::Ambiguous, vec![]),
        mock_rel(6, ResolutionState::Recursive, vec![]),
    ];

    let result = GraphBuilder::build(rels);
    assert_eq!(result.edges.len(), 0, "Non-repository states must emit 0 edges");
    assert_eq!(result.metrics.relationships_skipped, 6);
}

#[test]
fn test_mixed_repository() {
    let rels = vec![
        mock_rel(1, ResolutionState::RepositorySymbol, vec![100]),
        mock_rel(2, ResolutionState::Dynamic, vec![]),
        mock_rel(3, ResolutionState::Builtin, vec![]),
        mock_rel(4, ResolutionState::RepositorySymbol, vec![200]),
    ];

    let result = GraphBuilder::build(rels);
    assert_eq!(result.edges.len(), 2, "Only repository symbols emit edges");
    assert_eq!(result.metrics.relationships_emitted, 2);
    assert_eq!(result.metrics.relationships_skipped, 2);
}

#[test]
fn test_incremental_reindex_determinism() {
    // If we run the builder twice, we should get exactly the same results
    let rels = vec![
        mock_rel(1, ResolutionState::RepositorySymbol, vec![100]),
        mock_rel(2, ResolutionState::RepositorySymbol, vec![200]),
    ];

    let result1 = GraphBuilder::build(rels.clone());
    let result2 = GraphBuilder::build(rels);

    assert_eq!(result1.edges.len(), result2.edges.len());
    assert_eq!(result1.edges[0].target_symbol_id, result2.edges[0].target_symbol_id);
    assert_eq!(result1.edges[1].target_symbol_id, result2.edges[1].target_symbol_id);
}
