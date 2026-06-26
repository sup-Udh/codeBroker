use crate::provider::LlmProvider;
use reqwest::blocking::Client;
use serde_json::json;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// `gpt-4o-mini` — OpenAI's small/cheap chat model. CodeBroker's prompts
/// (impact analysis, patch generation, subsystem overviews) are mostly
/// "summarize this deterministic graph/code dump", which doesn't need a
/// frontier model; mini keeps per-call token cost down without giving up
/// usable output quality.
const DEFAULT_MODEL: &str = "gpt-4o-mini";

/// `text-embedding-3-small` — OpenAI's cheapest embedding model ($0.02 per 1M
/// tokens as of writing). CodeBroker's per-symbol embedding text is a short
/// one-liner (kind, name, signature), so even a full-repo re-embed of a few
/// thousand symbols costs a small fraction of a cent; the larger
/// `text-embedding-3-large` buys more precision than this use case needs.
const DEFAULT_EMBEDDING_MODEL: &str = "text-embedding-3-small";

pub struct OpenAiProvider {
    api_key: String,
    model_id: String,
    consecutive_failures: AtomicUsize,
    cooldown_until: AtomicI64,
    client: Client,
}

impl OpenAiProvider {
    pub fn new(api_key: String) -> Self {
        // Force HTTP/1.1. reqwest negotiates HTTP/2 via TLS ALPN by default,
        // and on networks behind certain transparent proxies/middleboxes the
        // TLS handshake and POST request both complete fine but the HTTP/2
        // response never arrives — the request hangs until timeout with 0
        // bytes received (reproduced with plain `curl` against this exact
        // endpoint: default curl over HTTP/2 hangs, `curl --http1.1`
        // succeeds immediately). HTTP/1.1 avoids that failure mode entirely
        // and OpenAI's API works identically over it.
        //
        // Disable automatic gzip/deflate/brotli decompression. When reqwest
        // decompresses a gzip stream and the connection drops mid-transfer, the
        // decompressor reports "error decoding response body" instead of a plain
        // network error — masking the real cause and causing the full 120-second
        // timeout to expire before the error surfaces. Without auto-decompression
        // the server sends plain JSON (OpenAI never compresses small API
        // responses in practice), response.bytes() returns raw JSON, and any
        // truncation produces a clear network error that retries can recover from.
        let client = Client::builder()
            .http1_only()
            .timeout(Duration::from_secs(60))
            .gzip(false)
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            api_key,
            model_id: DEFAULT_MODEL.to_string(),
            consecutive_failures: AtomicUsize::new(0),
            cooldown_until: AtomicI64::new(0),
            client,
        }
    }

    fn check_circuit_breaker(&self) -> Result<(), String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;
        let cooldown = self.cooldown_until.load(Ordering::SeqCst);
        if now < cooldown {
            return Err(format!(
                "Circuit breaker open: cooldown for {} more seconds",
                cooldown - now
            ));
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
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;
            self.cooldown_until.store(now + 60, Ordering::SeqCst); // 60s cooldown
        }
    }

    /// Shared call path for both `generate_summary` and `expand_query` — same
    /// request/response shape, just a different prompt, temperature, and
    /// max_tokens. OpenAI's chat completions response shape matches the one
    /// HuggingFaceProvider was already parsing (choices[0].message.content,
    /// usage.total_tokens), so this mirrors that implementation.
    fn chat_completion(
        &self,
        prompt: &str,
        timeout_secs: u64,
        max_tokens: usize,
        temperature: f64,
    ) -> Result<(String, usize), String> {
        self.check_circuit_breaker()?;

        let url = "https://api.openai.com/v1/chat/completions";

        let payload = json!({
            "model": self.model_id,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "max_tokens": max_tokens,
            "temperature": temperature
        });

        let response = match self
            .client
            .post(url)
            .timeout(Duration::from_secs(timeout_secs))
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                self.record_failure();
                return Err(format!("HTTP request failed: {}", e));
            }
        };

        // Use bytes() + from_slice() instead of text() + from_str() to skip
        // charset detection entirely. This is more efficient for large payloads
        // and avoids "error decoding response body" when a proxy sends a
        // Content-Encoding: gzip response that reqwest hasn't yet decompressed
        // (raw gzip bytes are not valid UTF-8).
        let bytes = match response.bytes() {
            Ok(b) => b,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to read body: {}", e));
            }
        };

        let json_resp: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(j) => j,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to parse JSON: {}", e));
            }
        };

        // OpenAI returns errors as a 200-shaped body with an "error" object
        // rather than always using HTTP status, so check for that explicitly
        // instead of only trusting "choices" being absent.
        if let Some(err) = json_resp.get("error") {
            self.record_failure();
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(format!("OpenAI API error: {}", msg));
        }

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
            return Err(format!(
                "Could not extract message content. Raw response: {}",
                String::from_utf8_lossy(&bytes)
            ));
        }

        self.record_success();
        Ok((final_text, token_count))
    }

    fn embeddings_request(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.check_circuit_breaker()?;

        if texts.is_empty() {
            return Ok(vec![]);
        }

        let url = "https://api.openai.com/v1/embeddings";
        let payload = json!({
            "model": DEFAULT_EMBEDDING_MODEL,
            "input": texts,
        });

        let response = match self
            .client
            .post(url)
            .timeout(Duration::from_secs(30))
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
        {
            Ok(r) => r,
            Err(e) => {
                self.record_failure();
                return Err(format!("HTTP request failed: {}", e));
            }
        };

        let bytes = match response.bytes() {
            Ok(b) => b,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to read body: {}", e));
            }
        };

        let json_resp: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(j) => j,
            Err(e) => {
                self.record_failure();
                return Err(format!("Failed to parse JSON: {}", e));
            }
        };

        if let Some(err) = json_resp.get("error") {
            self.record_failure();
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error");
            return Err(format!("OpenAI API error: {}", msg));
        }

        let data = match json_resp.get("data").and_then(|d| d.as_array()) {
            Some(d) => d,
            None => {
                self.record_failure();
                return Err(format!(
                    "Malformed embeddings response: {}",
                    String::from_utf8_lossy(&bytes)
                ));
            }
        };

        // The API is documented to return entries in input order, but each
        // entry also carries its own "index" — sort by it explicitly rather
        // than trusting array order, since silently mismatching a vector to
        // the wrong input text would corrupt every similarity score computed
        // against it with no visible error.
        let mut indexed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
        for entry in data {
            let index = entry.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
            let embedding: Vec<f32> = entry
                .get("embedding")
                .and_then(|e| e.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_f64().map(|f| f as f32))
                        .collect()
                })
                .unwrap_or_default();
            indexed.push((index, embedding));
        }
        indexed.sort_by_key(|(i, _)| *i);

        if indexed.len() != texts.len() {
            self.record_failure();
            return Err(format!(
                "Expected {} embeddings, got {}",
                texts.len(),
                indexed.len()
            ));
        }

        self.record_success();
        Ok(indexed.into_iter().map(|(_, v)| v).collect())
    }
}

