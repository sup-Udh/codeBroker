/// Universal code relationships used as edge `kind` values in the graph.
/// Every parser frontend and linker must use these constants instead of
/// inline string literals so there is one authoritative list of edge kinds.
///
/// The graph is intentionally language-agnostic: the same edge kinds apply
/// across Python, TypeScript, JavaScript, and Rust. Framework-specific
/// relationships (e.g. React component trees, FastAPI route registration)
/// are NOT modelled as distinct edge kinds — they emerge naturally from the
/// same universal primitives (imports, calls, inherits, etc.).
pub mod edge_kind {
    /// A module-level import declaration.
    pub const IMPORTS: &str = "imports";
    /// A direct function or closure call: `foo(args)`.
    pub const CALLS: &str = "calls";
    /// An invocation through a receiver: `obj.method(args)`.
    pub const METHOD_CALL: &str = "method_call";
    /// Property or field read without an immediate call: `obj.field`.
    pub const MEMBER_ACCESS: &str = "MEMBER_ACCESS";
    /// Constructor invocation: `new Foo(args)` (TypeScript/JavaScript).
    pub const NEW_CALL: &str = "new_call";
    /// Class or interface inheritance: `class A extends B` / `interface I extends J`.
    pub const EXTENDS: &str = "extends";
    /// TypeScript interface implementation: `class A implements I`.
    pub const IMPLEMENTS: &str = "implements";
    /// Python class inheritance: `class A(B)`.
    pub const INHERITS: &str = "inherits";
    /// Assignment whose right-hand side is a constructor call: `x = Foo()`.
    pub const INSTANTIATES: &str = "instantiates";
    /// Type annotation on a parameter or return value: `def f(x: T) -> R`.
    pub const TYPE_REF: &str = "type_ref";
    /// A `global x` declaration inside a Python function body.
    pub const GLOBAL_REF: &str = "global_ref";
    /// Re-export of a symbol from another module: `export { Foo } from './bar'`.
    pub const RE_EXPORT: &str = "re_export";
    /// Inferred runtime-boundary edge (e.g. an HTTP fetch resolved to its handler).
    pub const INTERACTION: &str = "interaction";
    /// A component rendered inside another component's output.
    pub const COMPONENT_USE: &str = "component_use";
}

#[derive(Debug, Default)]
pub struct FileMetadata {
    pub metadata: Option<String>,
}

pub struct SymbolNode {
    pub name: String,
    pub kind: String,
    pub start_line: usize,
    pub end_line: usize,
    pub start_byte: usize,
    pub end_byte: usize,
    pub signature: Option<String>,
    pub attributes: Vec<String>,
    pub metadata: Option<String>,
}

pub struct ImportNode {
    pub name: String,
    pub source: Option<String>,
    pub line_number: usize,
    /// One of the `edge_kind::*` constants. `None` defaults to `edge_kind::IMPORTS`.
    pub kind: Option<String>,
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
