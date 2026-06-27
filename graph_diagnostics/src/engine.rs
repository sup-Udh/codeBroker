use rusqlite::Result;
use storage::Database;
use std::collections::HashSet;

use crate::traits::{PipelineReport, PipelineValidator, PipelineStage, StageStatus, StageReport};
use crate::validators::{
    parser::ParserValidator,
    semantic::SemanticValidator,
    flow::FlowValidator,
    receiver::ReceiverValidator,
    method::MethodValidator,
    resolver::ResolverValidator,
    graph::GraphValidatorObj,
    feature_extraction::FeatureExtractionValidator,
    semantic_index::SemanticIndexValidator,
    retrieval::RetrievalValidator,
    consistency::ConsistencyValidator,
};

pub fn run_diagnostics(db: &Database) -> Result<PipelineReport> {
    let validators: Vec<Box<dyn PipelineValidator>> = vec![
        Box::new(ParserValidator),
        Box::new(SemanticValidator),
        Box::new(FlowValidator),
        Box::new(ReceiverValidator),
        Box::new(MethodValidator),
        Box::new(ResolverValidator),
        Box::new(GraphValidatorObj),
        Box::new(FeatureExtractionValidator),
        Box::new(SemanticIndexValidator),
        Box::new(RetrievalValidator),
        Box::new(ConsistencyValidator),
    ];

    let mut stages = Vec::new();
    let mut failed_stages = HashSet::new();

    for validator in validators {
        let stage = validator.stage();
        let deps = validator.dependencies();
        
        let mut skipped = false;
        for dep in deps {
            if failed_stages.contains(&dep) {
                skipped = true;
                break;
            }
        }

        if skipped {
            stages.push(StageReport {
                stage,
                status: StageStatus::Skipped,
                execution_time_ms: 0,
                metrics: Default::default(),
                findings: vec![],
            });
            failed_stages.insert(stage);
            continue;
        }

        match validator.validate(db) {
            Ok(report) => {
                if report.status == StageStatus::Fail {
                    failed_stages.insert(stage);
                }
                stages.push(report);
            }
            Err(e) => {
                eprintln!("Error running validator {:?}: {}", stage, e);
                failed_stages.insert(stage);
                stages.push(StageReport {
                    stage,
                    status: StageStatus::Fail,
                    execution_time_ms: 0,
                    metrics: Default::default(),
                    findings: vec![],
                });
            }
        }
    }

    Ok(PipelineReport { stages })
}
