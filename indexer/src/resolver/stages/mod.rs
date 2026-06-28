pub mod classification;
pub mod receiver;
pub mod generation;
pub mod filtering;
pub mod ranking;

use crate::resolver::context::ResolutionContext;
use crate::resolver::decisions::PipelineStageType;

pub trait ResolutionStage: Send + Sync {
    fn name(&self) -> &'static str;
    fn stage_type(&self) -> PipelineStageType;
    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String>;
}
