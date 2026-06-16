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
            // model used (note: switch later)
            model_id: "Qwen/Qwen2.5-Coder-7B-Instruct".to_string(), 
        }
    }
}

impl LlmProvider for HuggingFaceProvider {
    fn model_name(&self) -> &str {
        &self.model_id
    }
    fn generate_summary(&self, prompt: &str) -> Result<String, String> {
        let url = format!("https://api-inference.huggingface.co/models/{}", self.model_id);
        
        let payload = json!({
            "inputs": prompt,
            "parameters": {
                "max_new_tokens": 500,
                "temperature": 0.3
            }
        });
        
        let client = Client::new();
        let response = client
            .post(&url)
            .bearer_auth(&self.api_token)
            .json(&payload)
            .send()
            .map_err(|e| format!("HTTP request failed: {}", e))?;
            
        let text = response.text()
            .map_err(|e| format!("Failed to read body: {}", e))?;
            
        let json_resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
            
        // Extract the generated text from the Hugging Face response array
        if let Some(arr) = json_resp.as_array() {
            if let Some(obj) = arr.get(0) {
                if let Some(text) = obj.get("generated_text").and_then(|t| t.as_str()) {
                    return Ok(text.to_string());
                }
            }
        }
        
        // If the array extraction fails, just return the raw JSON for debugging
        Err(format!("Could not extract generated_text. Raw response: {}", text))
    }
}