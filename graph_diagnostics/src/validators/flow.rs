use rusqlite::Result;
use storage::Database;
use std::collections::HashMap;
use crate::traits::{DiagnosticFinding, PipelineValidator, PipelineStage, StageReport, StageStatus};
use std::time::Instant;
use indexer::flow::VariableFlowEngine;

pub struct FlowValidator;

impl PipelineValidator for FlowValidator {
    fn stage(&self) -> PipelineStage {
        PipelineStage::Flow
    }

    fn dependencies(&self) -> Vec<PipelineStage> {
        vec![PipelineStage::Semantic]
    }

    fn validate(&self, db: &Database) -> Result<StageReport> {
        let start = Instant::now();
        let findings = Vec::new();
        let mut metrics = HashMap::new();

        // Initialize engine to extract metrics
        let engine = VariableFlowEngine::new(db);
        
        let mut total_variables = 0;
        let mut variables_with_type = 0;
        
        for file_map in engine.variables.values() {
            total_variables += file_map.len();
            for var in file_map.values() {
                if var.inferred_type.is_some() {
                    variables_with_type += 1;
                }
            }
        }
        
        metrics.insert("Total Variables Tracked".to_string(), total_variables as f64);
        metrics.insert("Variables with Inferred Type".to_string(), variables_with_type as f64);
        
        let success_rate = if total_variables > 0 {
            (variables_with_type as f64 / total_variables as f64) * 100.0
        } else {
            0.0
        };
        metrics.insert("Flow Resolution Success (%)".to_string(), success_rate);

        let status = if total_variables > 0 && success_rate < 5.0 { 
            StageStatus::Warning 
        } else { 
            StageStatus::Pass 
        };

        Ok(StageReport {
            stage: self.stage(),
            status,
            execution_time_ms: start.elapsed().as_millis(),
            metrics,
            findings,
        })
    }
}
