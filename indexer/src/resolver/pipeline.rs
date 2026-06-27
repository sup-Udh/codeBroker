use crate::resolver::context::ResolutionContext;
use crate::resolver::stages::ResolutionStage;

pub struct ResolutionPipeline {
    stages: Vec<Box<dyn ResolutionStage>>,
}

impl ResolutionPipeline {
    pub fn new(stages: Vec<Box<dyn ResolutionStage>>) -> Self {
        Self { stages }
    }

    pub fn execute(&self, mut context: ResolutionContext) -> Result<ResolutionContext, String> {
        for stage in &self.stages {
            if context.resolved {
                break;
            }
            stage.execute(&mut context)?;
        }
        Ok(context)
    }
}
