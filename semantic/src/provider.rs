pub trait LlmProvider {
    /// Returns the unique ID of the model (e.g. 'Qwen/Qwen2.5-Coder-32B-Instruct')
    fn model_name(&self) -> &str;

    /// Generates a semantic summary of the code and returns (Summary, TokenCount)
    fn generate_summary(
        &self,
        prompt: &str,
        timeout_secs: u64,
        max_tokens: usize,
    ) -> Result<(String, usize), String>;

    /// Expands a conceptual query into a list of technical synonyms
    fn expand_query(
        &self,
        keyword: &str,
        timeout_secs: u64,
    ) -> Result<(Vec<String>, usize), String>;

    /// Embeds a batch of texts into vectors for semantic similarity search.
    /// Returned vectors are in the same order as `texts`. Batched (rather
    /// than one call per text) because indexing a repo means embedding
    /// hundreds of symbols at once, and most embedding APIs (including
    /// OpenAI's) accept and bill an array input as a single request.
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;

    /// The identifier to store alongside a stored embedding (e.g.
    /// "text-embedding-3-small"), so a later model change can be detected and
    /// stale vectors re-embedded instead of silently mixing two models'
    /// vectors in the same similarity comparison.
    fn embedding_model_name(&self) -> &str;
}

// Don't forget to update the MockProvider so it compiles!
pub struct MockProvider;

impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "offline/mock-model"
    }

    fn generate_summary(
        &self,
        prompt: &str,
        _timeout_secs: u64,
        _max_tokens: usize,
    ) -> Result<(String, usize), String> {
        let fake_summary = format!(
            "(Simulated AI Response)\n\nI have analyzed the prompt. The target symbol is critical to the architecture. Here is the prompt I received:\n\n---\n{}...",
            &prompt[..150]
        );
        // Simulate a response that used 142 tokens
        Ok((fake_summary, 142))
    }

    fn expand_query(
        &self,
        _keyword: &str,
        _timeout_secs: u64,
    ) -> Result<(Vec<String>, usize), String> {
        Ok((vec!["mock".to_string(), "synonym".to_string()], 10))
    }

    /// Deterministic, hash-derived fake embedding (not a real semantic
    /// representation) — identical input text always produces the identical
    /// vector, and different text produces a different one, which is enough
    /// to exercise/test the storage and cosine-similarity plumbing without a
    /// network call. Do not expect this to cluster related concepts.
    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        use std::hash::{Hash, Hasher};
        const DIMS: usize = 16;
        Ok(texts
            .iter()
            .map(|text| {
                let mut vec = Vec::with_capacity(DIMS);
                for i in 0..DIMS {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    text.hash(&mut hasher);
                    i.hash(&mut hasher);
                    let h = hasher.finish();
                    // Map to roughly [-1.0, 1.0].
                    vec.push((h % 2000) as f32 / 1000.0 - 1.0);
                }
                vec
            })
            .collect())
    }

    fn embedding_model_name(&self) -> &str {
        "offline/mock-embedding"
    }
}
