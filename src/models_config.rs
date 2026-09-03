//! item41 — per-provider model config: context windows, token costs and
//! offers live in per-provider JSONC files, NOT in the main app config or
//! the GUI.
//!
//! Locations (LOCAL MASKS USER):
//! - local:  `.axonerai/models/<provider>-models.jsonc` (repo, shareable)
//! - user:   `~/.axonerai/models/<provider>-models.jsonc` (user level)
//!
//! Missing dirs/files are missing-safe: `load*` returns `Ok(None)` — an
//! empty model list, never a crash. Every file is validated against the JTD
//! schema `schemas/provider_models.jdt.json` (`additionalProperties: false`)
//! before use, and every mutation goes serialize → validate → write: a
//! failed validation writes nothing.
//!
//! `updated` is an ISO 8601 UTC date string (`YYYY-MM-DD`).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::strip_jsonc_comments;

/// Strip trailing commas before `}` / `]` (outside strings) from JSONC text
/// after comment-stripping. The repo's shared JSONC parser handles comments
/// only; per-provider models files also allow trailing commas (item41).
pub(crate) fn strip_trailing_commas(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_string = false;
    let mut escape = false;
    for c in input.chars() {
        if in_string {
            result.push(c);
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        if c == '"' {
            in_string = true;
        } else if c == '}' || c == ']' {
            let trimmed_len = result.trim_end().len();
            if result[..trimmed_len].ends_with(',') {
                result.truncate(trimmed_len - 1);
            }
        }
        result.push(c);
    }
    result
}

/// The JSONC pipeline for models files: comments then trailing commas.
pub(crate) fn parse_jsonc(raw: &str) -> String {
    strip_trailing_commas(&strip_jsonc_comments(raw))
}

/// The JTD schema, embedded so every consumer (server, CLI bin, tests)
/// validates against the exact same contract.
pub const SCHEMA_JSON: &str = include_str!("../schemas/provider_models.jdt.json");

/// Top-level shape of one `<provider>-models.jsonc` file.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderModelsFile {
    /// Must match the file's provider short name (e.g. "mistral").
    pub provider: String,
    /// ISO 8601 UTC date the file was last written (`YYYY-MM-DD`).
    pub updated: String,
    pub models: Vec<ProviderModel>,
}

/// One model entry: the API id, a human name, its context window (tokens),
/// plus optional cost strings and a freeform offer string.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderModel {
    /// The model id used in API calls (e.g. "zai-glm-5-2").
    pub id: String,
    /// Human name (e.g. "GLM-5.2").
    pub display: String,
    /// Context window in tokens. REQUIRED.
    pub context_window: u32,
    /// Optional per-token cost strings (kept as strings to avoid float
    /// semantics), or null.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub costs: Option<ModelCosts>,
    /// Optional freeform offer (e.g. "2x usage").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offer: Option<String>,
}

/// Optional cost strings for one model. Absent fields are omitted on write.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelCosts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_per_mtok: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_per_mtok: Option<String>,
}

/// Which file won the local-masks-user resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelSource {
    Local,
    User,
}

impl ModelSource {
    pub fn as_str(self) -> &'static str {
        match self {
            ModelSource::Local => "local",
            ModelSource::User => "user",
        }
    }
}

/// A loaded, JTD-validated provider-models config plus where it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedModels {
    pub source: ModelSource,
    pub path: PathBuf,
    pub config: ProviderModelsFile,
}

impl LoadedModels {
    /// Look up one model by id.
    pub fn find(&self, model_id: &str) -> Option<&ProviderModel> {
        self.config.models.iter().find(|m| m.id == model_id)
    }

    /// Context window for one model id, if known.
    pub fn context_window(&self, model_id: &str) -> Option<u32> {
        self.find(model_id).map(|m| m.context_window)
    }
}

/// The local models dir: `.axonerai/models` under the current directory.
pub fn local_dir() -> PathBuf {
    PathBuf::from(".axonerai/models")
}

/// The user models dir: `~/.axonerai/models`.
pub fn user_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join(".axonerai/models"),
        None => PathBuf::from(""),
    }
}

/// The file name for one provider inside a models dir.
pub fn file_name(provider: &str) -> String {
    format!("{provider}-models.jsonc")
}

