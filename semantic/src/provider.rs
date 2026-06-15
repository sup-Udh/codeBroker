pub trait LlmProvider {

    // returning the fully assemblemd prompt using this function
    fn generate_summary(&self, prompt: &str) -> Result<String , String>;


}