use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::{ResolutionCandidate, ResolutionContext};
use crate::resolver::stages::classification::JS_BUILTIN_RECEIVERS;
use graph::models::{ResolutionEvidence, ResolutionState};
use crate::resolver::decisions::{PipelineStageType, DecisionReason};

/// Resolves method calls using the file's semantic type map.
///
/// Handles three cases:
/// 1. Direct receiver: `db.query()` where source="db" → look up "db" in var_types
/// 2. This-chain: `this.db.query()` where source="this.db" → look up "db" in field_types
/// 3. Self-chain (Python): `self.db.query()` where source="self.db" → same as above
/// All of these use alias propagation and annotation precedence from FileSemantics.
/// Tokenizes a member access chain (e.g. `foo.bar.baz`) into its components.
pub struct ReceiverChain {
    pub tokens: Vec<String>,
}

impl ReceiverChain {
    pub fn parse(raw: &str) -> Self {
        let mut tokens: Vec<String> = raw.split('.').map(|s| s.to_string()).collect();
        // If the chain starts with `this` or `self`, we want to treat the whole thing
        // as a property access on `this`/`self`, but actually the FlowEngine already
        // strips `this.` or `self.` internally when resolving variables, so we
        // will keep them as tokens and handle it in the resolver.
        Self { tokens }
    }
}

pub struct MemberResolverStage;

