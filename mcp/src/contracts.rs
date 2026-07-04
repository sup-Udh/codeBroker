use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum GraphPrimitive {
    SemanticNode,
    ParentFile,
    FeatureMembership,
    DeveloperIntelligence,
    Embedding,
    ContextReachability,
    DependencyEdges,
    CallerEdges,
    CalleeEdges,
    CallGraph,
    Implementation,
    Subsystem,
    Entrypoints,
    Dependencies,
    Hotspots,
    Community,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolManifest {
    pub name: String,
    pub requires: Vec<GraphPrimitive>,
}

impl ToolManifest {
    pub fn new(name: &str, requires: Vec<GraphPrimitive>) -> Self {
        Self {
            name: name.to_string(),
            requires,
        }
    }
}
