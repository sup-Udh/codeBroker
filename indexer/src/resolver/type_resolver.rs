use crate::resolver::index::SymbolIndex;
use crate::resolver::type_graph::TypeGraph;
use crate::resolver::import_resolver::ImportResolver;
use crate::flow::VariableFlowEngine;

/// TypeResolver is a dedicated component to centralize type resolution.
/// Instead of stages directly querying Flow, TypeResolver becomes the API.
pub struct TypeResolver<'a> {
    pub index: &'a SymbolIndex,
    pub type_graph: &'a TypeGraph,
    pub imports: &'a ImportResolver,
    pub flow: &'a VariableFlowEngine,
}

impl<'a> TypeResolver<'a> {
    pub fn new(
        index: &'a SymbolIndex,
        type_graph: &'a TypeGraph,
        imports: &'a ImportResolver,
        flow: &'a VariableFlowEngine,
    ) -> Self {
        Self {
            index,
            type_graph,
            imports,
            flow,
        }
    }
    
    // API methods like resolve_type(symbol_id) or get_receiver_type(file_id, name)
}
