use crate::resolver::context::ResolutionContext;
use crate::resolver::stages::ResolutionStage;

pub struct ResolutionPipeline {
    stages: Vec<Box<dyn ResolutionStage>>,
}

impl ResolutionPipeline {
    pub fn new(stages: Vec<Box<dyn ResolutionStage>>) -> Self {
        Self { stages }
    }

    pub fn execute(&self, mut context: ResolutionContext) -> Result<ResolutionContext, String> {
        for stage in &self.stages {
            if context.resolved {
                break;
            }
            stage.execute(&mut context)?;
        }
        
        // Ensure we never return without a trace for objective 3.
        if !context.resolved {
            if context.final_state == graph::models::ResolutionState::Unknown || context.final_state == graph::models::ResolutionState::Missing {
                if context.candidates.is_empty() {
                    context.final_state = graph::models::ResolutionState::Missing;
                    context.evidence = Some(graph::models::ResolutionEvidence::MissingImport);
                } else {
                    context.final_state = graph::models::ResolutionState::Ambiguous;
                    context.evidence = Some(graph::models::ResolutionEvidence::AmbiguousCandidates);
                }
            } else if context.final_state == graph::models::ResolutionState::Dynamic && context.evidence.is_none() {
                context.evidence = Some(graph::models::ResolutionEvidence::DynamicDispatch);
            }
        }
        
        Ok(context)
    }
}
