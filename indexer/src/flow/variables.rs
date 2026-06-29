use crate::semantic::evidence::ResolutionConfidence;
use graph::models::{ResolutionEvidence, VariableOrigin};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct VariableState {
    pub file_id: i64,
    pub name: String,
    pub inferred_type: Option<String>,
    pub origin: VariableOrigin,
    pub confidence: ResolutionConfidence,
    pub evidence: Vec<ResolutionEvidence>,
    pub fields: HashMap<String, VariableState>,
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
            fields: HashMap::new(),
        }
    }

    pub fn get_or_create_field(&mut self, name: &str) -> &mut VariableState {
        self.fields
            .entry(name.to_string())
            .or_insert_with(|| VariableState::new(self.file_id, name.to_string()))
    }
    
    pub fn get_field(&self, name: &str) -> Option<&VariableState> {
        self.fields.get(name)
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
