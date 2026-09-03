//! `axonerai-models` — manage the per-provider model config files
//! (`.axonerai/models/<provider>-models.jsonc`, user fallback
//! `~/.axonerai/models/<provider>-models.jsonc`; LOCAL MASKS USER).
//!
//! Subcommands:
//! - `backup`                          — copy every `*-models.jsonc` to
//!   `<name>.<unix-epoch>` before any mutation
//! - `dump [provider]`                 — pretty-print loaded config(s) with
//!   the winning source noted
//! - `set-cost <provider> <id> <in> <out>` — set cost strings (backup first)
//! - `set-context <provider> <id> <tokens>` — set the context window
//! - `fetch <provider>`                — pull the provider's models endpoint
//!   and fill in `context_window` where reported (backup first, diff printed)
//!
//! Every mutation goes serialize → JTD-validate → write; a failed
//! validation writes nothing. Only DOCUMENTED models endpoints are called —
//! unknown endpoints print a clear note and the config's models, never a
//! guessed URL.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use axonerai::models_config::{
    self, LoadedModels, ModelSource, fetch_diff, fetch_endpoint, fetch_models_from_url,
    iso_date_from_epoch_secs, load, local_dir, user_dir,
};

#[derive(Parser, Debug)]
#[command(
    name = "axonerai-models",
    version,
    about = "Manage per-provider model config (context windows, costs, offers)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Back up every `*-models.jsonc` (local + user) to `<name>.<unix-epoch>`
    Backup,
    /// Pretty-print the loaded config(s), noting which source won
    Dump {
        /// Provider short name (e.g. mistral). Omit for every config on disk.
        provider: Option<String>,
    },
    /// Set the input/output per-MTok cost strings for one model
    SetCost {
        provider: String,
        model_id: String,
        input: String,
        output: String,
    },
    /// Set the context window (tokens) for one model
    SetContext {
        provider: String,
        model_id: String,
        tokens: u32,
    },
    /// Fill context_window from the provider's models-listing endpoint
    Fetch { provider: String },
}

fn main() -> Result<()> {
    load_env_file();
    let cli = Cli::parse();

    match cli.command {
        Commands::Backup => cmd_backup(),
        Commands::Dump { provider } => cmd_dump(provider.as_deref()),
        Commands::SetCost {
            provider,
            model_id,
            input,
            output,
        } => {
            let epoch = models_config::unix_epoch_secs();
            let (path, config) = models_config::set_cost(
                &local_dir(),
                &user_dir(),
                &provider,
                &model_id,
                &input,
                &output,
                epoch,
            )?;
            println!(
                "set-cost: {} on {} — input {} / output {}",
                model_id,
                path.display(),
                config
                    .models
                    .iter()
                    .find(|m| m.id == model_id)
                    .and_then(|m| m.costs.as_ref())
                    .and_then(|c| c.input_per_mtok.clone())
                    .unwrap_or_default(),
                config
                    .models
                    .iter()
                    .find(|m| m.id == model_id)
                    .and_then(|m| m.costs.as_ref())
                    .and_then(|c| c.output_per_mtok.clone())
                    .unwrap_or_default(),
            );
            Ok(())
        }
        Commands::SetContext {
            provider,
            model_id,
            tokens,
        } => {
            let epoch = models_config::unix_epoch_secs();
            let (path, _) = models_config::set_context(
                &local_dir(),
                &user_dir(),
                &provider,
                &model_id,
                tokens,
                epoch,
            )?;
            println!(
                "set-context: {} on {} — context_window {}",
                model_id,
                path.display(),
                tokens
            );
            Ok(())
        }
        Commands::Fetch { provider } => cmd_fetch(&provider),
    }
}

/// Reuse the repo's .env loading convention (same as axoner-web/axoner:
/// keys already set in the environment win).
fn load_env_file() {
    use std::io::BufRead;

    let Ok(file) = std::fs::File::open(".env") else {
        return;
    };
    for line in std::io::BufReader::new(file).lines().map_while(Result::ok) {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let (key, value) = (key.trim(), value.trim());
            if std::env::var_os(key).is_none() {
                // SAFETY: single-threaded startup, before any thread spawn.
                unsafe {
                    std::env::set_var(key, value);
                }
            }
        }
    }
}