/// Validate one instance against the embedded JTD schema.
/// Errors name every JTD violation (instance + schema paths).
pub fn validate_instance(instance: &Value) -> Result<()> {
    let serde_schema: jtd::SerdeSchema = serde_json::from_str(SCHEMA_JSON)
        .context("provider_models.jdt.json is not a valid JTD schema")?;
    let schema = jtd::Schema::from_serde_schema(serde_schema)
        .context("provider_models.jdt.json failed JTD compilation")?;
    let errors = jtd::validate(&schema, instance, Default::default())
        .context("JTD validation call failed")?;
    if errors.is_empty() {
        return Ok(());
    }
    let details: Vec<String> = errors
        .iter()
        .map(|e| {
            format!(
                "{} (schema {})",
                e.instance_path.join("/"),
                e.schema_path.join("/")
            )
        })
        .collect();
    bail!("JTD validation failed: {}", details.join("; "))
}

/// Read, strip JSONC comments, JTD-validate and deserialize one file.
pub fn parse_file(path: &Path) -> Result<ProviderModelsFile> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let json = parse_jsonc(&raw);
    let value: Value = serde_json::from_str(&json)
        .with_context(|| format!("invalid JSON(C) in {}", path.display()))?;
    validate_instance(&value).with_context(|| format!("schema violation in {}", path.display()))?;
    let config: ProviderModelsFile = serde_json::from_value(value)
        .with_context(|| format!("failed to deserialize {}", path.display()))?;
    Ok(config)
}

/// Core loader: resolve `<provider>-models.jsonc` against two candidate
/// dirs, LOCAL MASKS USER. Missing dirs/files are missing-safe (`Ok(None)`).
pub fn load_from_dirs(local: &Path, user: &Path, provider: &str) -> Result<Option<LoadedModels>> {
    let name = file_name(provider);

    let local_path = local.join(&name);
    if local_path.exists() {
        let config = parse_file(&local_path).with_context(|| {
            format!(
                "local {} is invalid; fix or remove it",
                local_path.display()
            )
        })?;
        if config.provider != provider {
            bail!(
                "local {} declares provider '{}' but was loaded as '{}'",
                local_path.display(),
                config.provider,
                provider
            );
        }
        return Ok(Some(LoadedModels {
            source: ModelSource::Local,
            path: local_path,
            config,
        }));
    }

    let user_path = user.join(&name);
    if user_path.exists() {
        let config = parse_file(&user_path).with_context(|| {
            format!("user {} is invalid; fix or remove it", user_path.display())
        })?;
        if config.provider != provider {
            bail!(
                "user {} declares provider '{}' but was loaded as '{}'",
                user_path.display(),
                config.provider,
                provider
            );
        }
        return Ok(Some(LoadedModels {
            source: ModelSource::User,
            path: user_path,
            config,
        }));
    }

    Ok(None)
}

/// Load a provider's models config from the default dirs (local masks user).
/// Missing-safe: `Ok(None)` when no file exists anywhere.
pub fn load(provider: &str) -> Result<Option<LoadedModels>> {
    load_from_dirs(&local_dir(), &user_dir(), provider)
}

/// Copy one file to `<name>.<unix-epoch>` next to it, returning the backup
/// path. Used by `axonerai-models backup` and by every mutation.
pub fn backup_file(path: &Path, epoch_secs: u64) -> Result<PathBuf> {
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .with_context(|| format!("path has no file name: {}", path.display()))?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let backup = parent.join(format!("{file_name}.{epoch_secs}"));
    std::fs::copy(path, &backup).with_context(|| {
        format!(
            "failed to back up {} to {}",
            path.display(),
            backup.display()
        )
    })?;
    Ok(backup)
}

/// Back up every `*-models.jsonc` in both dirs (local + user) to
/// `<name>.<epoch_secs>`. No files → empty list, never a crash.
pub fn backup_all(local: &Path, user: &Path, epoch_secs: u64) -> Result<Vec<PathBuf>> {
    let mut backups = Vec::new();
    for dir in [local, user] {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue, // missing dir is missing-safe
        };
        let mut files: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.file_name()
                        .map(|n| {
                            let n = n.to_string_lossy();
                            n.ends_with("-models.jsonc")
                        })
                        .unwrap_or(false)
            })
            .collect();
        files.sort();
        for file in files {
            backups.push(backup_file(&file, epoch_secs)?);
        }
    }
    Ok(backups)
}

