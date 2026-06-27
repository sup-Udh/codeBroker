use crate::resolver::stages::ResolutionStage;
use crate::resolver::context::{ResolutionCandidate, ResolutionContext};
use crate::resolver::stages::classification::JS_BUILTIN_RECEIVERS;
use graph::models::{ResolutionEvidence, ResolutionState};

/// Resolves method calls using the file's semantic type map.
///
/// Handles three cases:
/// 1. Direct receiver: `db.query()` where source="db" → look up "db" in var_types
/// 2. This-chain: `this.db.query()` where source="this.db" → look up "db" in field_types
/// 3. Self-chain (Python): `self.db.query()` where source="self.db" → same as above
/// All of these use alias propagation and annotation precedence from FileSemantics.
pub struct ReceiverResolutionStage;

impl ResolutionStage for ReceiverResolutionStage {
    fn name(&self) -> &'static str {
        "ReceiverResolutionStage"
    }

    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String> {
        let kind = context.relationship.kind.as_deref().unwrap_or("imports");

        if !matches!(kind, "method_call" | "MEMBER_ACCESS" | "instantiates") {
            return Ok(());
        }

        let Some(receiver_raw) = context.relationship.source.as_deref() else {
            return Ok(());
        };

        let method_name = context.relationship.name.clone();

        // Determine the type of the receiver
        let type_name: Option<String> = if receiver_raw.starts_with("this.")
            || receiver_raw.starts_with("self.")
        {
            // this.field / self.field → look up field name in flow engine
            let field_name = &receiver_raw[receiver_raw.find('.').unwrap() + 1..];
            context.flow_engine.get_var(context.source_file_id, field_name)
                .and_then(|v| v.inferred_type.clone())
        } else {
            // Regular variable receiver → use flow engine
            context.flow_engine.get_var(context.source_file_id, receiver_raw)
                .and_then(|v| v.inferred_type.clone())
        };

        let Some(type_name) = type_name else {
            return Ok(());
        };

        // If the resolved type is a known JS/TS builtin, classify as Builtin now.
        // This handles e.g. `res: Response` → `res.status()` → Builtin.
        let file_path = context.symbol_index.file_paths.get(&context.source_file_id);
        let is_js_ts = file_path.map(|p| {
            p.ends_with(".ts") || p.ends_with(".tsx")
                || p.ends_with(".js") || p.ends_with(".jsx")
                || p.ends_with(".mjs") || p.ends_with(".cjs")
                || p.ends_with(".vue") || p.ends_with(".svelte")
        }).unwrap_or(false);

        if is_js_ts && JS_BUILTIN_RECEIVERS.contains(&type_name.as_str()) {
            context.final_state = ResolutionState::Builtin;
            context.evidence.push(ResolutionEvidence::NamespaceMatch);
            context.resolved = true;
            return Ok(());
        }

        let candidates = context.symbol_index.find_method_in_type(&method_name, &type_name);

        if !candidates.is_empty() {
            context.candidates = candidates
                .into_iter()
                .map(|id| ResolutionCandidate {
                    symbol_id: id,
                    score: 1.0,
                    state: ResolutionState::RepositorySymbol,
                })
                .collect();
            context.evidence.push(ResolutionEvidence::VariableAssignment);
        }

        Ok(())
    }
}
