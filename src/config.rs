use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Top-level application configuration loaded from a JSONC file.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct AppConfig {
    /// Which provider to use by default (must match a key in `providers`).
    #[serde(default)]
    pub default_provider: String,

    /// Which model to use by default (must exist in the chosen provider's models).
    #[serde(default)]
    pub default_model: String,

    /// Named provider definitions.
    #[serde(default)]
    pub providers: HashMap<String, ProviderConfig>,
}

/// A single provider definition.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ProviderConfig {
    /// Human-readable name for display.
    pub name: String,

    /// Base URL for the OpenAI-compatible chat completions endpoint.
    /// e.g. "https://api.mistral.ai/v1/chat/completions"
    pub endpoint: String,

    /// Environment variable name that holds the API key.
    /// The key is read from the process environment (or .env loaded by the caller).
    pub env_key: String,

    /// Optional: override the API key directly (takes precedence over env_key).
    #[serde(default)]
    pub api_key: Option<String>,

    /// Models available on this provider.
    #[serde(default)]
    pub models: Vec<ModelConfig>,

    /// Optional: which model to use as default for this provider.
    #[serde(default)]
    pub default_model: Option<String>,
}

/// A model definition within a provider.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ModelConfig {
    /// The model ID sent to the API (e.g. "zai-glm-5-2", "glm-5.2").
    pub id: String,

    /// Human-readable display name.
    #[serde(default)]
    pub name: Option<String>,

    /// Whether this model supports thinking/reasoning.
    #[serde(default)]
    pub thinking: bool,

    /// If thinking is supported, the available effort levels.
    /// e.g. ["none", "minimal", "low", "medium", "high", "xhigh"]
    #[serde(default)]
    pub thinking_levels: Vec<String>,
}

impl AppConfig {
    /// Load config from `.axonerai/axonerai.jsonc` in the current directory,
    /// falling back to `~/.config/axonerai/axonerai.jsonc`.
    /// If neither exists, returns a built-in default config.
    pub fn load() -> Result<Self> {
        let candidates = Self::config_paths();
        for path in &candidates {
            if path.exists() {
                let raw = std::fs::read_to_string(path)
                    .with_context(|| format!("failed to read config: {}", path.display()))?;
                let json = strip_jsonc_comments(&raw);
                let config: AppConfig = serde_json::from_str(&json)
                    .with_context(|| format!("failed to parse config: {}", path.display()))?;
                eprintln!("[config] loaded from {}", path.display());
                return Ok(config);
            }
        }
        eprintln!("[config] no config file found, using built-in defaults");
        Ok(Self::defaults())
    }

    /// Search order for config files.
    fn config_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        // .axonerai/axonerai.jsonc in the current directory
        paths.push(PathBuf::from(".axonerai/axonerai.jsonc"));

        // ~/.config/axonerai/axonerai.jsonc
        if let Some(home) = std::env::var_os("HOME") {
            let p = Path::new(&home).join(".config/axonerai/axonerai.jsonc");
            paths.push(p);
        }

