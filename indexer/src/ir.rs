use graph::models::{RelationshipNode, ResolutionState};

/// A stable, immutable intermediate representation of an unresolved relationship.
/// This acts as the single source of truth for the Resolver pipeline.
#[derive(Debug, Clone)]
pub struct RelationshipIR {
    pub id: i64,
    pub source_file_id: i64,
    pub node: RelationshipNode,
    pub enclosing_symbol_id: Option<i64>,
}

/// A fully resolved relationship, immutable, ready for graph emission.
#[derive(Debug, Clone)]
pub struct ResolvedRelationshipIR {
    pub original: RelationshipIR,
    pub state: ResolutionState,
    pub target_symbol_ids: Vec<i64>,
    pub confidence: f64,
}
