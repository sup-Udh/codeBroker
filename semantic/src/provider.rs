pub trait LlmProvider {

    // returning the fully assemblemd prompt using this function
    fn generate_summary(&self, prompt: &str) -> Result<String , String>;

    fn model_name(&self) -> &str;

    


}

pub struct MockProvider;

impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "offline/mock-model"
    }

    fn generate_summary(&self, prompt: &str) -> Result<String, String> {
        Ok(format!("(Simulated AI Response)\n\nI have analyzed the prompt. The target symbol is critical to the architecture. Here is the prompt I received:\n\n---\n{}...", &prompt[..150]))
    }
}