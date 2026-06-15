use crate::provider::LlmProvider;

use ureq;
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
            model_id: "meta-llama/Meta-Llama-3-8B-Instruct".to_string(), 
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
        // Make the synchronous HTTP request
                // Make the synchronous HTTP request
        let response = ureq::post(&url)
            .header("Authorization", &format!("Bearer {}", self.api_token))
            .header("Content-Type", "application/json")
            .send_json(payload)
            .map_err(|e| format!("HTTP request failed: {}", e))?;
        // Parse the response
        let mut response = response;
        let text = response.body_mut().read_to_string()
            .map_err(|e| format!("Failed to read body: {}", e))?;
        let json_resp: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("Failed to parse JSON: {}", e))?;
        // Extract the generated text from the Hugging Face response array
        if let Some(arr) = json_resp.as_array() {
            if let Some(obj) = arr.get(0) {
                if let Some(text) = obj.get("generated_text").and_then(|t| t.as_str()) {
                    // HF often returns the prompt inside the response. We return everything.
                    return Ok(text.to_string());
                }
            }
        }
        Err("Could not extract generated_text from response".to_string())
    }
}