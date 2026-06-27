use graph::{RelationshipNode, SemanticBinding, SymbolNode};

pub trait LanguageFrontend {
    fn can_handle(&self, path: &str) -> bool;

    /// Parse source_code and extract symbols, relationships, and semantic bindings.
    /// The 4th element (semantic bindings) is stored separately from relationships
    /// so it doesn't appear in resolution metrics — it feeds the type-propagation
    /// pre-pass in the resolver.
    fn parse_and_extract(
        &self,
        source_code: &str,
        path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<RelationshipNode>,
        Vec<SemanticBinding>,
    )>;
}

pub use crate::config_frontend::ConfigFrontend;
pub use crate::javascript_frontend::JavaScriptFrontend;
pub use crate::python_frontend::PythonFrontend;
pub use crate::svelte_frontend::SvelteFrontend;
pub use crate::vue_frontend::VueFrontend;

pub struct RustFrontend;

impl LanguageFrontend for RustFrontend {
    fn can_handle(&self, path: &str) -> bool {
        path.ends_with(".rs")
    }

    fn parse_and_extract(
        &self,
        source_code: &str,
        _path: &str,
    ) -> Option<(
        graph::models::FileMetadata,
        Vec<SymbolNode>,
        Vec<RelationshipNode>,
        Vec<SemanticBinding>,
    )> {
        let tree = crate::treesitter::parse_rust_code(source_code)?;
        let symbols = crate::extractor::extract_symbols(&tree, source_code);

        let mut collector = crate::discovery::RelationshipCollector::new();
        crate::discovery::LanguageVisitor::visit(
            &crate::discovery::rust::RustVisitor,
            &tree,
            source_code,
            &mut collector,
        );
        let relationships = collector.into_relationship_nodes();

        Some((graph::models::FileMetadata::default(), symbols, relationships, Vec::new()))
    }
}
