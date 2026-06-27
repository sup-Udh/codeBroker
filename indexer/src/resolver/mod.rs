pub mod index;
pub mod context;
pub mod pipeline;
pub mod stages;

pub use pipeline::ResolutionPipeline;
pub use index::SymbolIndex;
pub use context::{ResolutionContext, ResolutionCandidate};
pub use stages::ResolutionStage;

use std::collections::HashMap;
use std::sync::Arc;
use storage::Database;
use graph::models::{ResolutionState, SemanticBindingKind};
use crate::semantic::{
    FileSemantics, TypeBound,
    evidence::{ResolutionConfidence, SemanticEvidence},
};
use crate::flow::VariableFlowEngine;

pub fn resolve_relationships(
    db: &Database,
    restrict_to_files: Option<&[i64]>,
) -> Result<(usize, usize), String> {
    let relationships = db
        .get_all_relationships_with_lines()
        .map_err(|e| e.to_string())?;

    let total_relationships = relationships.len();
    let mut edges_created = 0;

    // 1. Build Symbol Index
    let symbol_index = Arc::new(SymbolIndex::build(db)?);

    // 2. Build Variable Flow Engine (replaces the old file_var_maps pre-pass)
    let flow_engine = Arc::new(VariableFlowEngine::new(db));

    // 3. Build Pipeline
    let pipeline = ResolutionPipeline::new(vec![
        Box::new(stages::classification::ClassificationStage),
        Box::new(stages::receiver::ReceiverResolutionStage),
        Box::new(stages::generation::LexicalGenerationStage),
        Box::new(stages::filtering::ScopeFilterStage),
        Box::new(stages::filtering::ModuleFilterStage),
        Box::new(stages::ranking::RankingStage),
    ]);

    // 4. Process each relationship
    for (rel_id, source_file_id, import_name, import_source, import_kind, line_number) in &relationships {
        if let Some(files) = restrict_to_files {
            if !files.contains(source_file_id) {
                let _ = files; // incremental: re-link touched files (caller decides scope)
            }
        }

        let edge_kind = import_kind.clone().unwrap_or_else(|| "imports".to_string());

        let src_sym = db.enclosing_symbol_id(*source_file_id, *line_number).unwrap_or(None);

        let context = ResolutionContext::new(
            *rel_id,
            *source_file_id,
            graph::models::RelationshipNode {
                name: import_name.clone(),
                source: import_source.clone(),
                line_number: *line_number as usize,
                kind: import_kind.clone(),
            },
            Arc::clone(&symbol_index),
            Arc::clone(&flow_engine),
        );

        let resolved_context = pipeline.execute(context)?;

        let final_state = resolved_context.final_state;
        let evidence = resolved_context.evidence.first().cloned();

        if final_state == ResolutionState::RepositorySymbol
            && !resolved_context.candidates.is_empty()
        {
            let target_id = resolved_context.candidates[0].symbol_id;
            if Some(target_id) != src_sym {
                let _ = db.insert_edge_attributed(*source_file_id, src_sym, target_id, &edge_kind);
                edges_created += 1;
            }
        }

        let _ = db.update_relationship_state(*rel_id, final_state.as_str(), 1.0, evidence);
    }

    Ok((edges_created, total_relationships))
}
