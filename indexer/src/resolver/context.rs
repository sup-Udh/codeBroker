use std::sync::Arc;
use graph::models::{ResolutionEvidence, ResolutionState};
use crate::resolver::index::SymbolIndex;
use crate::flow::VariableFlowEngine;
use crate::ir::RelationshipIR;
use crate::resolver::decisions::{StageDecision, PipelineStageType, StageStatus, DecisionReason};
use crate::resolver::type_graph::TypeGraph;
use crate::resolver::import_resolver::ImportResolver;
use crate::resolver::type_resolver::TypeResolver;

#[derive(Clone)]
pub struct ResolverContext {
    pub symbol_index: Arc<SymbolIndex>,
    pub type_graph: Arc<TypeGraph>,
    pub import_resolver: Arc<ImportResolver>,
    pub flow_engine: Arc<VariableFlowEngine>,
}

#[derive(Debug, Clone)]
pub struct ResolutionCandidate {
    pub symbol_id: i64,
    pub score: f64,
    pub state: ResolutionState,
}

pub struct ResolutionContext {
    pub ir: RelationshipIR,
    pub candidates: Vec<ResolutionCandidate>,
    pub ctx: Arc<ResolverContext>,
    pub evidence: Option<ResolutionEvidence>,
    pub final_state: ResolutionState,
    pub resolved: bool,
    pub decisions: Vec<StageDecision>,
    pub enable_tracing: bool,
}

impl ResolutionContext {
    pub fn new(
        ir: RelationshipIR,
        ctx: Arc<ResolverContext>,
        enable_tracing: bool,
    ) -> Self {
        Self {
            ir,
            candidates: Vec::new(),
            ctx,
            evidence: None,
            final_state: ResolutionState::Unknown,
            resolved: false,
            decisions: Vec::new(),
            enable_tracing,
        }
    }
    
    pub fn type_resolver(&self) -> TypeResolver<'_> {
        TypeResolver::new(
            &self.ctx.symbol_index,
            &self.ctx.type_graph,
            &self.ctx.import_resolver,
            &self.ctx.flow_engine,
        )
    }

    pub fn emit_decision(&mut self, stage: PipelineStageType, status: StageStatus, reason: Option<DecisionReason>, notes: Option<String>, candidates_before: Vec<i64>) {
        if self.enable_tracing {
            let candidates_after = self.candidates.iter().map(|c| c.symbol_id).collect();
            
            self.decisions.push(StageDecision {
                stage,
                status,
                reason,
                candidates_before,
                candidates_after,
                notes,
            });
        }
    }

    pub fn resolve_with(&mut self, stage: PipelineStageType, state: ResolutionState, reason: DecisionReason, notes: Option<String>) {
        if !self.resolved {
            let candidates_before = self.candidates.iter().map(|c| c.symbol_id).collect();
            self.final_state = state;
            self.evidence = Some(reason.to_evidence());
            self.resolved = true;
            self.emit_decision(stage, StageStatus::Success, Some(reason), notes, candidates_before);
        }
    }

    pub fn fail_stage(&mut self, stage: PipelineStageType, reason: DecisionReason, notes: Option<String>) {
        let candidates_before = self.candidates.iter().map(|c| c.symbol_id).collect();
        self.emit_decision(stage, StageStatus::Failed, Some(reason), notes, candidates_before);
    }
    
    pub fn skip_stage(&mut self, stage: PipelineStageType) {
        let candidates_before = self.candidates.iter().map(|c| c.symbol_id).collect();
        self.emit_decision(stage, StageStatus::NotApplicable, None, None, candidates_before);
    }

    pub fn add_candidates(&mut self, stage: PipelineStageType, new_candidates: Vec<ResolutionCandidate>, reason: Option<DecisionReason>, notes: Option<String>) {
        let candidates_before = self.candidates.iter().map(|c| c.symbol_id).collect();
        self.candidates = new_candidates;
        self.emit_decision(stage, StageStatus::Success, reason, notes, candidates_before);
    }
}
