use rusqlite::Result;
use serde::{Deserialize, Serialize};
use storage::Database;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash, Copy)]
pub enum PipelineStage {
    Parser,
    Semantic,
    Flow,
    Receiver,
    Method,
    Resolver,
    Graph,
    FeatureExtraction,
    SemanticIndex,
    Retrieval,
    Consistency,
}

impl std::fmt::Display for PipelineStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PipelineStage::Parser => write!(f, "Parser"),
            PipelineStage::Semantic => write!(f, "Semantic"),
            PipelineStage::Flow => write!(f, "Flow"),
            PipelineStage::Receiver => write!(f, "Receiver"),
            PipelineStage::Method => write!(f, "Method Resolution"),
            PipelineStage::Resolver => write!(f, "Resolver"),
            PipelineStage::Graph => write!(f, "Graph"),
            PipelineStage::FeatureExtraction => write!(f, "Feature Extraction"),
            PipelineStage::SemanticIndex => write!(f, "Semantic Index"),
            PipelineStage::Retrieval => write!(f, "Retrieval"),
            PipelineStage::Consistency => write!(f, "Consistency"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StageStatus {
    Pass,
    Warning,
    Fail,
    Skipped,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiagnosticFinding {
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub likely_cause: String,
    pub suggested_fix: String,
    pub file_id: Option<i64>,
    pub symbol_id: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageReport {
    pub stage: PipelineStage,
    pub status: StageStatus,
    pub execution_time_ms: u128,
    pub metrics: HashMap<String, f64>,
    pub findings: Vec<DiagnosticFinding>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineReport {
    pub stages: Vec<StageReport>,
}

pub trait PipelineValidator {
    /// Returns the stage of the pipeline this validator is responsible for.
    fn stage(&self) -> PipelineStage;
    
    /// Returns a list of stages that this validator depends on.
    /// If any of these prerequisite stages fail, this validator will be skipped.
    fn dependencies(&self) -> Vec<PipelineStage>;

    /// Runs the validation against the database and returns a StageReport.
    fn validate(&self, db: &Database) -> Result<StageReport>;
}