impl LlmProvider for OpenAiProvider {
    fn model_name(&self) -> &str {
        &self.model_id
    }

    fn embed_texts(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.embeddings_request(texts)
    }

    fn embedding_model_name(&self) -> &str {
        DEFAULT_EMBEDDING_MODEL
    }

    fn generate_summary(
        &self,
        prompt: &str,
        timeout_secs: u64,
        max_tokens: usize,
    ) -> Result<(String, usize), String> {
        self.chat_completion(prompt, timeout_secs, max_tokens, 0.3)
    }

    fn expand_query(
        &self,
        keyword: &str,
        timeout_secs: u64,
    ) -> Result<(Vec<String>, usize), String> {
        let prompt = format!(
            "Return ONLY valid JSON.\n\n{{\n  \"tokens\": [\n    \"database\",\n    \"pool\",\n    \"postgres\",\n    \"sql\"\n  ]\n}}\n\nThe user is searching a codebase for the concept: '{}'. Return exactly 10 technical synonyms, related terms, struct names, or variable names. No prose. No markdown. No explanation.",
            keyword
        );

        let (final_text, token_count) = self.chat_completion(&prompt, timeout_secs, 100, 0.1)?;

        #[derive(serde::Deserialize)]
        struct QueryExpansionResponse {
            #[serde(alias = "synonyms")]
            tokens: Vec<String>,
        }

        let clean_text = final_text.trim();
        let clean_text = if clean_text.starts_with("```json") {
            clean_text
                .trim_start_matches("```json")
                .trim_end_matches("```")
                .trim()
        } else if clean_text.starts_with("```") {
            clean_text
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim()
        } else {
            clean_text
        };

        match serde_json::from_str::<QueryExpansionResponse>(clean_text) {
            Ok(parsed) => {
                let synonyms = parsed
                    .tokens
                    .into_iter()
                    .map(|s| s.to_lowercase())
                    .collect();
                Ok((synonyms, token_count))
            }
            Err(_) => {
                self.record_failure();
                Err(format!("Malformed JSON response: {}", clean_text))
            }
        }
    }
}
