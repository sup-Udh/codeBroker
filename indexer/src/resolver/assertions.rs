use crate::resolver::context::ResolutionContext;
use graph::models::{ResolutionEvidence, ResolutionState};
use crate::resolver::decisions::DecisionReason;

pub struct PipelineAssertions;

impl PipelineAssertions {
    pub fn assert_terminal_state(context: &ResolutionContext) {
        if !context.resolved {
            return;
        }

        match &context.final_state {
            ResolutionState::RepositorySymbol => {
                debug_assert!(
                    context.evidence.is_some(),
                    "RepositorySymbol must have evidence"
                );
            }
            ResolutionState::Dynamic | ResolutionState::Missing => {
                debug_assert!(
                    context.evidence.is_some(),
                    "{:?} state must have evidence", context.final_state
                );
                let has_reason = context.decisions.iter().any(|d| d.reason.is_some());
                debug_assert!(
                    has_reason,
                    "{:?} state must have a DecisionReason", context.final_state
                );
            }
            ResolutionState::Unknown => {
                panic!("Relationship cannot finish in Unknown state");
            }
            ResolutionState::Ambiguous => {
                debug_assert!(
                    context.evidence.is_some(),
                    "Ambiguous state must have evidence"
                );
                debug_assert!(
                    context.candidates.len() > 1,
                    "Ambiguous state must have multiple candidates"
                );
            }
            ResolutionState::Builtin => {
                debug_assert!(
                    context.evidence == Some(ResolutionEvidence::BuiltinClassification),
                    "Builtin must have BuiltinClassification evidence"
                );
            }
            ResolutionState::ExternalDependency => {
                debug_assert!(
                    context.evidence == Some(ResolutionEvidence::ExternalDependency),
                    "ExternalDependency must have ExternalDependency evidence"
                );
            }
            ResolutionState::StandardLibrary => {
                debug_assert!(
                    context.evidence == Some(ResolutionEvidence::BuiltinClassification),
                    "StandardLibrary must have BuiltinClassification evidence"
                );
            }
            _ => {}
        }
    }

    pub fn assert_pipeline_complete(context: &ResolutionContext, expected_stage_count: usize) {
        if context.enable_tracing {
            debug_assert_eq!(
                context.decisions.len(),
                expected_stage_count,
                "Pipeline did not execute or skip all stages correctly"
            );
        }
    }
}
