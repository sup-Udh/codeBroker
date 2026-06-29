use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::{ResolutionContext, ResolutionCandidate};
use graph::models::ResolutionState;
use crate::resolver::decisions::{PipelineStageType, DecisionReason};

pub struct LexicalGenerationStage;

impl ResolutionStage for LexicalGenerationStage {
    fn name(&self) -> &'static str {
        "LexicalGenerationStage"
    }

    fn stage_type(&self) -> PipelineStageType {
        PipelineStageType::LexicalGeneration
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        let name = &context.ir.node.name;
        if let Some(ids) = context.ctx.symbol_index.find_by_name(name) {
            let mut candidates = Vec::new();
            for &id in ids {
                candidates.push(ResolutionCandidate {
                    symbol_id: id,
                    score: 0.0,
                    state: ResolutionState::RepositorySymbol,
                });
            }
            if candidates.is_empty() {
                context.fail_stage(self.stage_type(), DecisionReason::NoCandidatesGenerated, None);
            } else {
                context.add_candidates(self.stage_type(), candidates, Some(DecisionReason::RepositoryMatch), None);
            }
        } else {
            context.fail_stage(self.stage_type(), DecisionReason::NoCandidatesGenerated, None);
        }
        Ok(())
    }
}
