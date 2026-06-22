use crate::provider::LlmProvider;
use reqwest::blocking::Client;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH, Duration};

pub struct HuggingFaceProvider {
    api_token: String,
    model_id: String,
    consecutive_failures: AtomicUsize,
    cooldown_until: AtomicI64,
}

impl HuggingFaceProvider {
    pub fn new(api_token: String) -> Self {
        Self {
            api_token,
            model_id: "Qwen/Qwen2.5-Coder-32B-Instruct".to_string(), 
            consecutive_failures: AtomicUsize::new(0),
            cooldown_until: AtomicI64::new(0),
        }
    }

    fn check_circuit_breaker(&self) -> Result<(), String> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
        let cooldown = self.cooldown_until.load(Ordering::SeqCst);
        if now < cooldown {
            return Err(format!("Circuit breaker open: cooldown for {} more seconds", cooldown - now));
        }
        Ok(())
    }

    fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::SeqCst);
        self.cooldown_until.store(0, Ordering::SeqCst);
    }

    fn record_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        if failures >= 3 {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64;
            self.cooldown_until.store(now + 60, Ordering::SeqCst); // 60s cooldown
        }
    }
}

impl LlmProvider for HuggingFaceProvider {
    fn model_name(&self) -> &str {
        &self.model_id
    }
    
    fn generate_summary(&self, prompt: &str, timeout_secs: u64, max_tokens: usize) -> Result<(String, usize), String> {
        self.check_circuit_breaker()?;

        let url = "https://router.huggingface.co/v1/chat/completions";
        
        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens,
            "temperature": 0.3
        });
        
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))?;

        let response = match client.post(url)
            .bearer_auth(&self.api_token)
            .json(&payload)
            .send() {
                Ok(r) => r,
                Err(e) => {
                    self.record_failure();
                    return Err(format!("HTTP request failed: {}", e));
                }
            };
            
        let text = match response.text() {
            Ok(t) => t,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to read body: {}", e));
            }
        };
            
        let json_resp: serde_json::Value = match serde_json::from_str(&text) {
            Ok(j) => j,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to parse JSON: {}", e));
            }
        };
            
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
        
        if final_text.is_empty() {
            self.record_failure();
            return Err(format!("Could not extract generated_text. Raw response: {}", text));
        }

        self.record_success();
        Ok((final_text, token_count))
    }

    fn expand_query(&self, keyword: &str, timeout_secs: u64) -> Result<(Vec<String>, usize), String> {
        self.check_circuit_breaker()?;

        let prompt = format!(
            "Return ONLY valid JSON.\n\n{{\n  \"tokens\": [\n    \"database\",\n    \"pool\",\n    \"postgres\",\n    \"sql\"\n  ]\n}}\n\nThe user is searching a codebase for the concept: '{}'. Return exactly 10 technical synonyms, related terms, struct names, or variable names. No prose. No markdown. No explanation.",
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
        
        let client = Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .build()
            .map_err(|e| format!("Failed to build client: {}", e))?;

        let response = match client.post(url)
            .bearer_auth(&self.api_token)
            .json(&payload)
            .send() {
                Ok(r) => r,
                Err(e) => {
                    self.record_failure();
                    return Err(format!("HTTP request failed: {}", e));
                }
            };
            
        let text = match response.text() {
            Ok(t) => t,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to read body: {}", e));
            }
        };
            
        let json_resp: serde_json::Value = match serde_json::from_str(&text) {
            Ok(j) => j,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to parse JSON: {}", e));
            }
        };
            
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
        
        #[derive(serde::Deserialize)]
        struct QueryExpansionResponse {
            tokens: Vec<String>
        }

        let clean_text = final_text.trim();
        let clean_text = if clean_text.starts_with("```json") {
            clean_text.trim_start_matches("```json").trim_end_matches("```").trim()
        } else if clean_text.starts_with("```") {
            clean_text.trim_start_matches("```").trim_end_matches("```").trim()
        } else {
            clean_text
        };

        match serde_json::from_str::<QueryExpansionResponse>(clean_text) {
            Ok(parsed) => {
                self.record_success();
                let synonyms = parsed.tokens.into_iter().map(|s| s.to_lowercase()).collect();
                Ok((synonyms, token_count))
            },
            Err(_) => {
                self.record_failure();
                Err(format!("Malformed JSON response: {}", clean_text))
            }
        }
    }
}
