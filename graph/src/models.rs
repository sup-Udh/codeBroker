// main src of truth that plays the major role between parser db and the cli

// retruning the following below
#[derive(Debug, Default)]
pub struct FileMetadata {
    pub directive: Option<String>,
    pub route_path: Option<String>,
    pub route_segment: Option<String>,
}

pub struct SymbolNode {
    pub name: String,
    pub kind: String,
    pub prop_type: Option<String>,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub signature: Option<String>,
    pub route_path: Option<String>,
    pub route_method: Option<String>,
}

pub struct ImportNode {
    pub name: String,
    pub source: Option<String>,
    pub line_number: usize,
    pub kind: Option<String>, // e.g., "imports", "renders_component", "consumes_hook"
}

/// Distinguishes edges the parser/indexer found by walking syntax (imports,
/// calls, renders) from edges inferred by matching a runtime-shaped pattern
/// (a `fetch("/api/...")` literal resolved to the Next.js route handler that
/// answers it, a websocket `send`/`emit` resolved to its handler, etc.) where
/// no static import/call relationship exists at all. Before this distinction
/// existed, `shortest_path` between a client component and the API route it
/// calls over HTTP always returned `found: false` with no signal that the
/// real connector was one query away (benchmark run_003's central finding) —
/// logical edges make that connector a first-class, traversable graph edge
/// instead of a query-time-only heuristic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeType {
    /// Found directly in the AST: a call, an import, a render, a hook use.
    Static,
    /// Inferred by matching a runtime-boundary pattern (HTTP fetch, websocket
    /// message, pub/sub emit) to the symbol that handles it on the other side.
    Logical,
}

impl EdgeType {
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::Static => "static",
            EdgeType::Logical => "logical",
        }
    }
}

impl std::fmt::Display for EdgeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for EdgeType {
    type Err = std::convert::Infallible;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(if s == "logical" {
            EdgeType::Logical
        } else {
            EdgeType::Static
        })
    }
}
