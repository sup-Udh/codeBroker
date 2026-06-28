use graph::models::{ResolutionEvidence, ResolutionState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionTraceEntry {
    pub stage: String,
    pub status: String,
    pub evidence: Option<ResolutionEvidence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolutionTrace {
    pub events: Vec<ResolutionTraceEntry>,
}

impl ResolutionTrace {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn emit(&mut self, stage: &str, status: &str, evidence: Option<ResolutionEvidence>) {
        self.events.push(ResolutionTraceEntry {
            stage: stage.to_string(),
            status: status.to_string(),
            evidence,
        });
    }
}
