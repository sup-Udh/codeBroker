use crate::graph_builder::types::GraphEdgeIR;
use crate::graph_builder::metrics::GraphBuilderMetrics;

pub struct GraphBuildResult {
    pub edges: Vec<GraphEdgeIR>,
    pub metrics: GraphBuilderMetrics,
    pub diagnostics: Vec<String>, // GraphDiagnostic, using String for now unless a specific struct is needed
}
