use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SemanticNodeKind {
    Symbol,
    File,
    Directory,
    Subsystem,
    Community,
    Route,
    Feature,
    Entrypoint,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SemanticNode {
    pub id: i64,
    pub name: String,
    pub qualified_name: String,
    pub kind: SemanticNodeKind,
    pub language: Option<String>,
    pub byte_range: Option<(usize, usize)>,
    pub path: String,
}

impl SemanticNode {
    pub fn new(
        id: i64,
        name: String,
        qualified_name: String,
        kind: SemanticNodeKind,
        language: Option<String>,
        byte_range: Option<(usize, usize)>,
        path: String,
    ) -> Self {
        Self {
            id,
            name,
            qualified_name,
            kind,
            language,
            byte_range,
            path,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionResult {
    pub matched: bool,
    pub confidence: Option<String>,
    pub aliases: Vec<String>,
    pub ambiguities: Vec<String>,
    pub resolved_name: String,
    pub node: SemanticNode,
}
