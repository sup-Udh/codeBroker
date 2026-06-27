use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::{ResolutionContext, ResolutionCandidate};
use graph::models::ResolutionState;

pub struct LexicalGenerationStage;

impl ResolutionStage for LexicalGenerationStage {
    fn name(&self) -> &'static str {
        "LexicalGenerationStage"
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        let name = &context.relationship.name;
        if let Some(ids) = context.symbol_index.find_by_name(name) {
            for &id in ids {
                context.candidates.push(ResolutionCandidate {
                    symbol_id: id,
                    score: 0.0,
                    state: ResolutionState::RepositorySymbol,
                });
            }
        }
        Ok(())
    }
}
