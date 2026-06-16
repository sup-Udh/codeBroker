pub struct TokenAccounting; 

impl TokenAccounting {
    pub fn estimate_tokens(bytes: usize) -> usize {
        bytes / 4
    }


    pub fn calculate_savings(repo_bytes: usize, context_bytes: usize) -> usize {
        let repo_tokens = Self::estimate_tokens(repo_bytes);
        let context_tokens = Self::estimate_tokens(context_bytes);

        if repo_tokens > context_tokens {
            repo_tokens - context_tokens
        }else {
            0
        }
    }

}


// cost accounting

impl CostAccounting {
    /// Converts token savings into estimated US Cents. 
    /// Assumes a blended average cost of $3.00 per 1 Million input tokens.
    pub fn calculate_cents_saved(tokens_saved: usize) -> f64 {
        let cost_per_million_cents = 300.0;
        (tokens_saved as f64 / 1_000_000.0) * cost_per_million_cents
    }
}