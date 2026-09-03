//! item43 — read-only builtin `ModelsConfig` tool: the agent-facing window
//! onto the per-provider model config (item41, `crate::models_config`).
//!
//! Input `{}` lists every provider from the axonerai.jsonc roster (falling
//! back to the built-in known list); an optional `{"provider": "mistral"}`
//! filter loads exactly that provider. Output is pretty JSON: per provider
//! the JTD-validated config (`provider`, `updated`, `models[]` with
//! `id`/`display`/`context_window`/`costs`/`offer`) plus which source won
//! (`local` masks `user`). Missing config files are missing-safe (no entry,
//! never a crash); an invalid file surfaces as a per-provider `error` tool
//! result, never an agent-loop abort.
//!
//! READ-ONLY: `Tool::is_read_only()` stays defaulted `true` — the future
//! `--tools-readonly` flag (item45) indexes that marker. Updates belong in
//! the `.axonerai/models/<provider>-models.jsonc` files themselves
//! (JTD-validated), applied with the `axonerai-models` bin.

use crate::models_config as mcfg;
use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use std::path::PathBuf;

/// One provider's entry in the tool output. Providers with no config file
/// anywhere get no entry (missing-safe); a broken file gets `error`.
#[derive(Serialize)]
struct ProviderEntry {
    provider: String,
    /// Winning source: `"local"` (masks user) or `"user"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<&'static str>,
    /// The winning file path.
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// The JTD-validated config as stored on disk.
    #[serde(skip_serializing_if = "Option::is_none")]
    config: Option<mcfg::ProviderModelsFile>,
    /// Read/validation failure for this provider, surfaced as a tool result.
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Serialize)]
struct ModelsConfigOutput {
    providers: Vec<ProviderEntry>,
}

pub struct ModelsConfig {
    local: PathBuf,
    user: PathBuf,
    roster: PathBuf,
}

impl ModelsConfig {
    pub fn new() -> Self {
        Self {
            local: mcfg::local_dir(),
            user: mcfg::user_dir(),
            roster: PathBuf::from(".axonerai/axonerai.jsonc"),
        }
    }

    /// Explicit dirs for tests; the roster path still defaults to
    /// `.axonerai/axonerai.jsonc` (chain `with_roster` to isolate).
    pub fn with_dirs(local: PathBuf, user: PathBuf) -> Self {
        Self {
            local,
            user,
            roster: PathBuf::from(".axonerai/axonerai.jsonc"),
        }
    }

    pub fn with_roster(mut self, roster: PathBuf) -> Self {
        self.roster = roster;
        self
    }
}

impl Default for ModelsConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ModelsConfig {
    fn name(&self) -> String {
        "ModelsConfig".to_string()
    }

