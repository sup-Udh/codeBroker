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
        
        let mut current_type: Option<String> = None;
        let mut start_idx = 0;

        // Step 1: Resolve the base variable (or base field if starts with this/self)
        if !chain.tokens.is_empty() {
            if chain.tokens[0] == "this" || chain.tokens[0] == "self" {
                if chain.tokens.len() > 1 {
                    let field_name = &chain.tokens[1];
                    current_type = context.ctx.flow_engine.get_var(context.ir.source_file_id, field_name)
                        .and_then(|v| v.inferred_type.clone());
                    start_idx = 2;
                }
            } else {
                current_type = context.ctx.flow_engine.get_var(context.ir.source_file_id, &chain.tokens[0])
                    .and_then(|v| v.inferred_type.clone());
                start_idx = 1;
            }
        }

        // Step 2: Resolve property chain (bar.baz) on the type
        for i in start_idx..chain.tokens.len() {
            let token = &chain.tokens[i];
            if let Some(t) = current_type {
                // Find property `token` on type `t`
                let candidates = context.ctx.symbol_index.find_method_in_type(token, &t);
                if !candidates.is_empty() {
                    // We found the property, we need its type. But wait, if it's a property, its type might be in the type graph or semantic bindings.
                    // For now, if we don't have a way to get property types easily from the index, we just assume the property name is its type,
                    // OR we check the flow engine? Actually, the SymbolIndex doesn't store field types directly unless we check semantic bindings.
                    // BUT for phase 14A, we just need to resolve it step-by-step.
                    // Since we can't easily extract property types yet without TypeGraph, if we hit a chain we might just fail if we can't find it.
                    // Let's look up the field in the flow engine under the base variable instead. Flow engine tracks nested variables!
                    
                    // Actually, the FlowEngine already tracks `foo.bar.baz`. 
                    // So we can just ask the FlowEngine for the full receiver string!
                }
            }
            
            // Simpler approach leveraging FlowEngine's existing nested variable support:
            // FlowEngine creates variables like `foo.bar.baz` if we query it!
            break; 
        }
        
        // Let's just ask FlowEngine for the full receiver directly, since FlowEngine handles nested fields (like foo.bar, this.db).
        let type_name: Option<String> = if receiver_raw.starts_with("this.") || receiver_raw.starts_with("self.") {
            let field_name = &receiver_raw[receiver_raw.find('.').unwrap() + 1..];
            context.ctx.flow_engine.get_var(context.ir.source_file_id, field_name)
                .and_then(|v| v.inferred_type.clone())
        } else {
            context.ctx.flow_engine.get_var(context.ir.source_file_id, receiver_raw)
                .and_then(|v| v.inferred_type.clone())
        };

        let Some(type_name) = type_name else {
            context.fail_stage(self.stage_type(), DecisionReason::UnknownReceiverType, None);
            return Ok(());
        };

        let file_path = context.ctx.symbol_index.file_paths.get(&context.ir.source_file_id);
        let is_js_ts = file_path.map(|p| {
            p.ends_with(".ts") || p.ends_with(".tsx")
                || p.ends_with(".js") || p.ends_with(".jsx")
                || p.ends_with(".mjs") || p.ends_with(".cjs")
                || p.ends_with(".vue") || p.ends_with(".svelte")
        }).unwrap_or(false);

        if is_js_ts && JS_BUILTIN_RECEIVERS.contains(&type_name.as_str()) {
            context.resolve_with(
                self.stage_type(),
                ResolutionState::Builtin,
                DecisionReason::BuiltinClassification,
                None
            );
            return Ok(());
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
                
            context.add_candidates(
                self.stage_type(),
                resolution_candidates,
                Some(DecisionReason::RepositoryMatch),
                Some(format!("Receiver type: {}", type_name))
            );
            context.resolve_with(
                self.stage_type(),
                ResolutionState::RepositorySymbol,
                DecisionReason::VariableAssignment,
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
