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

/// Build a per-file `FileSemantics` by combining:
/// 1. Semantic bindings stored in the `semantic_bindings` DB table (type
///    annotations, return types, field types, aliases — written at parse time).
/// 2. Constructor bindings derived from `new_call`/`instantiates` relationships
///    with a source variable name (e.g. `const db = new Database()`).
/// 3. Alias propagation: if `x = y` and y's type is known, x inherits it.
/// 4. Return-type propagation: if `const x = f()` and f has a known return
///    type, x inherits that type.
fn build_file_semantics(
    db: &Database,
    relationships: &[(i64, i64, String, Option<String>, Option<String>, i64)],
) -> HashMap<i64, FileSemantics> {
    let mut file_semantics: HashMap<i64, FileSemantics> = HashMap::new();

    // ── Step 1: semantic_bindings table ──────────────────────────────────────
    let all_bindings = db.get_all_semantic_bindings().unwrap_or_default();
    for (file_id, binding) in all_bindings {
        let fs = file_semantics.entry(file_id).or_default();
        match binding.kind {
            SemanticBindingKind::VarType => {
                fs.var_types.entry(binding.name).or_insert_with(|| TypeBound {
                    type_name: binding.type_name,
                    evidence: SemanticEvidence::Annotation,
                    confidence: ResolutionConfidence::Certain,
                });
            }
            SemanticBindingKind::ReturnType => {
                fs.return_types.entry(binding.name).or_insert(binding.type_name);
            }
            SemanticBindingKind::FieldType => {
                fs.field_types.entry(binding.name).or_insert(binding.type_name);
            }
            SemanticBindingKind::Alias => {
                fs.aliases.entry(binding.name).or_insert(binding.type_name);
            }
        }
    }

    // ── Step 2: constructor bindings from relationships ───────────────────────
    // new_call/instantiates with source set: source_var → constructor_type
    for (_, file_id, name, source, kind, _) in relationships {
        let k = kind.as_deref().unwrap_or("imports");
        if k == "new_call" || k == "instantiates" {
            if let Some(var_name) = source {
                let fs = file_semantics.entry(*file_id).or_default();
                fs.var_types.entry(var_name.clone()).or_insert_with(|| TypeBound {
                    type_name: name.clone(),
                    evidence: SemanticEvidence::Constructor,
                    confidence: ResolutionConfidence::High,
                });
            }
        }
    }

    // ── Step 3: alias propagation (up to 5 hops) ─────────────────────────────
    let file_ids: Vec<i64> = file_semantics.keys().cloned().collect();
    for file_id in &file_ids {
        for _ in 0..5 {
            let mut new_bindings: Vec<(String, TypeBound)> = Vec::new();
            if let Some(fs) = file_semantics.get(file_id) {
                for (alias_name, source_name) in &fs.aliases {
                    if !fs.var_types.contains_key(alias_name) {
                        if let Some(bound) = fs.var_types.get(source_name.as_str()) {
                            new_bindings.push((alias_name.clone(), TypeBound {
                                type_name: bound.type_name.clone(),
                                evidence: SemanticEvidence::Alias,
                                confidence: ResolutionConfidence::Medium,
                            }));
                        }
                    }
                }
            }
            if new_bindings.is_empty() {
                break;
            }
            if let Some(fs) = file_semantics.get_mut(file_id) {
                for (name, bound) in new_bindings {
                    fs.var_types.entry(name).or_insert(bound);
                }
            }
        }
    }

    // ── Step 4: return-type propagation ──────────────────────────────────────
    // calls relationships with source set: if f() has known return type R,
    // and `const x = f()` was captured as new_call with source=x, or as a
    // calls with source=x, then x → R.
    for _ in 0..2 {
        let mut new_var_types: Vec<(i64, String, TypeBound)> = Vec::new();
        for (_, file_id, name, source, kind, _) in relationships {
            let k = kind.as_deref().unwrap_or("imports");
            if k == "calls" || k == "new_call" {
                if let Some(var_name) = source {
                    if let Some(fs) = file_semantics.get(file_id) {
                        if !fs.var_types.contains_key(var_name.as_str()) {
                            if let Some(ret_type) = fs.return_types.get(name.as_str()) {
                                new_var_types.push((*file_id, var_name.clone(), TypeBound {
                                    type_name: ret_type.clone(),
                                    evidence: SemanticEvidence::TypePropagation,
                                    confidence: ResolutionConfidence::Medium,
                                }));
                            }
                        }
                    }
                }
            }
        }
        if new_var_types.is_empty() {
            break;
        }
        for (fid, var_name, bound) in new_var_types {
            file_semantics.entry(fid).or_default()
                .var_types.entry(var_name).or_insert(bound);
        }
    }

    file_semantics
}

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

    // 2. Build FileSemantics per file (replaces the old file_var_maps pre-pass)
    let file_semantics_map = build_file_semantics(db, &relationships);

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

        let file_semantics = file_semantics_map
            .get(source_file_id)
            .cloned()
            .unwrap_or_default();

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
            file_semantics,
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
