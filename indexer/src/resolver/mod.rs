pub mod index;
pub mod context;
pub mod pipeline;
pub mod stages;
pub mod decisions;
pub mod assertions;

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
use crate::ir::{RelationshipIR, ResolvedRelationshipIR};
use crate::pipeline::PipelineStage;
use crate::graph_builder::{GraphBuilder, StorageWriter, GraphBuilderMetrics};

pub struct Resolver<'a> {
    pub db: &'a Database,
    pub enable_tracing: bool,
}

impl<'a> Resolver<'a> {
    pub fn new(db: &'a Database, enable_tracing: bool) -> Self {
        Self { db, enable_tracing }
    }
}

impl<'a> PipelineStage for Resolver<'a> {
    type Input = Vec<RelationshipIR>;
    type Output = Vec<ResolvedRelationshipIR>;

    fn execute(&self, input: Self::Input) -> Result<Self::Output, String> {
        let symbol_index = Arc::new(SymbolIndex::build(self.db)?);
        let flow_engine = Arc::new(VariableFlowEngine::new(self.db));

        let pipeline = ResolutionPipeline::new(vec![
            Box::new(stages::classification::ClassificationStage),
            Box::new(stages::receiver::ReceiverResolutionStage),
            Box::new(stages::generation::LexicalGenerationStage),
            Box::new(stages::filtering::ScopeFilterStage),
            Box::new(stages::filtering::ModuleFilterStage),
            Box::new(stages::ranking::RankingStage),
        ]);

        let mut output = Vec::with_capacity(input.len());

        for ir in input {
            let context = ResolutionContext::new(
                ir.clone(),
                Arc::clone(&symbol_index),
                Arc::clone(&flow_engine),
                self.enable_tracing,
            );

            let resolved_context = pipeline.execute(context)?;

            // For diagnostics inspect, we might need to print the trace here or return it
            // We now return decisions in ResolvedRelationshipIR for the caller to format.
            
            // Only update the evidence enum string in db
            let evidence_str = resolved_context.evidence.map(|e| e.as_str().to_string());
            let _ = self.db.update_relationship_state(
                ir.id, 
                resolved_context.final_state.as_str(), 
                1.0, 
                evidence_str.as_deref()
            );

            let target_ids = resolved_context.candidates.into_iter().map(|c| c.symbol_id).collect();

            output.push(ResolvedRelationshipIR {
                original: ir,
                state: resolved_context.final_state,
                target_symbol_ids: target_ids,
                confidence: 1.0,
                decisions: resolved_context.decisions,
            });
        }

        Ok(output)
    }
}

pub fn run_resolver(
    db: &Database,
    restrict_to_files: Option<&[i64]>,
) -> Result<Vec<ResolvedRelationshipIR>, String> {
    let raw_relationships = db
        .get_all_relationships_with_lines()
        .map_err(|e| e.to_string())?;
        
    let mut input_ir = Vec::new();
    for (rel_id, source_file_id, import_name, import_source, import_kind, line_number) in raw_relationships {
        if let Some(files) = restrict_to_files {
            if !files.contains(&source_file_id) {
                // If it's not a touched file, we still need to process it if it might be a consumer
                // of the touched file. For now, since the previous code didn't actually filter,
                // we'll just not `continue` to preserve correctness.
                // TODO: Optimize to only re-resolve consumers of touched files.
            }
        }
        let src_sym = db.enclosing_symbol_id(source_file_id, line_number).unwrap_or(None);
        input_ir.push(RelationshipIR {
            id: rel_id,
            source_file_id,
            node: graph::models::RelationshipNode {
                name: import_name,
                source: import_source,
                line_number: line_number as usize,
                kind: import_kind,
            },
            enclosing_symbol_id: src_sym,
        });
    }

    let resolver = Resolver::new(db, false);
    resolver.execute(input_ir)
}

pub fn resolve_relationships(
    db: &Database,
    restrict_to_files: Option<&[i64]>,
) -> Result<GraphBuilderMetrics, String> {
    let resolved_ir = run_resolver(db, restrict_to_files)?;
    
    let build_result = GraphBuilder::build(resolved_ir);
    let metrics = StorageWriter::flush_edges(db, build_result)?;

    Ok(metrics)
}