/// Unix seconds for the `updated` field and backup suffixes.
pub fn unix_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Today's ISO 8601 UTC date (`YYYY-MM-DD`) from unix seconds — the `updated`
/// value stamped on every mutation. Days-to-civil (Howard Hinnant's
/// algorithm), no chrono dependency outside the `web` feature.
pub fn iso_date_from_epoch_secs(epoch_secs: u64) -> String {
    let days = (epoch_secs / 86_400) as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Serialize → JTD-validate → write a mutated config to its file. A failed
/// validation writes nothing (the caller has already made its backup).
pub fn write_config(path: &Path, config: &ProviderModelsFile) -> Result<()> {
    let value = serde_json::to_value(config).context("failed to serialize models config")?;
    validate_instance(&value).context("refusing to write: serialized config fails JTD")?;
    let json = serde_json::to_string_pretty(&value).context("failed to render models config")?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, json + "\n")
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(())
}

/// Set the cost strings for one model in the winning config file: back up
/// ALL config files first, then load, mutate, validate and write. Unknown
/// provider/model → error, nothing written.
pub fn set_cost(
    local: &Path,
    user: &Path,
    provider: &str,
    model_id: &str,
    input: &str,
    output: &str,
    epoch_secs: u64,
) -> Result<(PathBuf, ProviderModelsFile)> {
    set_model(local, user, provider, model_id, epoch_secs, |model| {
        model.costs = Some(ModelCosts {
            input_per_mtok: Some(input.to_string()),
            output_per_mtok: Some(output.to_string()),
        });
    })
}

/// Set the context window for one model in the winning config file: back up
/// ALL config files first, then load, mutate, validate and write.
pub fn set_context(
    local: &Path,
    user: &Path,
    provider: &str,
    model_id: &str,
    tokens: u32,
    epoch_secs: u64,
) -> Result<(PathBuf, ProviderModelsFile)> {
    set_model(local, user, provider, model_id, epoch_secs, |model| {
        model.context_window = tokens;
    })
}

fn set_model(
    local: &Path,
    user: &Path,
    provider: &str,
    model_id: &str,
    epoch_secs: u64,
    mutate: impl FnOnce(&mut ProviderModel),
) -> Result<(PathBuf, ProviderModelsFile)> {
    backup_all(local, user, epoch_secs)?;
    let loaded = load_from_dirs(local, user, provider)?
        .with_context(|| format!("no models config file for provider '{provider}'"))?;
    let mut config = loaded.config;
    let model = config
        .models
        .iter_mut()
        .find(|m| m.id == model_id)
        .with_context(|| {
            format!(
                "model '{model_id}' not found in provider '{provider}' ({})",
                loaded.path.display()
            )
        })?;
    mutate(model);
    config.updated = iso_date_from_epoch_secs(epoch_secs);
    write_config(&loaded.path, &config)?;
    Ok((loaded.path, config))
}

/// One model reported by a provider's models-listing endpoint.
#[derive(Debug, Clone, PartialEq)]
pub struct FetchedModel {
    pub id: String,
    pub max_context_length: Option<u64>,
}

/// Known models-listing endpoints. Only DOCUMENTED endpoints live here —
/// `fetch` never guesses URLs.
pub fn fetch_endpoint(provider: &str) -> Option<(&'static str, &'static str)> {
    match provider {
        // Mistral's documented metadata endpoint.
        "mistral" => Some(("https://api.mistral.ai/v1/models", "MISTRAL_API_KEY")),
        _ => None,
    }
}

/// GET a provider's models-listing endpoint (OpenAI-compatible
/// `{"data": [{"id": ..., "max_context_length": ...}]}`) with a Bearer key.
/// `url` is a parameter so tests can point at a stub server.
pub async fn fetch_models_from_url(
    client: &reqwest::Client,
    url: &str,
    api_key: &str,
) -> Result<Vec<FetchedModel>> {
    #[derive(Deserialize)]
    struct RawModel {
        id: String,
        #[serde(default)]
        max_context_length: Option<u64>,
    }
    #[derive(Deserialize)]
    struct RawList {
        data: Vec<RawModel>,
    }

    let response = client
        .get(url)
        .bearer_auth(api_key)
        .send()
        .await
        .with_context(|| format!("request to {url} failed"))?;
    let status = response.status();
    let body = response
        .text()
        .await
        .context("failed to read models response body")?;
    if !status.is_success() {
        bail!("models endpoint returned {status}: {}", body.trim());
    }
    let list: RawList = serde_json::from_str(&body)
        .with_context(|| format!("unexpected models response shape from {url}"))?;
    Ok(list
        .data
        .into_iter()
        .map(|m| FetchedModel {
            id: m.id,
            max_context_length: m.max_context_length,
        })
        .collect())
}

