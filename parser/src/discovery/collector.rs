use super::relationship::Relationship;
use graph::RelationshipNode;

/// Centralized collector for relationships discovered during AST traversal.
/// All language visitors emit into this collector, which deduplicates and
/// converts to RelationshipNode for storage.
pub struct RelationshipCollector {
    items: Vec<Relationship>,
}

impl RelationshipCollector {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn emit(&mut self, rel: Relationship) {
        self.items.push(rel);
    }

    /// Convert all discovered relationships to RelationshipNodes for DB storage.
    /// Deduplicates by (name, kind, line) to prevent duplicate edges from
    /// overlapping AST node matches.
    pub fn into_relationship_nodes(self) -> Vec<RelationshipNode> {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for item in self.items {
            let key = (
                item.target_name.clone(),
                item.kind.as_edge_kind().to_string(),
                item.line_number,
            );
            if seen.insert(key) {
                result.push(item.into_relationship_node());
            }
        }
        result
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
}

impl Default for RelationshipCollector {
    fn default() -> Self {
        Self::new()
    }
}
