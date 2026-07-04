use graph::node::{ResolutionResult, SemanticNode, SemanticNodeKind};
use storage::Database;
use crate::pipeline::resolve_any;
use crate::types::ResolvedEntity;

pub struct UniversalResolver;

impl UniversalResolver {
    /// Translates arbitrary user/tool input into a ResolutionResult containing a SemanticNode.
    pub fn resolve_query(db: &Database, query: &str, file_hint: Option<&str>) -> ResolutionResult {
        let resolved = resolve_any(db, query, file_hint, &[], None);
        
        match resolved {
            ResolvedEntity::Symbol(s) => {
                let node = SemanticNode::new(
                    0, // In reality, fetch actual ID
                    s.name.clone(),
                    s.name.clone(),
                    if s.is_entrypoint { SemanticNodeKind::Entrypoint } else { SemanticNodeKind::Symbol },
                    None,
                    Some((s.start_line as usize, s.end_line as usize)), // Actually byte bounds in practice
                    s.file_path.clone(),
                );
                
                ResolutionResult {
                    matched: true,
                    confidence: Some(format!("{:?}", s.confidence.label)),
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: s.name,
                    node,
                }
            }
            // For now, map everything else as a generic miss or basic node
            // A full implementation will map Files, Subsystems, Features correctly.
            _ => {
                ResolutionResult {
                    matched: false,
                    confidence: None,
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: query.to_string(),
                    node: SemanticNode::new(
                        0, query.to_string(), query.to_string(), SemanticNodeKind::Symbol, None, None, String::new()
                    ),
                }
            }
        }
    }
}
