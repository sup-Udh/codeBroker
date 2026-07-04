use std::hash::{Hash, Hasher};

/// A stable, intermediate representation of a validated graph edge before it reaches SQLite.
#[derive(Debug, Clone)]
pub struct GraphEdgeIR {
    pub source_file_id: i64,
    pub source_symbol_id: Option<i64>,
    pub target_symbol_id: i64,
    pub kind: String,
    pub edge_type: String,

    /// Resolution confidence carried over from `ResolvedRelationshipIR::confidence`
    /// (1.0 = resolved via a typed receiver/import, 0.0 = bare lexical name match
    /// with no scope information). Lets query-time code admit `method_call` /
    /// `member_access` edges selectively instead of excluding the whole kind.
    pub confidence: f64,

    /// Provenance tracking: the originating relationship ID that produced this edge.
    /// Invaluable for diagnostics and debugging.
    pub relationship_id: i64,
}

impl PartialEq for GraphEdgeIR {
    fn eq(&self, other: &Self) -> bool {
        self.source_file_id == other.source_file_id &&
        self.source_symbol_id == other.source_symbol_id &&
        self.target_symbol_id == other.target_symbol_id &&
        self.kind == other.kind &&
        self.edge_type == other.edge_type
        // provenance (relationship_id) and confidence are explicitly excluded from
        // equality to ensure edges deduplicate properly even if they come from
        // different relationships or resolution passes.
    }
}

impl Eq for GraphEdgeIR {}

impl Hash for GraphEdgeIR {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.source_file_id.hash(state);
        self.source_symbol_id.hash(state);
        self.target_symbol_id.hash(state);
        self.kind.hash(state);
        self.edge_type.hash(state);
        // provenance and confidence intentionally excluded from hash
    }
}
