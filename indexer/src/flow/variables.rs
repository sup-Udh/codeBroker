use crate::semantic::evidence::ResolutionConfidence;
use graph::models::{ResolutionEvidence, VariableOrigin};

#[derive(Debug, Clone)]
pub struct VariableState {
    pub file_id: i64,
    pub name: String,
    pub inferred_type: Option<String>,
    pub origin: VariableOrigin,
    pub confidence: ResolutionConfidence,
    pub evidence: Vec<ResolutionEvidence>,
}

impl VariableState {
    pub fn new(file_id: i64, name: String) -> Self {
        Self {
            file_id,
            name,
            inferred_type: None,
            origin: VariableOrigin::Unknown,
            confidence: ResolutionConfidence::Low,
            evidence: Vec::new(),
        }
    }

    pub fn apply_type(&mut self, type_name: String, origin: VariableOrigin, confidence: ResolutionConfidence, ev: ResolutionEvidence) {
        if self.confidence < confidence || self.inferred_type.is_none() {
            self.inferred_type = Some(type_name);
            self.origin = origin;
            self.confidence = confidence;
        }
        self.evidence.push(ev);
    }
}
