use graph::RelationshipNode;
use graph::edge_kind;

/// All syntactic relationship kinds discoverable from AST traversal.
/// Maps to `edge_kind::*` constants for DB storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationshipKind {
    Import,
    Call,
    MethodCall,
    MemberAccess,
    TypeRef,
    Inherits,
    Implements,
    Extends,
    NewCall,
    Instantiates,
    GlobalRef,
    ReExport,
    Annotation,
    GenericConstraint,
    Interaction,
    ComponentUse,
}

impl RelationshipKind {
    pub fn as_edge_kind(&self) -> &'static str {
        match self {
            RelationshipKind::Import => edge_kind::IMPORTS,
            RelationshipKind::Call => edge_kind::CALLS,
            RelationshipKind::MethodCall => edge_kind::METHOD_CALL,
            RelationshipKind::MemberAccess => edge_kind::MEMBER_ACCESS,
            RelationshipKind::TypeRef => edge_kind::TYPE_REF,
            RelationshipKind::Inherits => edge_kind::INHERITS,
            RelationshipKind::Implements => edge_kind::IMPLEMENTS,
            RelationshipKind::Extends => edge_kind::EXTENDS,
            RelationshipKind::NewCall => edge_kind::NEW_CALL,
            RelationshipKind::Instantiates => edge_kind::INSTANTIATES,
            RelationshipKind::GlobalRef => edge_kind::GLOBAL_REF,
            RelationshipKind::ReExport => edge_kind::RE_EXPORT,
            RelationshipKind::Annotation => edge_kind::ANNOTATION,
            RelationshipKind::GenericConstraint => edge_kind::GENERIC_CONSTRAINT,
            RelationshipKind::Interaction => edge_kind::INTERACTION,
            RelationshipKind::ComponentUse => edge_kind::COMPONENT_USE,
        }
    }
}

/// Intermediate relationship emitted by a language visitor during AST traversal.
/// source_symbol_id is None at discovery time; the resolver fills it in.
#[derive(Debug, Clone)]
pub struct Relationship {
    pub target_name: String,
    pub kind: RelationshipKind,
    pub source_path: Option<String>,
    pub line_number: usize,
    pub confidence: f32,
}

impl Relationship {
    pub fn new(target_name: impl Into<String>, kind: RelationshipKind, line_number: usize) -> Self {
        Self {
            target_name: target_name.into(),
            kind,
            source_path: None,
            line_number,
            confidence: 1.0,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source_path = Some(source.into());
        self
    }

    pub fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence;
        self
    }

    /// Convert to the existing RelationshipNode for storage compatibility.
    pub fn into_relationship_node(self) -> RelationshipNode {
        RelationshipNode {
            name: self.target_name,
            source: self.source_path,
            line_number: self.line_number,
            kind: Some(self.kind.as_edge_kind().to_string()),
        }
    }
}
