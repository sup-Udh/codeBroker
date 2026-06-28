use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::ResolutionContext;
use graph::models::{ResolutionEvidence, ResolutionState};

pub struct RankingStage;

impl ResolutionStage for RankingStage {
    fn name(&self) -> &'static str {
        "RankingStage"
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        // Pipeline short-circuits before this stage if already resolved
        if context.resolved {
            return Ok(());
        }

        if context.candidates.is_empty() {
            let kind = context.ir.node.kind.as_deref().unwrap_or("imports");
            context.final_state = if matches!(
                kind,
                "method_call" | "MEMBER_ACCESS" | "annotation" | "generic_constraint"
            ) {
                ResolutionState::Dynamic
            } else {
                ResolutionState::Missing
            };
        } else if context.candidates.len() == 1 {
            context.final_state = context.candidates[0].state;
            // Use evidence already pushed by an earlier stage if present
            if context.evidence.is_none() {
                context.evidence = Some(ResolutionEvidence::LexicalScopeMatch); context.emit("Stage", "Resolved", Some(ResolutionEvidence::LexicalScopeMatch));
            }
        } else {
            context.candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

            if context.candidates[0].score == context.candidates[1].score {
                context.final_state = ResolutionState::Ambiguous;
            } else {
                context.final_state = context.candidates[0].state;
                if context.evidence.is_none() {
                    context.evidence = Some(ResolutionEvidence::LexicalScopeMatch); context.emit("Stage", "Resolved", Some(ResolutionEvidence::LexicalScopeMatch));
                }
            }
        }
        Ok(())
    }
}
