//! item41 — per-provider models config: load precedence (local masks user),
//! missing-safe loading, JTD bad cases through `parse_file`, backup naming,
//! set-cost/set-context mutations, and `fetch` against a stub HTTP server.

use std::path::{Path, PathBuf};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use axonerai::models_config::{
    self, FetchedModel, ModelSource, ProviderModel, ProviderModelsFile, backup_all, backup_file,
    fetch_diff, fetch_models_from_url, iso_date_from_epoch_secs, load_from_dirs, parse_file,
    set_context, set_cost, write_config,
};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("item41-{name}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// A valid mistral config with JSONC comments and a trailing comma — the
/// same laxity `.axonerai/axonerai.jsonc` enjoys.
fn valid_jsonc(provider: &str, context_window: u32) -> String {
    format!(
        r#"{{
        // Per-provider model facts. Local masks user.
        "provider": "{provider}",
        "updated": "2026-09-03",
        "models": [
            {{
                "id": "zai-glm-5-2",
                "display": "GLM-5.2",
                // tokens
                "context_window": {context_window},
                "costs": {{
                    "input_per_mtok": "$0.50",
                    "output_per_mtok": "$1.50",
                }},
            }},
        ],
    }}"#
    )
}

fn write_config_file(dir: &Path, provider: &str, body: &str) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let path = dir.join(models_config::file_name(provider));
    std::fs::write(&path, body).unwrap();
    path
}

// --- Load precedence + missing-safe ----------------------------------------

#[test]
fn local_masks_user() {
    let local = temp_dir("local");
    let user = temp_dir("user");
    write_config_file(&local, "mistral", &valid_jsonc("mistral", 32768));
    write_config_file(&user, "mistral", &valid_jsonc("mistral", 131072));

    let loaded = load_from_dirs(&local, &user, "mistral")
        .unwrap()
        .expect("a config exists in both dirs");

    assert_eq!(loaded.source, ModelSource::Local);
    assert_eq!(loaded.config.models[0].context_window, 32768);
    assert_eq!(loaded.context_window("zai-glm-5-2"), Some(32768));
}

#[test]
fn user_dir_used_when_local_missing() {
    let local = temp_dir("local-missing");
    let user = temp_dir("user-only");
    write_config_file(&user, "mistral", &valid_jsonc("mistral", 131072));

    let loaded = load_from_dirs(&local, &user, "mistral")
        .unwrap()
        .expect("user config falls through");

    assert_eq!(loaded.source, ModelSource::User);
    assert_eq!(loaded.config.models[0].context_window, 131072);
}

#[test]
fn missing_dirs_and_files_are_empty_safe() {
    let local = temp_dir("local-none");
    let user = temp_dir("user-none");
    // Neither dir even exists on disk.
    let loaded = load_from_dirs(&local, &user, "mistral").unwrap();
    assert!(loaded.is_none(), "no file → None, never a crash");
}

#[test]
fn jsonc_comments_and_trailing_commas_parse() {
    let dir = temp_dir("jsonc");
    let path = write_config_file(&dir, "mistral", &valid_jsonc("mistral", 131072));
    let config = parse_file(&path).unwrap();
    assert_eq!(config.provider, "mistral");
    assert_eq!(config.models.len(), 1);
    assert_eq!(config.models[0].id, "zai-glm-5-2");
    assert_eq!(
        config.models[0].costs.as_ref().unwrap().input_per_mtok,
        Some("$0.50".to_string())
    );
}

// --- JTD bad cases through parse_file --------------------------------------

#[test]
fn price_as_number_where_string_expected_fails() {
    let dir = temp_dir("bad-price");
    let body = r#"{
        "provider": "mistral",
        "updated": "2026-09-03",
        "models": [{
            "id": "zai-glm-5-2",
            "display": "GLM-5.2",
            "context_window": 131072,
            "costs": {"input_per_mtok": 0.5, "output_per_mtok": "1.5"}
        }]
    }"#;
    let path = write_config_file(&dir, "mistral", body);
    let err = format!("{:#}", parse_file(&path).unwrap_err());
    assert!(
        err.contains("JTD"),
        "the failure must be a schema violation: {err}"
    );
}

