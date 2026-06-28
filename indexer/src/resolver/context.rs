use std::sync::Arc;
use graph::models::{ResolutionEvidence, ResolutionState};
use crate::resolver::index::SymbolIndex;
use crate::flow::VariableFlowEngine;
use crate::ir::RelationshipIR;
use crate::resolver::events::ResolutionTrace;

#[derive(Debug, Clone)]
pub struct ResolutionCandidate {
    pub symbol_id: i64,
    pub score: f64,
    pub state: ResolutionState,
}

pub struct ResolutionContext {
    pub ir: RelationshipIR,
    pub candidates: Vec<ResolutionCandidate>,
    pub symbol_index: Arc<SymbolIndex>,
    pub evidence: Option<ResolutionEvidence>,
    pub final_state: ResolutionState,
    pub resolved: bool,
    pub flow_engine: Arc<VariableFlowEngine>,
    pub trace: Option<ResolutionTrace>,
}

impl ResolutionContext {
    pub fn new(
        ir: RelationshipIR,
        symbol_index: Arc<SymbolIndex>,
        flow_engine: Arc<VariableFlowEngine>,
        enable_tracing: bool,
    ) -> Self {
        Self {
            ir,
            candidates: Vec::new(),
            symbol_index,
            evidence: None,
            final_state: ResolutionState::Unknown,
            resolved: false,
            flow_engine,
            trace: if enable_tracing { Some(ResolutionTrace::new()) } else { None },
        }
    }

    pub fn emit(&mut self, stage: &str, status: &str, evidence: Option<ResolutionEvidence>) {
        if let Some(ref mut t) = self.trace {
            t.emit(stage, status, evidence);
        }
    }
}
