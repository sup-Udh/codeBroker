use crate::provider::LlmProvider;
use reqwest::blocking::Client;
use serde_json::json;

pub struct HuggingFaceProvider {
    api_token: String,
    model_id: String,
}

impl HuggingFaceProvider {
    pub fn new(api_token: String) -> Self {
        Self {
            api_token,
            model_id: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(), 
        }
    }
}

impl LlmProvider for HuggingFaceProvider {
    fn model_name(&self) -> &str {
        &self.model_id
    }
    
    fn generate_summary(&self, prompt: &str) -> Result<(String, usize), String> {
        let url = "https://router.huggingface.co/v1/chat/completions";
        
        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": 2048,
            "temperature": 0.3
        });
        
        let client = Client::new();
        let response = client
            .post(url)
            .bearer_auth(&self.api_token)
            .json(&payload)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;
            
        let text = response.text()
            .map_err(|e| format!("Failed to read body: {}", e))?;
            
        let json_resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
            
        let mut final_text = String::new();
        let mut token_count = 0;

        // 1. Extract Text from OpenAI-compatible response format
        if let Some(choices) = json_resp.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.get(0) {
                if let Some(message) = first_choice.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        final_text = content.to_string();
                    }
                }
            }
        }
        
        // 2. Extract Token Usage
        if let Some(usage) = json_resp.get("usage") {
            if let Some(total) = usage.get("total_tokens").and_then(|t| t.as_u64()) {
                token_count = total as usize;
            }
        }
        
        if final_text.is_empty() {
            return Err(format!("Could not extract generated_text. Raw response: {}", text));
        }

        Ok((final_text, token_count))
    }

    fn expand_query(&self, keyword: &str) -> Result<(Vec<String>, usize), String> {
        let prompt = format!(
            "You are a coding assistant. The user is searching a codebase for the concept: '{}'. 
Return a simple, comma-separated list of exactly 10 technical synonyms, related terms, struct names, or variable names that might exist in a codebase for this concept. 
Do not include any other text, explanations, or quotes. Only the comma-separated words.",
            keyword
        );

        let url = "https://router.huggingface.co/v1/chat/completions";
        
        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": 100,
            "temperature": 0.1
        });
        
        let client = Client::new();
        let response = client
            .post(url)
            .bearer_auth(&self.api_token)
            .json(&payload)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;
            
        let text = response.text()
            .map_err(|e| format!("Failed to read body: {}", e))?;
            
        let json_resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
            
        let mut final_text = String::new();
        let mut token_count = 0;

        if let Some(choices) = json_resp.get("choices").and_then(|c| c.as_array()) {
            if let Some(first_choice) = choices.get(0) {
                if let Some(message) = first_choice.get("message") {
                    if let Some(content) = message.get("content").and_then(|c| c.as_str()) {
                        final_text = content.to_string();
                    }
                }
            }
        }
        
        if let Some(usage) = json_resp.get("usage") {
            if let Some(total) = usage.get("total_tokens").and_then(|t| t.as_u64()) {
                token_count = total as usize;
            }
        }
        
        let synonyms: Vec<String> = final_text
            .split(',')
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();

        Ok((synonyms, token_count))
    }
}