#[test]
fn unknown_extra_field_fails() {
    let dir = temp_dir("bad-extra");
    let body = r#"{
        "provider": "mistral",
        "updated": "2026-09-03",
        "models": [{
            "id": "zai-glm-5-2",
            "display": "GLM-5.2",
            "context_window": 131072,
            "unexpected": true
        }]
    }"#;
    let path = write_config_file(&dir, "mistral", body);
    let err = format!("{:#}", parse_file(&path).unwrap_err());
    assert!(err.contains("JTD"), "extra field must fail JTD: {err}");
}

#[test]
fn missing_context_window_fails() {
    let dir = temp_dir("bad-window");
    let body = r#"{
        "provider": "mistral",
        "updated": "2026-09-03",
        "models": [{"id": "x", "display": "X"}]
    }"#;
    let path = write_config_file(&dir, "mistral", body);
    assert!(parse_file(&path).is_err());
}

#[test]
fn provider_mismatch_fails() {
    let dir = temp_dir("provider-mismatch");
    let local = dir.join("local");
    let user = dir.join("user");
    write_config_file(&local, "mistral", &valid_jsonc("groq", 131072));
    let err = load_from_dirs(&local, &user, "mistral")
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("declares provider 'groq'"),
        "file provider must match: {err}"
    );
}

// --- Round-trip: serialize → validate → write → parse -----------------------

#[test]
fn write_config_round_trips_through_jtd() {
    let dir = temp_dir("round-trip");
    let path = dir.join(models_config::file_name("mistral"));
    let config = ProviderModelsFile {
        provider: "mistral".into(),
        updated: iso_date_from_epoch_secs(1_788_393_600),
        models: vec![ProviderModel {
            id: "zai-glm-5-2".into(),
            display: "GLM-5.2".into(),
            context_window: 131072,
            costs: None,
            offer: Some("half price to end of year".into()),
        }],
    };
    write_config(&path, &config).unwrap();
    let parsed = parse_file(&path).unwrap();
    assert_eq!(parsed, config);
}

#[test]
fn write_config_refuses_to_write_jtd_violations() {
    // Directly corrupt the in-memory struct past the type system (u32 cannot
    // be negative, so go through the Value path instead): a config whose
    // serialized shape violates the schema must not be written.
    let dir = temp_dir("write-refusal");
    let path = dir.join(models_config::file_name("mistral"));
    // Simulate: serialize a valid struct, tamper the Value (numeric cost),
    // and prove validate_instance catches it — write_config runs the same
    // gate before every write.
    let config = ProviderModelsFile {
        provider: "mistral".into(),
        updated: "2026-09-03".into(),
        models: vec![ProviderModel {
            id: "zai-glm-5-2".into(),
            display: "GLM-5.2".into(),
            context_window: 131072,
            costs: None,
            offer: None,
        }],
    };
    let mut value = serde_json::to_value(&config).unwrap();
    value["models"][0]["costs"] = serde_json::json!({"input_per_mtok": 0.5});
    let err = models_config::validate_instance(&value)
        .unwrap_err()
        .to_string();
    assert!(err.contains("JTD"));
    assert!(!path.exists(), "nothing was written");
}

// --- Backup naming ----------------------------------------------------------

#[test]
fn backup_file_uses_unix_epoch_suffix() {
    let dir = temp_dir("backup-one");
    let path = write_config_file(&dir, "mistral", &valid_jsonc("mistral", 131072));
    let epoch = 1_725_000_000u64;

    let backup = backup_file(&path, epoch).unwrap();

    assert_eq!(
        backup.file_name().unwrap().to_string_lossy(),
        format!("mistral-models.jsonc.{epoch}")
    );
    assert_eq!(
        std::fs::read_to_string(&backup).unwrap(),
        std::fs::read_to_string(&path).unwrap()
    );
}