    fn description(&self) -> String {
        "Current model-cost and context-window config for every provider, \
         loaded from .axonerai/models/<provider>-models.jsonc (user fallback \
         ~/.axonerai/models/, local masks user). Input {} lists all providers; \
         optional {\"provider\": \"mistral\"} filters to one. Updates belong in \
         those files (JTD-validated schema provider_models.jdt.json) via the \
         axonerai-models bin — never guessed or hard-coded here."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "provider": {
                    "type": "string",
                    "description": "Optional provider short name (e.g. \"mistral\"); omit for every provider"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let filter = input
            .get("provider")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let providers: Vec<String> = match filter {
            // A filter loads exactly the named provider, roster or not.
            Some(provider) => vec![provider],
            None => mcfg::list_providers(&self.roster),
        };

        let mut entries = Vec::new();
        for provider in providers {
            match mcfg::load_from_dirs(&self.local, &self.user, &provider) {
                Ok(Some(loaded)) => entries.push(ProviderEntry {
                    source: Some(loaded.source.as_str()),
                    path: Some(loaded.path.display().to_string()),
                    config: Some(loaded.config),
                    provider,
                    error: None,
                }),
                Ok(None) => {} // no config file anywhere: missing-safe, no entry
                Err(err) => entries.push(ProviderEntry {
                    source: None,
                    path: None,
                    config: None,
                    provider,
                    error: Some(err.to_string()),
                }),
            }
        }

        let output = ModelsConfigOutput { providers: entries };
        Ok(serde_json::to_string_pretty(&output)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::fs;
    use std::path::Path;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Serialises the (process-global) temp dir naming; dirs themselves are
    /// injected, so no CWD locking is needed.
    static LOCK: Mutex<()> = Mutex::new(());

    fn temp_dir(tag: &str) -> PathBuf {
        let _lock = LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "axonerai-models-config-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// Hermetic tool: temp dirs, roster path that cannot exist so
    /// enumeration falls back to KNOWN_PROVIDERS.
    fn tool(local: &Path, user: &Path) -> ModelsConfig {
        ModelsConfig::with_dirs(local.to_path_buf(), user.to_path_buf())
            .with_roster(PathBuf::from("nonexistent-roster-for-tests.jsonc"))
    }

    fn write_config(dir: &Path, provider: &str, jsonc: &str) {
        fs::create_dir_all(dir).expect("create models dir");
        fs::write(dir.join(mcfg::file_name(provider)), jsonc).expect("write config");
    }

    fn mistral_jsonc(input: &str, output: &str) -> String {
        format!(
            r#"{{
  "provider": "mistral",
  "updated": "2026-09-03",
  "models": [
    {{
      "id": "mistral-large-latest",
      "display": "Mistral Large",
      "context_window": 131072,
      "costs": {{"input_per_mtok": "{input}", "output_per_mtok": "{output}"}}
    }}
  ]
}}"#
        )
    }

    fn groq_jsonc() -> String {
        r#"{
  "provider": "groq",
  "updated": "2026-09-01",
  "models": [
    {"id": "test-model", "display": "Test Model", "context_window": 8192}
  ]
}"#
        .to_string()
    }

    async fn run(tool: &ModelsConfig, input: Value) -> Value {
        let raw = tool.execute(input).await.expect("execute succeeds");
        serde_json::from_str(&raw).expect("output must be valid JSON")
    }

    #[tokio::test]
    async fn empty_config_yields_valid_empty_json() {
        let local = temp_dir("empty-local");
        let user = temp_dir("empty-user");
        let tool = tool(&local, &user);

        let out = run(&tool, json!({})).await;
        assert_eq!(
            out["providers"].as_array().map(Vec::len),
            Some(0),
            "no config files → empty providers array: {out}"
        );
    }

    #[tokio::test]
    async fn loaded_configs_include_contents_and_source_label() {
        let local = temp_dir("local");
        let user = temp_dir("user");
        write_config(&local, "mistral", &mistral_jsonc("$2.00", "$6.00"));
        write_config(&user, "groq", &groq_jsonc());
        let tool = tool(&local, &user);

        let out = run(&tool, json!({})).await;
        let by_provider = |name: &str| {
            out["providers"]
                .as_array()
                .expect("providers array")
                .iter()
                .find(|e| e["provider"] == name)
                .unwrap_or_else(|| panic!("no entry for {name}: {out}"))
        };

        let mistral = by_provider("mistral");
        assert_eq!(mistral["source"], "local", "local file wins: {mistral}");
        assert_eq!(
            mistral["config"]["models"][0]["id"], "mistral-large-latest",
            "config contents round-trip"
        );
        assert_eq!(
            mistral["config"]["models"][0]["costs"]["input_per_mtok"],
            "$2.00"
        );
        assert_eq!(mistral["config"]["models"][0]["context_window"], 131072);

        let groq = by_provider("groq");
        assert_eq!(
            groq["source"], "user",
            "user file used when no local: {groq}"
        );
        assert_eq!(groq["config"]["models"][0]["id"], "test-model");
        assert_eq!(groq["config"]["models"][0]["context_window"], 8192);
    }

    #[tokio::test]
    async fn local_masks_user_for_the_same_provider() {
        let local = temp_dir("mask-local");
        let user = temp_dir("mask-user");
        write_config(&local, "mistral", &mistral_jsonc("$2.00", "$6.00"));
        write_config(&user, "mistral", &mistral_jsonc("$9.99", "$9.99"));
        let tool = tool(&local, &user);

        let out = run(&tool, json!({"provider": "mistral"})).await;
        let entries = out["providers"].as_array().expect("providers array");
        assert_eq!(entries.len(), 1, "exactly one entry: {out}");
        assert_eq!(entries[0]["source"], "local", "local must mask user: {out}");
        assert_eq!(
            entries[0]["config"]["models"][0]["costs"]["input_per_mtok"], "$2.00",
            "the LOCAL costs, not the user ones"
        );
    }

    #[tokio::test]
    async fn provider_filter_returns_only_that_provider() {
        let local = temp_dir("filter-local");
        let user = temp_dir("filter-user");
        write_config(&local, "mistral", &mistral_jsonc("$2.00", "$6.00"));
        write_config(&local, "groq", &groq_jsonc());
        let tool = tool(&local, &user);

        let out = run(&tool, json!({"provider": "mistral"})).await;
        let entries = out["providers"].as_array().expect("providers array");
        assert_eq!(entries.len(), 1, "filter must return one entry: {out}");
        assert_eq!(entries[0]["provider"], "mistral");

        // Filtering for a provider with no config file is missing-safe.
        let out = run(&tool, json!({"provider": "nope"})).await;
        assert_eq!(
            out["providers"].as_array().map(Vec::len),
            Some(0),
            "unknown provider → empty, never a crash: {out}"
        );
    }

    #[tokio::test]
    async fn invalid_config_file_surfaces_an_error_entry_not_a_crash() {
        let local = temp_dir("invalid-local");
        let user = temp_dir("invalid-user");
        write_config(&local, "mistral", "{ \"provider\": \"mistral\", }");
        let tool = tool(&local, &user);

        let out = run(&tool, json!({"provider": "mistral"})).await;
        let entries = out["providers"].as_array().expect("providers array");
        assert_eq!(entries.len(), 1, "broken file → one error entry: {out}");
        assert!(
            entries[0]["error"].as_str().is_some_and(|e| !e.is_empty()),
            "error must be reported as a tool result: {out}"
        );
        assert!(entries[0]["config"].is_null(), "no config on failure");
    }

    #[test]
    fn models_config_is_read_only() {
        assert!(
            ModelsConfig::new().is_read_only(),
            "ModelsConfig only reads config files; the future --tools-readonly \
             index must keep it"
        );
    }

    #[test]
    fn description_points_updates_at_the_jsonc_files() {
        let description = ModelsConfig::new().description();
        assert!(
            description.contains(".axonerai/models/<provider>-models.jsonc")
                && description.contains("JTD-validated"),
            "description must tell the model where updates belong: {description}"
        );
    }
}