fn cmd_backup() -> Result<()> {
    let epoch = models_config::unix_epoch_secs();
    let backups = models_config::backup_all(&local_dir(), &user_dir(), epoch)?;
    if backups.is_empty() {
        println!(
            "No models config files found in {} or {} — nothing to back up.",
            local_dir().display(),
            user_dir().display()
        );
        return Ok(());
    }
    for backup in &backups {
        println!("backed up: {}", backup.display());
    }
    Ok(())
}

fn cmd_dump(provider: Option<&str>) -> Result<()> {
    let providers: Vec<String> = match provider {
        Some(p) => vec![p.to_string()],
        None => providers_on_disk(),
    };
    if providers.is_empty() {
        println!(
            "No models config files found in {} or {}.",
            local_dir().display(),
            user_dir().display()
        );
        return Ok(());
    }
    for provider in &providers {
        match load(provider)? {
            Some(loaded) => {
                print_loaded(&loaded);
            }
            None => {
                println!(
                    "{}: no config file (checked {} then {})",
                    provider,
                    local_dir()
                        .join(models_config::file_name(provider))
                        .display(),
                    user_dir()
                        .join(models_config::file_name(provider))
                        .display(),
                );
            }
        }
    }
    Ok(())
}

/// Every provider with a config file on disk, local first then user, sorted.
fn providers_on_disk() -> Vec<String> {
    let mut names = std::collections::BTreeSet::new();
    for dir in [local_dir(), user_dir()] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if let Some(stem) = name.strip_suffix("-models.jsonc") {
                names.insert(stem.to_string());
            }
        }
    }
    names.into_iter().collect()
}

fn print_loaded(loaded: &LoadedModels) {
    let source = match loaded.source {
        ModelSource::Local => "local (masks user)",
        ModelSource::User => "user (no local file)",
    };
    println!(
        "{}: source {}, updated {}",
        loaded.config.provider, source, loaded.config.updated
    );
    for model in &loaded.config.models {
        let costs = match &model.costs {
            Some(costs) => format!(
                "  in {} out {}",
                costs.input_per_mtok.as_deref().unwrap_or("?"),
                costs.output_per_mtok.as_deref().unwrap_or("?")
            ),
            None => String::new(),
        };
        let offer = model
            .offer
            .as_deref()
            .map(|o| format!("  [offer: {o}]"))
            .unwrap_or_default();
        println!(
            "  {} ({}) — context_window {}{}{}",
            model.id, model.display, model.context_window, costs, offer
        );
    }
}

fn cmd_fetch(provider: &str) -> Result<()> {
    let Some((url, env_key)) = fetch_endpoint(provider) else {
        println!(
            "endpoint unknown for provider '{provider}': no documented models-listing endpoint is wired up."
        );
        println!("Known models (from config, unchanged):");
        match load(provider)? {
            Some(loaded) => print_loaded(&loaded),
            None => println!("  (no config file for '{provider}')"),
        }
        println!(
            "To wire one up, add it to fetch_endpoint() in src/models_config.rs — never guess a URL."
        );
        return Ok(());
    };

    let api_key = std::env::var(env_key)
        .with_context(|| format!("{env_key} not set (export it or put it in .env)"))?;

    let fetched = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(async { fetch_models_from_url(&reqwest::Client::new(), url, &api_key).await })?;

    let loaded = load(provider)?.with_context(|| {
        format!("no models config file for provider '{provider}' — create one before fetching")
    })?;
    let diff = fetch_diff(&loaded.config, &fetched);

    if diff.is_empty() {
        println!(
            "fetch: no context_window changes for '{provider}' ({} API models scanned, {} config models known)",
            fetched.len(),
            loaded.config.models.len()
        );
        return Ok(());
    }

    let epoch = models_config::unix_epoch_secs();
    let backups = models_config::backup_all(&local_dir(), &user_dir(), epoch)?;
    for backup in &backups {
        println!("backed up: {}", backup.display());
    }

    let mut config = loaded.config.clone();
    for (id, old, new) in &diff {
        if let Some(model) = config.models.iter_mut().find(|m| &m.id == id) {
            model.context_window = *new;
        }
        println!("changed: {id} context_window {old} → {new}");
    }
    config.updated = iso_date_from_epoch_secs(epoch);
    models_config::write_config(&loaded.path, &config)?;
    println!("fetch: wrote {}", loaded.path.display());
    Ok(())
}
