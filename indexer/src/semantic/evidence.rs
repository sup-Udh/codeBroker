/// The mechanism by which a type was determined for a variable or expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticEvidence {
    /// Explicit type annotation: `const x: Type` or `x: Type` in Python
    Annotation,
    /// Constructor call with binding: `const x = new Type()` or `x = Type()`
    Constructor,
    /// Simple identifier alias: `const x = y` where y's type is known
    Alias,
    /// Import statement gave the type: the name resolves to an imported type
    Import,
    /// Function return type propagation: `const x = f()` where `f(): Type`
    TypePropagation,
    /// Receiver chain resolution: `this.field.method()` via class field types
    Receiver,
    /// Direct method resolution on a typed receiver
    Method,
}

/// How confident we are in the type determination — used to prioritise
/// competing type bindings for the same variable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolutionConfidence {
    Low,
    Medium,
    High,
    Certain,
}
