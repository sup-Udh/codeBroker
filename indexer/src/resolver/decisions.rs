use serde::{Deserialize, Serialize};
use graph::models::ResolutionState;
use crate::resolver::context::ResolutionCandidate;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PipelineStageType {
    Classification,
    ReceiverResolution,
    LexicalGeneration,
    ScopeFilter,
    ModuleFilter,
    Ranking,
}

impl PipelineStageType {
    pub fn as_str(&self) -> &'static str {
        match self {
            PipelineStageType::Classification => "Classification",
            PipelineStageType::ReceiverResolution => "Receiver Resolution",
            PipelineStageType::LexicalGeneration => "Lexical Generation",
            PipelineStageType::ScopeFilter => "Scope Filter",
            PipelineStageType::ModuleFilter => "Module Filter",
            PipelineStageType::Ranking => "Ranking",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageStatus {
    Success,
    Failed,
    NotApplicable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum DecisionReason {
    UnknownReceiver,
    UnknownReceiverType,
    VariableFlowUnavailable,
    AliasResolutionFailed,
    ConstructorResolutionFailed,
    MissingImport,
    MissingExport,
    ModuleNotIndexed,
    CandidateRejectedByScope,
    CandidateRejectedByModule,
    MultipleCandidates,
    RepositoryMatch,
    BuiltinClassification,
    ExternalDependencyClassification,
    StandardLibraryClassification,
    RecursiveRelationship,
    ParserGap,
    DynamicDispatch,
    DynamicPropertyAccess,
    Reflection,
    NoCandidatesGenerated,
    LexicalScopeMatch,
    VariableAssignment,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Recoverability {
    Expected,
    Recoverable,
    Unrecoverable,
}

impl DecisionReason {
    pub fn recoverability(&self) -> Recoverability {
        match self {
            DecisionReason::BuiltinClassification
            | DecisionReason::ExternalDependencyClassification
            | DecisionReason::StandardLibraryClassification
            | DecisionReason::RepositoryMatch
            | DecisionReason::LexicalScopeMatch
            | DecisionReason::VariableAssignment
            | DecisionReason::MultipleCandidates
            | DecisionReason::DynamicDispatch => Recoverability::Expected,

            DecisionReason::UnknownReceiver
            | DecisionReason::UnknownReceiverType
            | DecisionReason::VariableFlowUnavailable
            | DecisionReason::AliasResolutionFailed
            | DecisionReason::ConstructorResolutionFailed
            | DecisionReason::MissingImport
            | DecisionReason::MissingExport
            | DecisionReason::ModuleNotIndexed
            | DecisionReason::CandidateRejectedByScope
            | DecisionReason::CandidateRejectedByModule
            | DecisionReason::RecursiveRelationship
            | DecisionReason::NoCandidatesGenerated
            | DecisionReason::ParserGap => Recoverability::Recoverable,

            DecisionReason::DynamicPropertyAccess
            | DecisionReason::Reflection => Recoverability::Unrecoverable,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            DecisionReason::UnknownReceiver => "UnknownReceiver",
            DecisionReason::UnknownReceiverType => "UnknownReceiverType",
            DecisionReason::VariableFlowUnavailable => "VariableFlowUnavailable",
            DecisionReason::AliasResolutionFailed => "AliasResolutionFailed",
            DecisionReason::ConstructorResolutionFailed => "ConstructorResolutionFailed",
            DecisionReason::MissingImport => "MissingImport",
            DecisionReason::MissingExport => "MissingExport",
            DecisionReason::ModuleNotIndexed => "ModuleNotIndexed",
            DecisionReason::CandidateRejectedByScope => "CandidateRejectedByScope",
            DecisionReason::CandidateRejectedByModule => "CandidateRejectedByModule",
            DecisionReason::MultipleCandidates => "MultipleCandidates",
            DecisionReason::RepositoryMatch => "RepositoryMatch",
            DecisionReason::BuiltinClassification => "BuiltinClassification",
            DecisionReason::ExternalDependencyClassification => "ExternalDependencyClassification",
            DecisionReason::StandardLibraryClassification => "StandardLibraryClassification",
            DecisionReason::RecursiveRelationship => "RecursiveRelationship",
            DecisionReason::ParserGap => "ParserGap",
            DecisionReason::DynamicDispatch => "DynamicDispatch",
            DecisionReason::DynamicPropertyAccess => "DynamicPropertyAccess",
            DecisionReason::Reflection => "Reflection",
            DecisionReason::NoCandidatesGenerated => "NoCandidatesGenerated",
            DecisionReason::LexicalScopeMatch => "LexicalScopeMatch",
            DecisionReason::VariableAssignment => "VariableAssignment",
        }
    }

    pub fn to_evidence(&self) -> graph::models::ResolutionEvidence {
        match self {
            DecisionReason::UnknownReceiver => graph::models::ResolutionEvidence::UnknownReceiver,
            DecisionReason::UnknownReceiverType => graph::models::ResolutionEvidence::UnknownReceiver,
            DecisionReason::VariableFlowUnavailable => graph::models::ResolutionEvidence::DynamicDispatch,
            DecisionReason::AliasResolutionFailed => graph::models::ResolutionEvidence::Alias,
            DecisionReason::ConstructorResolutionFailed => graph::models::ResolutionEvidence::ConstructorCall,
            DecisionReason::MissingImport => graph::models::ResolutionEvidence::MissingImport,
            DecisionReason::MissingExport => graph::models::ResolutionEvidence::MissingExport,
            DecisionReason::ModuleNotIndexed => graph::models::ResolutionEvidence::UnknownModule,
            DecisionReason::CandidateRejectedByScope => graph::models::ResolutionEvidence::LexicalScopeMatch,
            DecisionReason::CandidateRejectedByModule => graph::models::ResolutionEvidence::UnknownModule,
            DecisionReason::MultipleCandidates => graph::models::ResolutionEvidence::AmbiguousCandidates,
            DecisionReason::RepositoryMatch => graph::models::ResolutionEvidence::ImportMatch,
            DecisionReason::BuiltinClassification => graph::models::ResolutionEvidence::BuiltinClassification,
            DecisionReason::ExternalDependencyClassification => graph::models::ResolutionEvidence::ExternalDependency,
            DecisionReason::StandardLibraryClassification => graph::models::ResolutionEvidence::BuiltinClassification,
            DecisionReason::RecursiveRelationship => graph::models::ResolutionEvidence::RecursiveCall,
            DecisionReason::ParserGap => graph::models::ResolutionEvidence::DynamicDispatch,
            DecisionReason::DynamicDispatch => graph::models::ResolutionEvidence::DynamicDispatch,
            DecisionReason::DynamicPropertyAccess => graph::models::ResolutionEvidence::DynamicMemberAccess,
            DecisionReason::Reflection => graph::models::ResolutionEvidence::DynamicDispatch,
            DecisionReason::NoCandidatesGenerated => graph::models::ResolutionEvidence::MissingImport,
            DecisionReason::LexicalScopeMatch => graph::models::ResolutionEvidence::LexicalScopeMatch,
            DecisionReason::VariableAssignment => graph::models::ResolutionEvidence::VariableAssignment,
        }
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageDecision {
    pub stage: PipelineStageType,
    pub status: StageStatus,
    pub reason: Option<DecisionReason>,
    pub candidates_before: Vec<i64>,
    pub candidates_after: Vec<i64>,
    pub notes: Option<String>,
}
