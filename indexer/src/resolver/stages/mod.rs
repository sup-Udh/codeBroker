pub mod classification;
pub mod filtering;
pub mod generation;
pub mod ranking;
pub mod receiver;

use crate::resolver::context::ResolutionContext;

pub trait ResolutionStage {
    fn name(&self) -> &'static str;
    fn execute(&self, context: &mut ResolutionContext) -> Result<(), String>;
}