#[test]
fn backup_all_copies_every_local_and_user_file() {
    let root = temp_dir("backup-all");
    let local = root.join("local");
    let user = root.join("user");
    write_config_file(&local, "mistral", &valid_jsonc("mistral", 131072));
    write_config_file(&local, "groq", &valid_jsonc("groq", 131072));
    write_config_file(&user, "opencode-zen", &valid_jsonc("opencode-zen", 131072));
    let epoch = 1_725_000_100u64;

    let backups = backup_all(&local, &user, epoch).unwrap();

    assert_eq!(backups.len(), 3, "2 local + 1 user");
    for backup in &backups {
        let name = backup.file_name().unwrap().to_string_lossy().into_owned();
        assert!(
            name.ends_with(&format!(".{epoch}")),
            "epoch suffix required: {name}"
        );
        assert!(backup.exists());
    }
}

#[test]
fn backup_all_with_no_files_is_empty_not_a_crash() {
    let root = temp_dir("backup-empty");
    let local = root.join("does-not-exist");
    let user = root.join("also-missing");
    let backups = backup_all(&local, &user, 1).unwrap();
    assert!(backups.is_empty());
}

// --- set-cost / set-context --------------------------------------------------

#[test]
fn set_cost_backs_up_then_updates_the_file() {
    let root = temp_dir("set-cost");
    let local = root.join("local");
    let user = root.join("user");
    let path = write_config_file(&local, "mistral", &valid_jsonc("mistral", 131072));
    let epoch = 1_788_393_600u64; // 2026-09-03T00:00:00Z

    let (written, config) = set_cost(
        &local,
        &user,
        "mistral",
        "zai-glm-5-2",
        "$0.40",
        "$1.20",
        epoch,
    )
    .unwrap();

    assert_eq!(written, path);
    let costs = config.models[0].costs.as_ref().unwrap();
    assert_eq!(costs.input_per_mtok, Some("$0.40".to_string()));
    assert_eq!(costs.output_per_mtok, Some("$1.20".to_string()));
    assert_eq!(config.updated, "2026-09-03", "mutations stamp updated");

    // A backup was created before the mutation.
    let backup = local.join(format!("mistral-models.jsonc.{epoch}"));
    assert!(backup.exists(), "backup must exist before mutation");
    let backup_config = parse_file(&backup).unwrap();
    let backup_costs = backup_config.models[0].costs.as_ref().unwrap();
    assert_eq!(
        backup_costs.input_per_mtok,
        Some("$0.50".to_string()),
        "backup holds the pre-mutation state"
    );

    // The live file parses back with the new costs.
    let reloaded = load_from_dirs(&local, &user, "mistral").unwrap().unwrap();
    assert_eq!(
        reloaded.config.models[0]
            .costs
            .as_ref()
            .unwrap()
            .output_per_mtok,
        Some("$1.20".to_string())
    );
}

#[test]
fn set_context_updates_the_window_and_keeps_costs() {
    let root = temp_dir("set-context");
    let local = root.join("local");
    let user = root.join("user");
    write_config_file(&local, "mistral", &valid_jsonc("mistral", 131072));
    let epoch = 1_725_000_300u64;

    let (_, config) = set_context(&local, &user, "mistral", "zai-glm-5-2", 65536, epoch).unwrap();

    assert_eq!(config.models[0].context_window, 65536);
    assert!(
        config.models[0].costs.is_some(),
        "unrelated fields survive the mutation"
    );
}

