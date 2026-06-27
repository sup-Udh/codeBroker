use super::collector::RelationshipCollector;
use graph::SemanticBinding;
use tree_sitter::Tree;

/// A language-specific AST visitor that emits relationships into a collector.
/// Visitors are purely syntactic — they must not resolve symbols, query
/// databases, or perform semantic reasoning.
pub trait LanguageVisitor {
    fn visit(&self, tree: &Tree, source_code: &str, collector: &mut RelationshipCollector);

    /// Extract semantic bindings (type annotations, return types, field types,
    /// alias assignments) from the AST. These are stored separately from graph
    /// relationships and used only during resolution to propagate type info.
    /// Default: no bindings (non-typed languages or languages not yet instrumented).
    fn visit_semantic(&self, _tree: &Tree, _source_code: &str) -> Vec<SemanticBinding> {
        Vec::new()
    }
}
