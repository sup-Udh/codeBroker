use std::sync::Arc;
use graph::models::{RelationshipNode, ResolutionEvidence, ResolutionState};
use crate::resolver::index::SymbolIndex;
use crate::flow::VariableFlowEngine;

#[derive(Debug, Clone)]
pub struct ResolutionCandidate {
    pub symbol_id: i64,
    pub score: f64,
    pub state: ResolutionState,
}

pub struct ResolutionContext {
    pub relationship: RelationshipNode,
    pub rel_id: i64,
    pub source_file_id: i64,
    pub candidates: Vec<ResolutionCandidate>,
    pub symbol_index: Arc<SymbolIndex>,
    pub evidence: Vec<ResolutionEvidence>,
    pub final_state: ResolutionState,
    pub resolved: bool,
    pub flow_engine: Arc<VariableFlowEngine>,
}

impl ResolutionContext {
    pub fn new(
        rel_id: i64,
        source_file_id: i64,
        relationship: RelationshipNode,
        symbol_index: Arc<SymbolIndex>,
        flow_engine: Arc<VariableFlowEngine>,
    ) -> Self {
        Self {
            relationship,
            rel_id,
            source_file_id,
            candidates: Vec::new(),
            symbol_index,
            evidence: Vec::new(),
            final_state: ResolutionState::Unknown,
            resolved: false,
            flow_engine,
        }
    }
}
