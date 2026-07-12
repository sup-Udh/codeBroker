use std::path::Path;

/// Which embedding backend produces vectors. `Local` is the default: an
/// in-process ONNX model via fastembed, so an MCP server never ships source
/// code to a third-party API unless the user explicitly opts in via
/// `.codebroker/config.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingProvider {
    Local,
    OpenAi,
    Voyage,
}

#[derive(Debug, Clone)]
pub struct EmbeddingsConfig {
    pub provider: EmbeddingProvider,
    /// Model name as the user wrote it (e.g. "bge-small-en-v1.5",
    /// "text-embedding-3-small", "voyage-code-3").
    pub model: String,
    /// Name of the environment variable holding the API key (API providers
    /// only). The key itself is read from the environment at call time and
    /// never persisted anywhere.
    pub api_key_env: String,
}

pub const DEFAULT_LOCAL_MODEL: &str = "bge-small-en-v1.5";

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        EmbeddingsConfig {
            provider: EmbeddingProvider::Local,
            model: DEFAULT_LOCAL_MODEL.to_string(),
            api_key_env: String::new(),
        }
    }
}

impl EmbeddingsConfig {
    /// The identifier stored alongside each vector in the `embeddings` table.
    /// Prefixed with the provider so e.g. a hypothetical local model and an
    /// API model sharing a name can never be confused for one another.
    pub fn model_id(&self) -> String {
        match self.provider {
            EmbeddingProvider::Local => format!("local/{}", self.model),
            EmbeddingProvider::OpenAi => format!("openai/{}", self.model),
            EmbeddingProvider::Voyage => format!("voyage/{}", self.model),
        }
    }

    /// Loads `[embeddings]` from `<project_root>/.codebroker/config.toml`.
    /// A missing file, missing section, or unparseable TOML all yield the
    /// local default — configuration problems must degrade to "no API calls",
    /// never to "search is broken". An unknown `provider` value falls back to
    /// local for the same reason.
    pub fn load(project_root: &str) -> EmbeddingsConfig {
        let path = Path::new(project_root)
            .join(".codebroker")
            .join("config.toml");
        let Ok(raw) = std::fs::read_to_string(&path) else {
            return EmbeddingsConfig::default();
        };
        let Ok(value) = raw.parse::<toml::Value>() else {
            eprintln!(
                "[semantic] {} is not valid TOML; using default local embeddings",
                path.display()
            );
            return EmbeddingsConfig::default();
        };
        let Some(section) = value.get("embeddings") else {
            return EmbeddingsConfig::default();
        };

        let provider_str = section
            .get("provider")
            .and_then(|v| v.as_str())
            .unwrap_or("local")
            .to_lowercase();
        let provider = match provider_str.as_str() {
            "local" => EmbeddingProvider::Local,
            "openai" => EmbeddingProvider::OpenAi,
            "voyage" => EmbeddingProvider::Voyage,
            other => {
                eprintln!(
                    "[semantic] unknown embeddings provider '{}' in {}; using local",
                    other,
                    path.display()
                );
                EmbeddingProvider::Local
            }
        };

        let model = section
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| match provider {
                EmbeddingProvider::Local => DEFAULT_LOCAL_MODEL.to_string(),
                EmbeddingProvider::OpenAi => "text-embedding-3-small".to_string(),
                EmbeddingProvider::Voyage => "voyage-code-3".to_string(),
            });

        let api_key_env = section
            .get("api_key_env")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| match provider {
                EmbeddingProvider::OpenAi => "OPENAI_API_KEY".to_string(),
                EmbeddingProvider::Voyage => "VOYAGE_API_KEY".to_string(),
                EmbeddingProvider::Local => String::new(),
            });

        EmbeddingsConfig {
            provider,
            model,
            api_key_env,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(config_toml: Option<&str>) -> std::path::PathBuf {
        let unique = format!(
            "codebroker_test_config_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join(".codebroker")).unwrap();
        if let Some(toml) = config_toml {
            std::fs::write(root.join(".codebroker").join("config.toml"), toml).unwrap();
        }
        root
    }

    #[test]
    fn defaults_to_local_when_no_config() {
        let root = temp_root(None);
        let cfg = EmbeddingsConfig::load(root.to_str().unwrap());
        assert_eq!(cfg.provider, EmbeddingProvider::Local);
        assert_eq!(cfg.model, DEFAULT_LOCAL_MODEL);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn reads_api_provider_opt_in() {
        let root = temp_root(Some(
            "[embeddings]\nprovider = \"openai\"\nmodel = \"text-embedding-3-small\"\napi_key_env = \"MY_KEY\"\n",
        ));
        let cfg = EmbeddingsConfig::load(root.to_str().unwrap());
        assert_eq!(cfg.provider, EmbeddingProvider::OpenAi);
        assert_eq!(cfg.model_id(), "openai/text-embedding-3-small");
        assert_eq!(cfg.api_key_env, "MY_KEY");
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_provider_degrades_to_local_not_error() {
        let root = temp_root(Some("[embeddings]\nprovider = \"aliens\"\n"));
        let cfg = EmbeddingsConfig::load(root.to_str().unwrap());
        assert_eq!(cfg.provider, EmbeddingProvider::Local);
        std::fs::remove_dir_all(&root).ok();
    }
}
