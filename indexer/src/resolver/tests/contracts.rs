use crate::resolver::context::{ResolutionContext, ResolutionCandidate, ResolverContext};
use crate::resolver::assertions::PipelineAssertions;
use graph::models::{ResolutionState, ResolutionEvidence};
use crate::ir::{RelationshipIR, SymbolNode};
use crate::resolver::index::SymbolIndex;
use crate::resolver::type_graph::TypeGraph;
use crate::resolver::import_resolver::ImportResolver;
use crate::flow::VariableFlowEngine;
use std::sync::Arc;
use crate::resolver::decisions::{PipelineStageType, DecisionReason, StageStatus};

fn dummy_context() -> ResolutionContext {
    let ir = RelationshipIR {
        source_file_id: 1,
        source_symbol_id: None,
        node: SymbolNode {
            name: "test".to_string(),
            kind: Some("imports".to_string()),
            source: Some("std::test".to_string()),
            location: None,
            is_exported: false,
        },
    };
    // Mock db isn't easy here, let's just make empty ones if we can, or we need empty constructors.
    // Assuming we can create empty ones. SymbolIndex doesn't have `new()`, but we can add one.
    // The previous code had `SymbolIndex::new()` so it must exist.
    let ctx = Arc::new(ResolverContext {
        symbol_index: Arc::new(SymbolIndex::new()),
        type_graph: Arc::new(TypeGraph::new()),
        import_resolver: Arc::new(ImportResolver::new()),
        flow_engine: Arc::new(VariableFlowEngine::new()),
    });
    
    ResolutionContext::new(
        ir,
        ctx,
        true
    )
}

#[test]
fn test_repository_symbol_must_have_evidence() {
    let mut ctx = dummy_context();
    // Resolve with reason, which provides evidence
    ctx.resolve_with(
        PipelineStageType::Ranking,
        ResolutionState::RepositorySymbol,
        DecisionReason::VariableAssignment,
        None
    );
    // Erase the evidence to trigger assertion
    ctx.evidence = None;
    
    let result = std::panic::catch_unwind(|| {
        PipelineAssertions::assert_terminal_state(&ctx);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if RepositorySymbol lacks evidence");
}

#[test]
fn test_dynamic_must_have_evidence() {
    let mut ctx = dummy_context();
    ctx.resolve_with(
        PipelineStageType::Ranking,
        ResolutionState::Dynamic,
        DecisionReason::DynamicDispatch,
        None
    );
    ctx.evidence = None;
    
    let result = std::panic::catch_unwind(|| {
        PipelineAssertions::assert_terminal_state(&ctx);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if Dynamic lacks evidence");
}

#[test]
fn test_missing_must_have_evidence() {
    let mut ctx = dummy_context();
    ctx.resolve_with(
        PipelineStageType::Ranking,
        ResolutionState::Missing,
        DecisionReason::MissingImport,
        None
    );
    ctx.evidence = None;
    
    let result = std::panic::catch_unwind(|| {
        PipelineAssertions::assert_terminal_state(&ctx);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if Missing lacks evidence");
}

#[test]
fn test_ambiguous_must_have_multiple_candidates() {
    let mut ctx = dummy_context();
    // Resolve with ambiguous, but no multiple candidates added
    ctx.resolve_with(
        PipelineStageType::Ranking,
        ResolutionState::Ambiguous,
        DecisionReason::MultipleCandidates,
        None
    );
    
    let result = std::panic::catch_unwind(|| {
        PipelineAssertions::assert_terminal_state(&ctx);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if Ambiguous lacks multiple candidates");
}

#[test]
fn test_builtin_must_have_classification_evidence() {
    let mut ctx = dummy_context();
    ctx.resolve_with(
        PipelineStageType::Classification,
        ResolutionState::Builtin,
        DecisionReason::VariableAssignment, // Wrong! Maps to VariableAssignment evidence
        None
    );
    
    let result = std::panic::catch_unwind(|| {
        PipelineAssertions::assert_terminal_state(&ctx);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if Builtin lacks ClassificationMatch evidence");
}

#[test]
fn test_pipeline_completion_length() {
    let mut ctx = dummy_context();
    ctx.skip_stage(PipelineStageType::Classification);
    
    let result = std::panic::catch_unwind(|| {
        // Asserting 2 stages, but we only ran 1
        PipelineAssertions::assert_pipeline_complete(&ctx, 2);
    });
    assert!(result.is_err(), "PipelineAssertions should panic if stage count mismatches");
}
