use crate::naming::CanonicalNameResolver;
use crate::pipeline::resolve_any;
use crate::types::ResolvedEntity;
use graph::node::{ResolutionResult, SemanticNode, SemanticNodeKind};
use storage::Database;

pub struct UniversalResolver;

impl UniversalResolver {
    /// Translates arbitrary user/tool input into a `ResolutionResult`
    /// containing a `SemanticNode` — the one entry point every graph-aware
    /// tool should call instead of matching, ranking, or picking a "first
    /// row" independently. Delegates all matching/ranking/ambiguity
    /// decisions to `resolve_any` (the same deterministic pipeline
    /// `resolve_symbol`/`resolve_subsystem`/`resolve_path` are built from);
    /// this function only reshapes whichever `ResolvedEntity` variant comes
    /// back into the coarser `SemanticNode` vocabulary.
    pub fn resolve_query(db: &Database, query: &str, file_hint: Option<&str>) -> ResolutionResult {
        let resolved = resolve_any(db, query, file_hint);
        Self::from_resolved_entity(db, query, resolved)
    }

    fn from_resolved_entity(
        db: &Database,
        query: &str,
        resolved: ResolvedEntity,
    ) -> ResolutionResult {
        match resolved {
            ResolvedEntity::Symbol(s) => {
                let node = SemanticNode::new(
                    s.id,
                    s.name.clone(),
                    s.name.clone(),
                    if s.is_entrypoint {
                        SemanticNodeKind::Entrypoint
                    } else {
                        SemanticNodeKind::Symbol
                    },
                    None,
                    Some((s.start_byte as usize, s.end_byte as usize)),
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
            ResolvedEntity::File(f) => {
                // Files have a real row id in the `files` table (unlike
                // directories/subsystems/features, which aren't single rows)
                // — look it up so a File node's `id` is as load-bearing as a
                // Symbol node's, not a placeholder.
                let relative_path = CanonicalNameResolver::normalize_path(db, &f.file_path);
                let file_id: i64 = db
                    .conn
                    .query_row(
                        "SELECT id FROM files WHERE path = ?1",
                        rusqlite::params![relative_path],
                        |row| row.get(0),
                    )
                    .unwrap_or(0);
                let node = SemanticNode::new(
                    file_id,
                    f.file_path.clone(),
                    f.file_path.clone(),
                    SemanticNodeKind::File,
                    None,
                    None,
                    f.file_path.clone(),
                );
                ResolutionResult {
                    matched: true,
                    confidence: Some(format!("{:?}", f.confidence.label)),
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: f.file_path,
                    node,
                }
            }
            ResolvedEntity::Directory(d) => {
                // Directories aren't a single indexed row, so there is no
                // meaningful numeric id — 0 with `SemanticNodeKind::Directory`
                // documents that explicitly rather than fabricating one.
                let node = SemanticNode::new(
                    0,
                    d.directory_path.clone(),
                    d.directory_path.clone(),
                    SemanticNodeKind::Directory,
                    None,
                    None,
                    d.directory_path.clone(),
                );
                ResolutionResult {
                    matched: true,
                    confidence: Some(format!("{:?}", d.confidence.label)),
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: d.directory_path,
                    node,
                }
            }
            ResolvedEntity::Subsystem(sub) => {
                let node = SemanticNode::new(
                    0,
                    sub.name.clone(),
                    sub.name.clone(),
                    SemanticNodeKind::Subsystem,
                    None,
                    None,
                    sub.files.first().cloned().unwrap_or_default(),
                );
                ResolutionResult {
                    matched: true,
                    confidence: Some(format!("{:?}", sub.confidence.label)),
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: sub.name,
                    node,
                }
            }
            ResolvedEntity::Feature(feat) => {
                let node = SemanticNode::new(
                    0,
                    feat.concept.clone(),
                    feat.concept.clone(),
                    SemanticNodeKind::Feature,
                    None,
                    None,
                    String::new(),
                );
                ResolutionResult {
                    matched: true,
                    confidence: Some(format!("{:?}", feat.confidence.label)),
                    aliases: vec![],
                    ambiguities: vec![],
                    resolved_name: feat.concept,
                    node,
                }
            }
            ResolvedEntity::Ambiguous(a) => ResolutionResult {
                matched: false,
                confidence: None,
                aliases: vec![],
                ambiguities: a.candidates.iter().map(|c| c.name.clone()).collect(),
                resolved_name: a.query,
                node: Self::unmatched_node(query),
            },
            ResolvedEntity::NotFound(nf) => ResolutionResult {
                matched: false,
                confidence: None,
                aliases: vec![],
                ambiguities: vec![],
                resolved_name: nf.query,
                node: Self::unmatched_node(query),
            },
        }
    }

    fn unmatched_node(query: &str) -> SemanticNode {
        SemanticNode::new(
            0,
            query.to_string(),
            query.to_string(),
            SemanticNodeKind::Symbol,
            None,
            None,
            String::new(),
        )
    }
}
