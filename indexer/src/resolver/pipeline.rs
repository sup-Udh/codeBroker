use crate::resolver::context::ResolutionContext;
use crate::resolver::stages::ResolutionStage;
use crate::resolver::assertions::PipelineAssertions;

pub struct ResolutionPipeline {
    stages: Vec<Box<dyn ResolutionStage>>,
}

impl ResolutionPipeline {
    pub fn new(stages: Vec<Box<dyn ResolutionStage>>) -> Self {
        Self { stages }
    }

    pub fn execute(&self, mut context: ResolutionContext) -> Result<ResolutionContext, String> {
        let expected_stages = self.stages.len();
        
        for stage in &self.stages {
            if context.resolved {
                // Stage skipped due to early resolution
                context.skip_stage(stage.stage_type());
                continue;
            }
            
            stage.execute(&mut context)?;
        }
        
        if !context.resolved {
            panic!("Pipeline completed but context is not resolved. A terminal state must be assigned via resolve_with().");
        }
        
        PipelineAssertions::assert_terminal_state(&context);
        PipelineAssertions::assert_pipeline_complete(&context, expected_stages);
        
        Ok(context)
    }
}