        paths
    }

    /// Built-in default config with the three providers we support.
    pub fn defaults() -> Self {
        AppConfig {
            default_provider: "mistral".to_string(),
            default_model: "zai-glm-5-2".to_string(),
            providers: {
                let mut m = HashMap::new();

                m.insert(
                    "mistral".to_string(),
                    ProviderConfig {
                        name: "Mistral".to_string(),
                        endpoint: "https://api.mistral.ai/v1/chat/completions".to_string(),
                        env_key: "MISTRAL_API_KEY".to_string(),
                        api_key: None,
                        default_model: Some("zai-glm-5-2".to_string()),
                        models: vec![ModelConfig {
                            id: "zai-glm-5-2".to_string(),
                            name: Some("Z.ai GLM 5.2".to_string()),
                            thinking: false,
                            thinking_levels: vec![],
                        }],
                    },
                );

                m.insert(
                    "opencode-zen".to_string(),
                    ProviderConfig {
                        name: "OpenCode Zen".to_string(),
                        endpoint: "https://opencode.ai/zen/v1/chat/completions".to_string(),
                        env_key: "OPENCODE_API_KEY".to_string(),
                        api_key: None,
                        default_model: Some("glm-5.2".to_string()),
                        models: vec![
                            ModelConfig {
                                id: "glm-5.2".to_string(),
                                name: Some("GLM 5.2".to_string()),
                                thinking: false,
                                thinking_levels: vec![],
                            },
                            ModelConfig {
                                id: "glm-5.1".to_string(),
                                name: Some("GLM 5.1".to_string()),
                                thinking: false,
                                thinking_levels: vec![],
                            },
                        ],
                    },
                );

                m.insert(
                    "opencode-go".to_string(),
                    ProviderConfig {
                        name: "OpenCode Go".to_string(),
                        endpoint: "https://opencode.ai/zen/go/v1/chat/completions".to_string(),
                        env_key: "OPENCODE_API_KEY".to_string(),
                        api_key: None,
                        default_model: Some("glm-5.2".to_string()),
                        models: vec![
                            ModelConfig {
                                id: "glm-5.2".to_string(),
                                name: Some("GLM 5.2".to_string()),
                                thinking: false,
                                thinking_levels: vec![],
                            },
                            ModelConfig {
                                id: "glm-5.3".to_string(),
                                name: Some("GLM 5.3".to_string()),
                                thinking: false,
                                thinking_levels: vec![],
                            },
                        ],
                    },
                );

                m.insert(
                    "groq".to_string(),
                    ProviderConfig {
                        name: "Groq".to_string(),
                        endpoint: "https://api.groq.com/openai/v1/chat/completions".to_string(),
                        env_key: "GROQ_API_KEY".to_string(),
                        api_key: None,
                        default_model: Some("openai/gpt-oss-120b".to_string()),
                        models: vec![ModelConfig {
                            id: "openai/gpt-oss-120b".to_string(),
                            name: Some("GPT-OSS 120B".to_string()),
                            thinking: false,
                            thinking_levels: vec![],
                        }],
                    },
                );

                m
            },
        }
    }

    /// Resolve the API key for a provider: config api_key > env var.
    pub fn resolve_api_key(&self, provider_name: &str) -> Result<String> {
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow!("unknown provider: {}", provider_name))?;

        if let Some(key) = &provider.api_key {
            return Ok(key.clone());
        }

        std::env::var(&provider.env_key).map_err(|_| {
            anyhow!(
                "env var {} not set for provider '{}'",
                provider.env_key,
                provider_name
            )
        })
    }

    /// Get the endpoint URL for a provider.
    pub fn endpoint(&self, provider_name: &str) -> Result<&str> {
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow!("unknown provider: {}", provider_name))?;
        Ok(&provider.endpoint)
    }

    /// Get the default model ID for a provider.
    pub fn default_model_id(&self, provider_name: &str) -> Result<&str> {
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow!("unknown provider: {}", provider_name))?;

        if let Some(model) = &provider.default_model {
            return Ok(model);
        }
        if let Some(first) = provider.models.first() {
            return Ok(&first.id);
        }
        Err(anyhow!(
            "no models configured for provider '{}'",
            provider_name
        ))
    }

    /// Find a model config by ID within a provider.
    pub fn find_model(&self, provider_name: &str, model_id: &str) -> Result<&ModelConfig> {
        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow!("unknown provider: {}", provider_name))?;
        provider
            .models
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| {
                anyhow!(
                    "model '{}' not found in provider '{}'",
                    model_id,
                    provider_name
                )
            })
    }

    /// List all provider names.
    pub fn provider_names(&self) -> Vec<String> {
        self.providers.keys().cloned().collect()
    }
}

/// Strip `//` line comments and `/* */` block comments from JSONC text,
/// producing valid JSON. String contents are preserved.
fn strip_jsonc_comments(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;
    let mut in_string = false;
    let mut escape = false;

    while i < chars.len() {
        let c = chars[i];

        if in_string {
            result.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if c == '"' {
            in_string = true;
            result.push(c);
            i += 1;
            continue;
        }

        // Line comment
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            i += 2;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }

        // Block comment
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i += 2;
            continue;
        }

        result.push(c);
        i += 1;
    }

    result
}
