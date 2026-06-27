use std::sync::Arc;
use graph::models::{RelationshipNode, ResolutionEvidence, ResolutionState};
use crate::resolver::index::SymbolIndex;
use crate::semantic::types::FileSemantics;

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
    /// Set to true by a stage to stop the pipeline early.
    pub resolved: bool,
    /// Per-file semantic facts: variable types, field types, return types, aliases.
    /// Replaces the old `file_var_map` with a richer structure that supports
    /// type annotations, return-type propagation, and alias chains.
    pub file_semantics: FileSemantics,
}

impl ResolutionContext {
    pub fn new(
        rel_id: i64,
        source_file_id: i64,
        relationship: RelationshipNode,
        symbol_index: Arc<SymbolIndex>,
        file_semantics: FileSemantics,
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
            file_semantics,
        }
    }
}
