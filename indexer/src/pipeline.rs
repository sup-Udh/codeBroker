pub trait PipelineStage {
    type Input;
    type Output;

    fn execute(&self, input: Self::Input) -> Result<Self::Output, String>;
}