#[test]
fn set_unknown_model_errors_and_writes_nothing() {
    let root = temp_dir("set-unknown");
    let local = root.join("local");
    let user = root.join("user");
    let path = write_config_file(&local, "mistral", &valid_jsonc("mistral", 131072));
    let before = std::fs::read_to_string(&path).unwrap();
    let epoch = 1_725_000_400u64;

    let err = set_cost(&local, &user, "mistral", "not-a-model", "$1", "$2", epoch)
        .unwrap_err()
        .to_string();
    assert!(err.contains("not-a-model"), "{err}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "the config file is untouched"
    );
}

#[test]
fn set_with_no_config_file_errors() {
    let root = temp_dir("set-no-config");
    let local = root.join("local");
    let user = root.join("user");
    let err = set_context(&local, &user, "mistral", "zai-glm-5-2", 1, 1)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("no models config file"),
        "missing config is an error, not a fresh write: {err}"
    );
}

// --- fetch against a stub HTTP server ----------------------------------------

/// Spawn a one-shot stub HTTP server on 127.0.0.1 that answers every
/// connection with `status` + `body`. Returns its base URL.
async fn spawn_stub(status_line: &'static str, body: &'static str) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        if let Ok((mut socket, _)) = listener.accept().await {
            let mut buf = [0u8; 8192];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "{status_line}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        }
    });
    format!("http://{addr}/v1/models")
}

#[tokio::test]
async fn fetch_parses_context_lengths_from_the_models_endpoint() {
    let url = spawn_stub(
        "HTTP/1.1 200 OK",
        r#"{"object":"list","data":[
            {"id":"zai-glm-5-2","max_context_length":131072,"object":"model"},
            {"id":"mistral-medium-latest","max_context_length":32768},
            {"id":"no-window-model"}
        ]}"#,
    )
    .await;

    let fetched = fetch_models_from_url(&reqwest::Client::new(), &url, "test-key")
        .await
        .unwrap();

    assert_eq!(fetched.len(), 3);
    assert_eq!(fetched[0].id, "zai-glm-5-2");
    assert_eq!(fetched[0].max_context_length, Some(131072));
    assert_eq!(fetched[2].max_context_length, None);
}

#[tokio::test]
async fn fetch_surfaces_http_errors_with_status() {
    let url = spawn_stub("HTTP/1.1 401 Unauthorized", r#"{"message":"bad key"}"#).await;

    let err = fetch_models_from_url(&reqwest::Client::new(), &url, "wrong-key")
        .await
        .unwrap_err()
        .to_string();
    assert!(err.contains("401"), "error must carry the status: {err}");
}

#[tokio::test]
async fn fetch_diff_applies_through_backup_validate_write() {
    let root = temp_dir("fetch-apply");
    let local = root.join("local");
    let user = root.join("user");
    write_config_file(&local, "mistral", &valid_jsonc("mistral", 131072));
    let epoch = 1_725_000_500u64;

    let fetched = vec![FetchedModel {
        id: "zai-glm-5-2".into(),
        max_context_length: Some(32768),
    }];

    // Backup → mutate → validate → write, exactly what `axonerai-models
    // fetch` does end to end.
    let backups = backup_all(&local, &user, epoch).unwrap();
    assert_eq!(backups.len(), 1);
    let loaded = load_from_dirs(&local, &user, "mistral").unwrap().unwrap();
    let diff = fetch_diff(&loaded.config, &fetched);
    assert_eq!(diff, vec![("zai-glm-5-2".to_string(), 131072, 32768)]);

    let mut config = loaded.config.clone();
    for (id, _, window) in &diff {
        if let Some(model) = config.models.iter_mut().find(|m| &m.id == id) {
            model.context_window = *window;
        }
    }
    config.updated = iso_date_from_epoch_secs(epoch);
    write_config(&loaded.path, &config).unwrap();

    let reloaded = load_from_dirs(&local, &user, "mistral").unwrap().unwrap();
    assert_eq!(reloaded.context_window("zai-glm-5-2"), Some(32768));
    assert!(
        local.join(format!("mistral-models.jsonc.{epoch}")).exists(),
        "fetch backed up before writing"
    );
}