impl ResolutionStage for MemberResolverStage {
    fn name(&self) -> &'static str {
        "MemberResolverStage"
    }

    fn stage_type(&self) -> PipelineStageType {
        PipelineStageType::ReceiverResolution
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        let kind = context.ir.node.kind.as_deref().unwrap_or("imports");

        if !matches!(kind, "method_call" | "MEMBER_ACCESS" | "instantiates") {
            context.skip_stage(self.stage_type());
            return Ok(());
        }

        let Some(receiver_raw) = context.ir.node.source.as_deref() else {
            context.fail_stage(self.stage_type(), DecisionReason::UnknownReceiver, None);
            return Ok(());
        };

        let method_name = context.ir.node.name.clone();
        let chain = ReceiverChain::parse(receiver_raw);

        // Bare `this.method()` / `self.method()` — a direct call on the
        // enclosing instance, as opposed to `this.field.method()` (handled
        // below via flow_engine) or `obj.method()` on some other variable.
        // There's no field to look up a type through, so resolve directly
        // against the enclosing class using SymbolIndex's containment-based
        // parent_map, the same one find_method_in_type already relies on.
        if receiver_raw == "this" || receiver_raw == "self" {
            let enclosing_class_name = context
                .ir
                .enclosing_symbol_id
                .and_then(|id| context.ctx.symbol_index.parent_map.get(&id).copied())
                .and_then(|parent_id| context.ctx.symbol_index.get_symbol(parent_id))
                .map(|s| s.name.clone());

            if let Some(class_name) = enclosing_class_name {
                let mut candidates = context.ctx.symbol_index.find_method_in_type(&method_name, &class_name);
                if candidates.is_empty() {
                    if let Some(type_ids) = context.ctx.symbol_index.find_by_name(&class_name) {
                        for &type_id in type_ids {
                            for ancestor_id in context.ctx.type_graph.get_ancestors(type_id) {
                                if let Some(ancestor) = context.ctx.symbol_index.get_symbol(ancestor_id) {
                                    candidates.extend(context.ctx.symbol_index.find_method_in_type(&method_name, &ancestor.name));
                                }
                            }
                        }
                    }
                }

                if !candidates.is_empty() {
                    let resolution_candidates = candidates
                        .into_iter()
                        .map(|id| ResolutionCandidate {
                            symbol_id: id,
                            score: 1.0,
                            state: ResolutionState::RepositorySymbol,
                        })
                        .collect();
                    context.add_candidates(
                        self.stage_type(),
                        resolution_candidates,
                        Some(DecisionReason::RepositoryMatch),
                        Some(format!("Enclosing class: {}", class_name)),
                    );
                    context.resolve_with(self.stage_type(), ResolutionState::RepositorySymbol, DecisionReason::RepositoryMatch, None);
                } else {
                    context.resolve_with(
                        self.stage_type(),
                        ResolutionState::Missing,
                        DecisionReason::MissingImport,
                        Some(format!("Method missing on enclosing class: {}", class_name)),
                    );
                }
                return Ok(());
            }
            // No enclosing class found (e.g. a bare `this` outside any class,
            // or the containment index missed it) — fall through to the
            // existing logic below, which fails with UnknownReceiverType.
        }

        let mut origin = crate::resolver::context::SymbolOrigin::Unknown;
        let mut type_name_to_use = None;
        let mut imported_source = None;
        let mut resolved_state = None;

        let type_name: Option<String> = if receiver_raw.starts_with("this.") || receiver_raw.starts_with("self.") {
            let field_name = &receiver_raw[receiver_raw.find('.').unwrap() + 1..];
            context.ctx.flow_engine.get_var(context.ir.source_file_id, field_name)
                .and_then(|v| v.inferred_type.clone())
        } else {
            context.ctx.flow_engine.get_var(context.ir.source_file_id, receiver_raw)
                .and_then(|v| v.inferred_type.clone())
        };

        if let Some(t_name) = type_name {
            type_name_to_use = Some(t_name.clone());
            origin = crate::resolver::context::SymbolOrigin::LocalVariable;
            
            // Check if it's an import by looking up its type_name in import_resolver
            if let Some(imported) = context.ctx.import_resolver.resolve(context.ir.source_file_id, &t_name) {
                let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
                let is_rust = file_path.map(|p| p.ends_with(".rs")).unwrap_or(false);
                let is_js_ts = file_path.map(|p| {
                    p.ends_with(".ts") || p.ends_with(".tsx")
                        || p.ends_with(".js") || p.ends_with(".jsx")
                        || p.ends_with(".mjs") || p.ends_with(".cjs")
                        || p.ends_with(".vue") || p.ends_with(".svelte")
                }).unwrap_or(false);
                let is_python = file_path.map(|p| p.ends_with(".py")).unwrap_or(false);

                let state = crate::resolver::stages::classification::classify_import_source(
                    &imported.source, &imported.name, is_rust, is_js_ts, is_python, &context.ctx.symbol_index.python_packages
                );
                
                match state {
                    ResolutionState::RepositorySymbol => origin = crate::resolver::context::SymbolOrigin::RepositoryImport,
                    ResolutionState::ExternalDependency => origin = crate::resolver::context::SymbolOrigin::ExternalImport,
                    ResolutionState::StandardLibrary => origin = crate::resolver::context::SymbolOrigin::StandardLibrary,
                    ResolutionState::Builtin => origin = crate::resolver::context::SymbolOrigin::Builtin,
                    _ => origin = crate::resolver::context::SymbolOrigin::Unknown,
                }
                
                imported_source = Some(imported.source);
                resolved_state = Some(state);
            }
        } else {
            // Check if the receiver raw itself is an import (just in case flow engine missed it)
            if let Some(imported) = context.ctx.import_resolver.resolve(context.ir.source_file_id, receiver_raw) {
                type_name_to_use = Some(imported.name.clone());

                // The imported name might not itself be a class/type — e.g. a
                // module-level singleton instance (`export const
                // inventoryRepository = new InventoryRepository();`),
                // imported and called directly by its lowercase instance
                // name. find_method_in_type would then search for methods
                // under a type literally named "inventoryRepository", which
                // doesn't exist. Follow it one hop instead: look up what that
                // singleton's own defining file inferred its constructed
                // type to be (VariableFlowEngine's load_constructors runs
                // over every file's relationships, so this is already
                // available), and use that as the effective type.
                if let Some(sym_id) = imported.symbol_id {
                    if let Some(sym) = context.ctx.symbol_index.get_symbol(sym_id) {
                        if sym.kind != "type" {
                            if let Some(inferred) = context.ctx.flow_engine
                                .get_var(sym.file_id, &imported.name)
                                .and_then(|v| v.inferred_type.clone())
                            {
                                type_name_to_use = Some(inferred);
                            }
                        }
                    }
                }
                let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
                let is_rust = file_path.map(|p| p.ends_with(".rs")).unwrap_or(false);
                let is_js_ts = file_path.map(|p| {
                    p.ends_with(".ts") || p.ends_with(".tsx")
                        || p.ends_with(".js") || p.ends_with(".jsx")
                        || p.ends_with(".mjs") || p.ends_with(".cjs")
                        || p.ends_with(".vue") || p.ends_with(".svelte")
                }).unwrap_or(false);
                let is_python = file_path.map(|p| p.ends_with(".py")).unwrap_or(false);

                let state = crate::resolver::stages::classification::classify_import_source(
                    &imported.source, &imported.name, is_rust, is_js_ts, is_python, &context.ctx.symbol_index.python_packages
                );
                
                match state {
                    ResolutionState::RepositorySymbol => origin = crate::resolver::context::SymbolOrigin::RepositoryImport,
                    ResolutionState::ExternalDependency => origin = crate::resolver::context::SymbolOrigin::ExternalImport,
                    ResolutionState::StandardLibrary => origin = crate::resolver::context::SymbolOrigin::StandardLibrary,
                    ResolutionState::Builtin => origin = crate::resolver::context::SymbolOrigin::Builtin,
                    _ => origin = crate::resolver::context::SymbolOrigin::Unknown,
                }
                
                imported_source = Some(imported.source);
                resolved_state = Some(state);
            }
        }

        let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
        let is_js_ts = file_path.map(|p| {
            p.ends_with(".ts") || p.ends_with(".tsx")
                || p.ends_with(".js") || p.ends_with(".jsx")
                || p.ends_with(".mjs") || p.ends_with(".cjs")
                || p.ends_with(".vue") || p.ends_with(".svelte")
        }).unwrap_or(false);

        if origin == crate::resolver::context::SymbolOrigin::Unknown && is_js_ts {
            if JS_BUILTIN_RECEIVERS.contains(&receiver_raw) {
                origin = crate::resolver::context::SymbolOrigin::Builtin;
                resolved_state = Some(ResolutionState::Builtin);
            }
        }
        
        let Some(type_name) = type_name_to_use.or_else(|| {
            if origin == crate::resolver::context::SymbolOrigin::Builtin {
                Some(receiver_raw.to_string())
            } else {
                None
            }
        }) else {
            context.fail_stage(self.stage_type(), DecisionReason::UnknownReceiverType, None);
            return Ok(());
        };

        match origin {
            crate::resolver::context::SymbolOrigin::ExternalImport => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::ExternalDependency,
                    DecisionReason::ExternalDependencyClassification,
                    Some(format!("External receiver from: {}", imported_source.unwrap_or_default()))
                );
                return Ok(());
            }
            crate::resolver::context::SymbolOrigin::StandardLibrary => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::StandardLibrary,
                    DecisionReason::StandardLibraryClassification,
                    Some(format!("Stdlib receiver from: {}", imported_source.unwrap_or_default()))
                );
                return Ok(());
            }
            crate::resolver::context::SymbolOrigin::Builtin => {
                context.resolve_with(
                    self.stage_type(),
                    ResolutionState::Builtin,
                    DecisionReason::BuiltinClassification,
                    Some(format!("Builtin receiver: {}", type_name))
                );
                return Ok(());
            }
            crate::resolver::context::SymbolOrigin::LocalVariable | crate::resolver::context::SymbolOrigin::RepositoryImport => {
                // proceed
            }
            crate::resolver::context::SymbolOrigin::Unknown => {
                // proceed
            }
        }

        // Step 3: Resolve method on the final type
        let mut candidates = context.ctx.symbol_index.find_method_in_type(&method_name, &type_name);

        if candidates.is_empty() {
            if let Some(type_ids) = context.ctx.symbol_index.find_by_name(&type_name) {
                for &type_id in type_ids {
                    for ancestor_id in context.ctx.type_graph.get_ancestors(type_id) {
                        if let Some(ancestor) = context.ctx.symbol_index.get_symbol(ancestor_id) {
                            candidates.extend(context.ctx.symbol_index.find_method_in_type(&method_name, &ancestor.name));
                        }
                    }
                }
            }
        }

        if !candidates.is_empty() {
            let resolution_candidates = candidates
                .into_iter()
                .map(|id| ResolutionCandidate {
                    symbol_id: id,
                    score: 1.0,
                    state: ResolutionState::RepositorySymbol,
                })
                .collect();
                
            let reason = if origin == crate::resolver::context::SymbolOrigin::RepositoryImport {
                DecisionReason::RepositoryMatch
            } else {
                DecisionReason::VariableAssignment
            };

            context.add_candidates(
                self.stage_type(),
                resolution_candidates,
                Some(reason.clone()),
                Some(format!("Receiver type: {}", type_name))
            );
            context.resolve_with(
                self.stage_type(),
                ResolutionState::RepositorySymbol,
                reason,
                None
            );
        } else {
            context.resolve_with(
                self.stage_type(),
                ResolutionState::Missing,
                DecisionReason::MissingImport,
                Some(format!("Method missing on type: {}", type_name))
            );
        }
        
        Ok(())
    }
}