/// The diff one fetch would apply to a loaded config: (id, old, new) for
/// every KNOWN model whose context window the API reports differently.
/// Unknown API models are ignored — fetch never invents roster entries.
pub fn fetch_diff(
    config: &ProviderModelsFile,
    fetched: &[FetchedModel],
) -> Vec<(String, u32, u32)> {
    let mut diff = Vec::new();
    for model in &config.models {
        let Some(report) = fetched
            .iter()
            .find(|f| f.id == model.id)
            .and_then(|f| f.max_context_length)
        else {
            continue;
        };
        let report = u32::try_from(report).unwrap_or(u32::MAX);
        if report != model.context_window {
            diff.push((model.id.clone(), model.context_window, report));
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn valid_value() -> Value {
        json!({
            "provider": "mistral",
            "updated": "2026-09-03",
            "models": [
                {
                    "id": "zai-glm-5-2",
                    "display": "GLM-5.2",
                    "context_window": 131072,
                    "costs": {
                        "input_per_mtok": "$0.50",
                        "output_per_mtok": "$1.50"
                    }
                }
            ]
        })
    }

    #[test]
    fn schema_accepts_a_valid_instance() {
        assert!(validate_instance(&valid_value()).is_ok());
    }

    #[test]
    fn schema_accepts_minimal_instance_with_offer() {
        let value = json!({
            "provider": "opencode-zen",
            "updated": "2026-09-03",
            "models": [
                {"id": "glm-5.2", "display": "GLM 5.2", "context_window": 1,
                 "offer": "2x usage"}
            ]
        });
        assert!(validate_instance(&value).is_ok());
    }

    #[test]
    fn schema_rejects_price_as_number_where_string_expected() {
        let mut bad = valid_value();
        bad["models"][0]["costs"]["input_per_mtok"] = json!(0.5);
        assert!(validate_instance(&bad).is_err(), "numeric cost must fail");
    }

    #[test]
    fn schema_rejects_unknown_extra_field() {
        let mut bad = valid_value();
        bad["models"][0]["unexpected"] = json!("nope");
        assert!(validate_instance(&bad).is_err(), "extra field must fail");
    }

    #[test]
    fn schema_rejects_missing_context_window() {
        let mut bad = valid_value();
        bad["models"][0]
            .as_object_mut()
            .unwrap()
            .remove("context_window");
        assert!(validate_instance(&bad).is_err());
    }

    #[test]
    fn schema_rejects_context_window_overflow() {
        let mut bad = valid_value();
        bad["models"][0]["context_window"] = json!(4294967296u64);
        assert!(
            validate_instance(&bad).is_err(),
            "uint32 overflow must fail"
        );
    }

    #[test]
    fn iso_date_from_epoch_secs_formats_utc_dates() {
        assert_eq!(iso_date_from_epoch_secs(0), "1970-01-01");
        // 2026-09-03T00:00:00Z
        assert_eq!(iso_date_from_epoch_secs(1_788_393_600), "2026-09-03");
        // 2024-02-29 leap day: 1709164800
        assert_eq!(iso_date_from_epoch_secs(1_709_164_800), "2024-02-29");
    }

    #[test]
    fn fetch_diff_reports_only_known_models_with_changed_windows() {
        let config = ProviderModelsFile {
            provider: "mistral".into(),
            updated: "2026-09-03".into(),
            models: vec![
                ProviderModel {
                    id: "a".into(),
                    display: "A".into(),
                    context_window: 100,
                    costs: None,
                    offer: None,
                },
                ProviderModel {
                    id: "b".into(),
                    display: "B".into(),
                    context_window: 200,
                    costs: None,
                    offer: None,
                },
            ],
        };
        let fetched = vec![
            FetchedModel {
                id: "a".into(),
                max_context_length: Some(128),
            },
            FetchedModel {
                id: "unknown".into(),
                max_context_length: Some(999),
            },
            FetchedModel {
                id: "b".into(),
                max_context_length: Some(200),
            },
            FetchedModel {
                id: "c".into(),
                max_context_length: None,
            },
        ];
        assert_eq!(
            fetch_diff(&config, &fetched),
            vec![("a".to_string(), 100, 128)]
        );
    }

    #[test]
    fn fetch_endpoint_only_maps_documented_providers() {
        assert!(fetch_endpoint("mistral").is_some());
        assert!(fetch_endpoint("opencode-zen").is_none());
        assert!(fetch_endpoint("opencode-go").is_none());
        assert!(fetch_endpoint("groq").is_none());
    }
}
