/// Semantic binding from static type analysis — not a graph edge, used only
/// during indexing to propagate type information for receiver resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticBindingKind {
    /// Explicit type annotation: `const x: Type` or `x: Type` (Python)
    VarType,
    /// Function return type annotation: `function f(): Type` or `def f() -> Type`
    ReturnType,
    /// Class / struct field type: `class C { field: Type }` (context = class name)
    FieldType,
    /// Simple alias assignment: `const x = y` where y is an identifier
    Alias,
    /// Assignment from a call: `const x = foo()`
    Assignment,
    /// Destructuring: `const { x } = y`
    Destructuring,
    /// Object literal: `const x = { y: z }`
    ObjectLiteral,
}

impl SemanticBindingKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            SemanticBindingKind::VarType => "var_type",
            SemanticBindingKind::ReturnType => "return_type",
            SemanticBindingKind::FieldType => "field_type",
            SemanticBindingKind::Alias => "alias",
            SemanticBindingKind::Assignment => "assignment",
            SemanticBindingKind::Destructuring => "destructuring",
            SemanticBindingKind::ObjectLiteral => "object_literal",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "var_type" => Some(SemanticBindingKind::VarType),
            "return_type" => Some(SemanticBindingKind::ReturnType),
            "field_type" => Some(SemanticBindingKind::FieldType),
            "alias" => Some(SemanticBindingKind::Alias),
            "assignment" => Some(SemanticBindingKind::Assignment),
            "destructuring" => Some(SemanticBindingKind::Destructuring),
            "object_literal" => Some(SemanticBindingKind::ObjectLiteral),
            _ => None,
        }
    }
}

/// A semantic fact extracted from a file during parsing, stored separately from
/// graph relationships so that it doesn't appear in resolution metrics.
#[derive(Debug, Clone)]
pub struct SemanticBinding {
    pub kind: SemanticBindingKind,
    /// The name being bound: variable name, function name, or field name.
    pub name: String,
    /// The type or source name:
    /// - VarType/ReturnType/FieldType: the type identifier (e.g. "Database")
    /// - Alias: the source variable name (e.g. `const x = y` → type_name = "y")
    pub type_name: String,
    /// For FieldType: the class name that owns this field.
    pub context: Option<String>,
}

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
    /// Decorator, attribute, or annotation applied to a symbol:
    /// `#[derive(Debug)]`, `@Injectable()`, `@app.get("/")`.
    pub const ANNOTATION: &str = "annotation";
    /// Generic type constraint: `T: Clone` (Rust) or `T extends Serializable` (TypeScript).
    pub const GENERIC_CONSTRAINT: &str = "generic_constraint";
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

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionState {
    RepositorySymbol,
    WorkspaceModule,
    ExternalDependency,
    StandardLibrary,
    Builtin,
    Dynamic,
    Ambiguous,
    Unknown,
    Missing,
    Recursive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariableOrigin {
    Assignment,
    Constructor,
    ReturnValue,
    Alias,
    Parameter,
    Field,
    Destructuring,
    ObjectLiteral,
    Import,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ResolutionEvidence {
    ImportMatch,
    LexicalScopeMatch,
    ConstructorCall,
    ReturnFlow,
    VariableAssignment,
    Alias,
    AliasFlow,
    ParameterType,
    FieldType,
    ReceiverType,
    ObjectLiteral,
    Destructuring,
    GenericConstraint,
    ModuleExport,
    Builtin,
    BuiltinClassification,
    ExternalDependency,
    NamespaceMatch,
    DynamicDispatch,
    DynamicMemberAccess,
    UnknownReceiver,
    UnknownModule,
    MissingImport,
    MissingExport,
    AmbiguousCandidates,
    NoMatchingExport,
    RecursiveCall,
}

impl ResolutionEvidence {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionEvidence::ImportMatch => "ImportMatch",
            ResolutionEvidence::LexicalScopeMatch => "LexicalScopeMatch",
            ResolutionEvidence::ConstructorCall => "ConstructorCall",
            ResolutionEvidence::ReturnFlow => "ReturnFlow",
            ResolutionEvidence::VariableAssignment => "VariableAssignment",
            ResolutionEvidence::Alias => "Alias",
            ResolutionEvidence::AliasFlow => "AliasFlow",
            ResolutionEvidence::ParameterType => "ParameterType",
            ResolutionEvidence::FieldType => "FieldType",
            ResolutionEvidence::ReceiverType => "ReceiverType",
            ResolutionEvidence::ObjectLiteral => "ObjectLiteral",
            ResolutionEvidence::Destructuring => "Destructuring",
            ResolutionEvidence::GenericConstraint => "GenericConstraint",
            ResolutionEvidence::ModuleExport => "ModuleExport",
            ResolutionEvidence::Builtin => "Builtin",
            ResolutionEvidence::BuiltinClassification => "BuiltinClassification",
            ResolutionEvidence::ExternalDependency => "ExternalDependency",
            ResolutionEvidence::NamespaceMatch => "NamespaceMatch",
            ResolutionEvidence::DynamicDispatch => "DynamicDispatch",
            ResolutionEvidence::DynamicMemberAccess => "DynamicMemberAccess",
            ResolutionEvidence::UnknownReceiver => "UnknownReceiver",
            ResolutionEvidence::UnknownModule => "UnknownModule",
            ResolutionEvidence::MissingImport => "MissingImport",
            ResolutionEvidence::MissingExport => "MissingExport",
            ResolutionEvidence::AmbiguousCandidates => "AmbiguousCandidates",
            ResolutionEvidence::NoMatchingExport => "NoMatchingExport",
            ResolutionEvidence::RecursiveCall => "RecursiveCall",
        }
    }
}

impl ResolutionState {
    pub fn as_str(&self) -> &'static str {
        match self {
            ResolutionState::RepositorySymbol => "RepositorySymbol",
            ResolutionState::WorkspaceModule => "WorkspaceModule",
            ResolutionState::ExternalDependency => "ExternalDependency",
            ResolutionState::StandardLibrary => "StandardLibrary",
            ResolutionState::Builtin => "Builtin",
            ResolutionState::Dynamic => "Dynamic",
            ResolutionState::Ambiguous => "Ambiguous",
            ResolutionState::Unknown => "Unknown",
            ResolutionState::Missing => "Missing",
            ResolutionState::Recursive => "Recursive",
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelationshipNode {
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
