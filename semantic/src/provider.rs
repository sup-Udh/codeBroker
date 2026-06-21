pub trait LlmProvider {
    /// Returns the unique ID of the model (e.g. 'Qwen/Qwen2.5-Coder-32B-Instruct')
    fn model_name(&self) -> &str;
    
    /// Generates a semantic summary of the code and returns (Summary, TokenCount)
    fn generate_summary(&self, prompt: &str, timeout_secs: u64) -> Result<(String, usize), String>;

    /// Expands a conceptual query into a list of technical synonyms
    fn expand_query(&self, keyword: &str, timeout_secs: u64) -> Result<(Vec<String>, usize), String>;
}

// Don't forget to update the MockProvider so it compiles!
pub struct MockProvider;

impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "offline/mock-model"
    }

    fn generate_summary(&self, prompt: &str, _timeout_secs: u64) -> Result<(String, usize), String> {
        let fake_summary = format!("(Simulated AI Response)\n\nI have analyzed the prompt. The target symbol is critical to the architecture. Here is the prompt I received:\n\n---\n{}...", &prompt[..150]);
        // Simulate a response that used 142 tokens
        Ok((fake_summary, 142)) 
    }

    fn expand_query(&self, _keyword: &str, _timeout_secs: u64) -> Result<(Vec<String>, usize), String> {
        Ok((vec!["mock".to_string(), "synonym".to_string()], 10))
    }
}