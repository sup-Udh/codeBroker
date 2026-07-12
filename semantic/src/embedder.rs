use crate::config::{EmbeddingProvider, EmbeddingsConfig};
use std::sync::Mutex;

/// A batch text-embedding backend. Implementations must be usable from a
/// long-lived MCP server: cheap to construct (any heavy model load happens
/// lazily on the first `embed` call) and safe to call repeatedly.
pub trait Embedder: Send + Sync {
    /// Embeds `texts` in order; the returned vectors are index-aligned with
    /// the input. An error here means "semantic search is degraded", never a
    /// reason to fail keyword indexing or search.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String>;

    /// Identifier stored with every vector (see `EmbeddingsConfig::model_id`),
    /// so switching models invalidates old vectors instead of mixing them.
    fn model_id(&self) -> &str;

    /// Vector dimensionality (e.g. 384 for bge-small-en-v1.5). 0 when not
    /// yet known (API providers before their first successful call).
    fn dims(&self) -> usize;
}

/// Builds the embedder the workspace's config asks for. Never performs
/// network or model I/O itself — failures surface on first `embed`.
pub fn embedder_from_config(config: &EmbeddingsConfig) -> Result<Box<dyn Embedder>, String> {
    match config.provider {
        EmbeddingProvider::Local => Ok(Box::new(LocalEmbedder::new(config)?)),
        EmbeddingProvider::OpenAi | EmbeddingProvider::Voyage => {
            Ok(Box::new(ApiEmbedder::new(config)?))
        }
    }
}

// ---------------------------------------------------------------------------
// LocalEmbedder — fastembed / ONNX, CPU, in-process. The default backend.
// ---------------------------------------------------------------------------

/// Where local model files live. Downloaded once by fastembed's hf-hub
/// integration on first use, then loaded from disk on every later run.
fn models_cache_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&home).join(".codebroker").join("models")
}

/// Maps a config model string like "bge-small-en-v1.5" onto fastembed's
/// `EmbeddingModel` variant (whose Debug name is e.g. "BGESmallENV15") by
/// comparing alphanumerics case-insensitively, so users write the familiar
/// HuggingFace-style name rather than a Rust enum variant.
fn parse_local_model(name: &str) -> Result<fastembed::EmbeddingModel, String> {
    let normalize = |s: &str| -> String {
        s.chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect::<String>()
            .to_lowercase()
    };
    let wanted = normalize(name);
    fastembed::TextEmbedding::list_supported_models()
        .into_iter()
        .find(|info| {
            normalize(&format!("{:?}", info.model)) == wanted
                || normalize(&info.model_code) == wanted
                || normalize(&info.model_code.split('/').next_back().unwrap_or("")) == wanted
        })
        .map(|info| info.model)
        .ok_or_else(|| format!("unknown local embedding model '{}'", name))
}

pub struct LocalEmbedder {
    model_id: String,
    model: fastembed::EmbeddingModel,
    dims: usize,
    /// Lazily initialized on first `embed`: loading (and on the very first
    /// run, downloading) the ONNX model takes seconds, and most tool calls
    /// never need it. A failed load is returned as the degradation reason
    /// and left `None`, so the next call retries — the failure may have been
    /// transient (e.g. network during the one-time download).
    state: Mutex<Option<fastembed::TextEmbedding>>,
}

impl LocalEmbedder {
    pub fn new(config: &EmbeddingsConfig) -> Result<Self, String> {
        let model = parse_local_model(&config.model)?;
        let dims = fastembed::TextEmbedding::list_supported_models()
            .into_iter()
            .find(|info| info.model == model)
            .map(|info| info.dim)
            .unwrap_or(0);
        Ok(LocalEmbedder {
            model_id: config.model_id(),
            model,
            dims,
            state: Mutex::new(None),
        })
    }
}

impl Embedder for LocalEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut guard = self
            .state
            .lock()
            .map_err(|_| "local embedder poisoned by a previous panic".to_string())?;
        if guard.is_none() {
            let cache_dir = models_cache_dir();
            let _ = std::fs::create_dir_all(&cache_dir);
            // Progress bars are suppressed: the MCP transport is JSON-RPC on
            // stdout and must never be interleaved with terminal output.
            let options = fastembed::InitOptions::new(self.model.clone())
                .with_cache_dir(cache_dir)
                .with_show_download_progress(false);
            let loaded = fastembed::TextEmbedding::try_new(options).map_err(|e| {
                format!(
                    "failed to load local embedding model {:?} (first use downloads it to {}): {}",
                    self.model,
                    models_cache_dir().display(),
                    e
                )
            })?;
            *guard = Some(loaded);
        }
        let model = guard.as_mut().expect("just initialized");
        model
            .embed(texts, None)
            .map_err(|e| format!("local embedding failed: {}", e))
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dims(&self) -> usize {
        self.dims
    }
}

// ---------------------------------------------------------------------------
// ApiEmbedder — OpenAI / Voyage, opt-in via config only.
// ---------------------------------------------------------------------------

/// Keeps each API request to a reasonable payload size — both providers
/// accept more per call, but batching too aggressively makes one transient
/// failure cost a whole repo's worth of retries instead of one chunk's.
const API_BATCH_SIZE: usize = 100;
const API_MAX_ATTEMPTS: u32 = 3;

pub struct ApiEmbedder {
    model_id: String,
    model: String,
    endpoint: &'static str,
    api_key_env: String,
    /// Learned from the first successful response; both providers' dims are
    /// model-dependent and not worth hardcoding a table for.
    dims: std::sync::atomic::AtomicUsize,
}

