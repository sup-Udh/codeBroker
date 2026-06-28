use std::collections::HashSet;
use crate::graph_builder::types::GraphEdgeIR;

pub struct EdgeAccumulator {
    edges: HashSet<GraphEdgeIR>,
}

impl EdgeAccumulator {
    pub fn new() -> Self {
        Self {
            edges: HashSet::new(),
        }
    }

    /// Inserts a new graph edge. Returns true if the edge was not previously in the accumulator.
    pub fn insert(&mut self, edge: GraphEdgeIR) -> bool {
        self.edges.insert(edge)
    }

    pub fn into_inner(self) -> HashSet<GraphEdgeIR> {
        self.edges
    }
    
    pub fn len(&self) -> usize {
        self.edges.len()
    }
}
