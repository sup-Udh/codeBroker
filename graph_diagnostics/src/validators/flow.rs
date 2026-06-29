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

        use indexer::resolver::index::SymbolIndex;
        use indexer::resolver::import_resolver::ImportResolver;
        use std::sync::Arc;
        
        let symbol_index = Arc::new(SymbolIndex::build(db).unwrap());
        let import_resolver = Arc::new(ImportResolver::build(db, &symbol_index).unwrap());
        let engine = VariableFlowEngine::new(db, symbol_index, import_resolver);
        
        let mut total_variables = 0;
        let mut variables_with_type = 0;
        
        let mut alias_flow = 0;
        let mut property_flow = 0;
        let mut return_flow = 0;
        let mut constructor_flow = 0;
        let mut import_flow = 0;
        
        for file_map in engine.variables.values() {
            total_variables += file_map.len();
            for var in file_map.values() {
                if var.inferred_type.is_some() {
                    variables_with_type += 1;
                    for evidence in &var.evidence {
                        match evidence {
                            graph::models::ResolutionEvidence::Alias => alias_flow += 1,
                            graph::models::ResolutionEvidence::FieldType | graph::models::ResolutionEvidence::Destructuring | graph::models::ResolutionEvidence::ObjectLiteral => property_flow += 1,
                            graph::models::ResolutionEvidence::ReturnFlow => return_flow += 1,
                            graph::models::ResolutionEvidence::ConstructorCall => constructor_flow += 1,
                            graph::models::ResolutionEvidence::ImportFlow => import_flow += 1,
                            _ => {}
                        }
                    }
                }
            }
        }
        
        metrics.insert("Total Variables Tracked".to_string(), total_variables as f64);
        metrics.insert("Variables with Inferred Type".to_string(), variables_with_type as f64);
        metrics.insert("Alias Flow".to_string(), alias_flow as f64);
        metrics.insert("Property Flow".to_string(), property_flow as f64);
        metrics.insert("Return Flow".to_string(), return_flow as f64);
        metrics.insert("Constructor Flow".to_string(), constructor_flow as f64);
        metrics.insert("Import Flow".to_string(), import_flow as f64);
        
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