impl ApiEmbedder {
    pub fn new(config: &EmbeddingsConfig) -> Result<Self, String> {
        if config.api_key_env.is_empty() {
            return Err("api_key_env is not set in [embeddings] config".to_string());
        }
        let endpoint = match config.provider {
            EmbeddingProvider::OpenAi => "https://api.openai.com/v1/embeddings",
            EmbeddingProvider::Voyage => "https://api.voyageai.com/v1/embeddings",
            EmbeddingProvider::Local => return Err("ApiEmbedder requires an API provider".into()),
        };
        Ok(ApiEmbedder {
            model_id: config.model_id(),
            model: config.model.clone(),
            endpoint,
            api_key_env: config.api_key_env.clone(),
            dims: std::sync::atomic::AtomicUsize::new(0),
        })
    }

    /// One POST for one batch, with retry/backoff on 429 and 5xx. Both
    /// OpenAI and Voyage share the same request/response shape:
    /// `{model, input: [...]}` → `{data: [{index, embedding: [...]}]}`.
    fn embed_batch(&self, key: &str, batch: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .map_err(|e| e.to_string())?;
        let body = serde_json::json!({ "model": self.model, "input": batch });

        let mut last_err = String::new();
        for attempt in 0..API_MAX_ATTEMPTS {
            if attempt > 0 {
                std::thread::sleep(std::time::Duration::from_millis(500 * (1 << (attempt - 1))));
            }
            let response = client
                .post(self.endpoint)
                .bearer_auth(key)
                .json(&body)
                .send();
            let response = match response {
                Ok(r) => r,
                Err(e) => {
                    last_err = format!("request failed: {}", e);
                    continue;
                }
            };
            let status = response.status();
            if status.as_u16() == 429 || status.is_server_error() {
                last_err = format!("{} returned {}", self.endpoint, status);
                continue;
            }
            if !status.is_success() {
                // 4xx other than 429 (bad key, bad model) won't improve with
                // retries — fail immediately with the body for diagnosis.
                let text = response.text().unwrap_or_default();
                return Err(format!("{} returned {}: {}", self.endpoint, status, text));
            }
            let parsed: serde_json::Value = response.json().map_err(|e| e.to_string())?;
            let data = parsed
                .get("data")
                .and_then(|d| d.as_array())
                .ok_or_else(|| "malformed embeddings response: no data array".to_string())?;
            let mut indexed: Vec<(usize, Vec<f32>)> = Vec::with_capacity(data.len());
            for item in data {
                let index = item.get("index").and_then(|i| i.as_u64()).unwrap_or(0) as usize;
                let vector: Vec<f32> = item
                    .get("embedding")
                    .and_then(|e| e.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_f64()).map(|f| f as f32).collect())
                    .unwrap_or_default();
                indexed.push((index, vector));
            }
            indexed.sort_by_key(|(i, _)| *i);
            let vectors: Vec<Vec<f32>> = indexed.into_iter().map(|(_, v)| v).collect();
            if vectors.len() != batch.len() {
                return Err(format!(
                    "embeddings response count mismatch: sent {}, got {}",
                    batch.len(),
                    vectors.len()
                ));
            }
            if let Some(first) = vectors.first() {
                self.dims
                    .store(first.len(), std::sync::atomic::Ordering::Relaxed);
            }
            return Ok(vectors);
        }
        Err(format!(
            "embeddings request failed after {} attempts: {}",
            API_MAX_ATTEMPTS, last_err
        ))
    }
}

impl Embedder for ApiEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        // Read the key from the named env var at call time; it is never
        // stored on the struct or persisted anywhere.
        let key = std::env::var(&self.api_key_env)
            .ok()
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                format!(
                    "environment variable {} (from [embeddings].api_key_env) is not set",
                    self.api_key_env
                )
            })?;
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(API_BATCH_SIZE) {
            out.extend(self.embed_batch(&key, batch)?);
        }
        Ok(out)
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    fn dims(&self) -> usize {
        self.dims.load(std::sync::atomic::Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------------------
// MockEmbedder — deterministic, offline, for tests.
// ---------------------------------------------------------------------------

/// Deterministic embedder for tests: the vector is a token-hash bag, so
/// identical texts embed identically and share tokens overlap somewhat —
/// enough to test storage, incrementality, and fusion plumbing without any
/// model or network. Also counts every text actually embedded, which is what
/// the body_hash-skip tests assert on.
pub struct MockEmbedder {
    pub calls: std::sync::atomic::AtomicUsize,
    pub texts_embedded: std::sync::atomic::AtomicUsize,
}

pub const MOCK_MODEL_ID: &str = "mock/deterministic-8d";

impl MockEmbedder {
    pub fn new() -> Self {
        MockEmbedder {
            calls: std::sync::atomic::AtomicUsize::new(0),
            texts_embedded: std::sync::atomic::AtomicUsize::new(0),
        }
    }
}

impl Default for MockEmbedder {
    fn default() -> Self {
        Self::new()
    }
}

impl Embedder for MockEmbedder {
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        use std::hash::{Hash, Hasher};
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.texts_embedded
            .fetch_add(texts.len(), std::sync::atomic::Ordering::SeqCst);
        Ok(texts
            .iter()
            .map(|t| {
                let mut v = [0f32; 8];
                for token in t.split(|c: char| !c.is_ascii_alphanumeric()) {
                    if token.is_empty() {
                        continue;
                    }
                    let mut h = std::collections::hash_map::DefaultHasher::new();
                    token.to_lowercase().hash(&mut h);
                    let d = h.finish();
                    v[(d % 8) as usize] += 1.0;
                }
                v.to_vec()
            })
            .collect())
    }

    fn model_id(&self) -> &str {
        MOCK_MODEL_ID
    }

    fn dims(&self) -> usize {
        8
    }
}
