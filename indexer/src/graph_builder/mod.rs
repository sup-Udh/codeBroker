use std::collections::HashSet;
use crate::ir::ResolvedRelationshipIR;
use crate::pipeline::PipelineStage;
use storage::Database;

#[derive(Debug, Default)]
pub struct GraphBuilderMetrics {
    pub relationships_received: usize,
    pub relationships_resolved: usize,
    pub relationships_skipped: usize,
    pub repository_relationships: usize,
    pub external_relationships: usize,
    pub builtin_relationships: usize,
    pub dynamic_relationships: usize,
    pub missing_relationships: usize,
    pub ambiguous_relationships: usize,
    pub edges_emitted: usize,
    pub edges_deduplicated: usize,
    pub insertion_failures: usize,
    pub recursion_skipped: usize,
    pub orphan_relationships: usize,
}

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct ResolvedEdge {
    pub source_file_id: i64,
    pub source_symbol_id: Option<i64>,
    pub target_symbol_id: i64,
    pub edge_kind: String,
}

pub struct EdgeAccumulator {
    edges: HashSet<ResolvedEdge>,
}

impl EdgeAccumulator {
    pub fn new() -> Self {
        Self {
            edges: HashSet::new(),
        }
    }

    pub fn insert(&mut self, edge: ResolvedEdge) -> bool {
        self.edges.insert(edge)
    }

    pub fn into_inner(self) -> HashSet<ResolvedEdge> {
        self.edges
    }
}

pub struct GraphBuilder<'a> {
    pub db: &'a Database,
}

impl<'a> GraphBuilder<'a> {
    pub fn new(db: &'a Database) -> Self {
        Self { db }
    }
}

impl<'a> PipelineStage for GraphBuilder<'a> {
    type Input = Vec<ResolvedRelationshipIR>;
    type Output = GraphBuilderMetrics;

    fn execute(&self, input: Self::Input) -> Result<Self::Output, String> {
        let mut metrics = GraphBuilderMetrics::default();
        let mut accumulator = EdgeAccumulator::new();

        metrics.relationships_received = input.len();

        for resolved in input {
            use graph::models::ResolutionState::*;
            match resolved.state {
                RepositorySymbol => {
                    metrics.repository_relationships += 1;
                    metrics.relationships_resolved += 1;
                }
                ExternalDependency | WorkspaceModule => {
                    metrics.external_relationships += 1;
                    metrics.relationships_resolved += 1;
                }
                StandardLibrary | Builtin => {
                    metrics.builtin_relationships += 1;
                    metrics.relationships_resolved += 1;
                }
                Dynamic => {
                    metrics.dynamic_relationships += 1;
                    metrics.relationships_resolved += 1;
                }
                Missing => {
                    metrics.missing_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                Ambiguous => {
                    metrics.ambiguous_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
                Recursive => {
                    metrics.recursion_skipped += 1;
                    metrics.relationships_skipped += 1;
                }
                Unknown => {
                    metrics.orphan_relationships += 1;
                    metrics.relationships_skipped += 1;
                }
            }

            if resolved.state == RepositorySymbol && !resolved.target_symbol_ids.is_empty() {
                let target_id = resolved.target_symbol_ids[0];
                let source_file_id = resolved.original.source_file_id;
                let edge_kind = resolved.original.node.kind.clone().unwrap_or_else(|| "imports".to_string());
                
                let src_sym = resolved.original.enclosing_symbol_id;
                
                if Some(target_id) == src_sym {
                    // This should have been caught by Resolver and marked Recursive, but just in case
                    metrics.recursion_skipped += 1;
                } else {
                    let edge = ResolvedEdge {
                        source_file_id,
                        source_symbol_id: src_sym,
                        target_symbol_id: target_id,
                        edge_kind,
                    };
                    if accumulator.insert(edge) {
                        // will be inserted later
                    } else {
                        metrics.edges_deduplicated += 1;
                    }
                }
            }
        }

        // Batch insert unique edges
        for edge in accumulator.into_inner() {
            if let Err(_) = self.db.insert_edge_attributed(
                edge.source_file_id,
                edge.source_symbol_id,
                edge.target_symbol_id,
                &edge.edge_kind,
            ) {
                metrics.insertion_failures += 1;
            } else {
                metrics.edges_emitted += 1;
            }
        }

        Ok(metrics)
    }
}
