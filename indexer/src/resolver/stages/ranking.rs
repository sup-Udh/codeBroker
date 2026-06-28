use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::ResolutionContext;
use graph::models::{ResolutionEvidence, ResolutionState};
use crate::resolver::decisions::{PipelineStageType, DecisionReason};

pub struct RankingStage;

impl ResolutionStage for RankingStage {
    fn name(&self) -> &'static str {
        "RankingStage"
    }

    fn stage_type(&self) -> PipelineStageType {
        PipelineStageType::Ranking
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        if context.candidates.is_empty() {
            let kind = context.ir.node.kind.as_deref().unwrap_or("imports");
            let state = if matches!(
                kind,
                "method_call" | "MEMBER_ACCESS" | "annotation" | "generic_constraint"
            ) {
                ResolutionState::Dynamic
            } else {
                ResolutionState::Missing
            };
            let reason = if state == ResolutionState::Dynamic {
                DecisionReason::DynamicDispatch
            } else {
                DecisionReason::NoCandidatesGenerated
            };
            context.resolve_with(self.stage_type(), state, reason, None);
            return Ok(());
        } else if context.candidates.len() == 1 {
            let state = context.candidates[0].state.clone();
            context.resolve_with(self.stage_type(), state, DecisionReason::LexicalScopeMatch, None);
            return Ok(());
        } else {
            context.candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

            if context.candidates[0].score == context.candidates[1].score {
                context.resolve_with(self.stage_type(), ResolutionState::Ambiguous, DecisionReason::MultipleCandidates, None);
                return Ok(());
            } else {
                let state = context.candidates[0].state.clone();
                context.resolve_with(self.stage_type(), state, DecisionReason::LexicalScopeMatch, None);
                return Ok(());
            }
        }
    }
}
