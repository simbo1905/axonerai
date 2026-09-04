use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Context;
use axum::Json;
use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::http::{HeaderName, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use axonerai::agent::ToolTrace;
use axonerai::models_config::{self, LoadedModels};
use axonerai::rollout::{self, Rollout, SessionInfo};
use axonerai::session::context_tokens_on_disk;
use axonerai::settings::Settings;
use axonerai::tool::{ToolInfo, ToolRegistry};
use axonerai::tools::{
    Calculator, Context7McpGetLibraryDocs, Context7McpResolveLibraryId, ListDir, ModelsConfig,
    ReadFile, ReadSkill, TavilyMcpExtract, TavilyMcpSearch, WebFetch, WebSearch, WriteFile,
};
use axonerai::wire::{ClientMsg, RolloutRecord, ServerMsg};
use axonerai::{Agent, AppConfig, FileSessionManager};

/// Max characters for large string fields on WS egress to the browser. The
/// rollout always stores FULL text; abridge is applied only when serving.
const EGRESS_ABRIDGE_CHARS: usize = 1024;

/// Max bytes for one streamed catch-up payload (`<ts>\0<type>\0<text>\n`).
/// Oversized payloads are cut char-boundary-safe by `lineformat::truncate_payload`;
/// no `…` marker is appended — the browser detects truncation by a failed
/// strict JSON parse.
const EGRESS_LINE_BYTES: usize = 1024;

#[derive(Parser, Debug)]
#[command(name = "agt", version, about = "AxonerAI tooling")]
struct Cli {
    /// Enable verbose logging (-v for debug, -vv for trace)
    #[arg(short, action = clap::ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Serve {
        /// Hostname to bind to (default: 127.0.0.1)
        #[arg(long)]
        host: Option<String>,

        /// Port to bind to (default: 0, auto-select a free high port)
        #[arg(long)]
        port: Option<u16>,

        /// Directory to serve `index.html` + `assets/` from (default: ./web)
        #[arg(long)]
        web_root: Option<PathBuf>,

        /// Provider to use (overrides config default: mistral, opencode-zen, opencode-go, groq)
        #[arg(long)]
        provider: Option<String>,

        /// Model ID to use (overrides provider default)
        #[arg(long)]
        model: Option<String>,

        /// Continue the most recent session
        #[arg(short = 'c', long = "continue")]
        continue_: bool,

        /// Reopen a specific session by uuid
        #[arg(short = 's', long = "session")]
        session: Option<String>,
    },
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
}

#[derive(Subcommand, Debug)]
enum SessionAction {
    /// List sessions, newest first
    List,
}

/// Swappable session (item54): the rollout and its id behind a lock so
/// POST /api/session/reset can start a fresh session in place (AppState is
/// cloned per request; the swap must be visible to every later reader).
struct SessionHandle {
    rollout: Arc<Rollout>,
    session_id: String,
}

/// Swappable agent runtime shared behind a lock so a /api/model swap is
/// visible to every handler clone and to every subsequent WS prompt run
/// (AppState is cloned per request, so the swappable state must live behind
/// interior mutability). item57: the SERVICE is swapped alongside the model
/// (the provider is rebuilt from the service on every swap).
struct Runtime {
    service: String,
    model: String,
    agent: Option<Arc<Agent>>,
    system_prompt: Option<String>,
}

/// Shared, swappable provider-models config (item41). Loaded at startup and
/// reloadable on demand; `None` = no config file for the provider (missing-safe).
struct ModelsConfigState {
    provider: String,
    /// Candidate dirs: local masks user.
    local: PathBuf,
    user: PathBuf,
    inner: RwLock<Option<LoadedModels>>,
}

impl ModelsConfigState {
    fn load(provider: &str) -> Self {
        Self::load_from(
            &models_config::local_dir(),
            &models_config::user_dir(),
            provider,
        )
    }

    /// Test seam: build from explicit candidate dirs (local masks user).
    fn load_from(local: &std::path::Path, user: &std::path::Path, provider: &str) -> Self {
        let loaded = models_config::load_from_dirs(local, user, provider)
            .map_err(|e| warn!("provider-models config ignored: {e:#}"))
            .unwrap_or(None);
        Self {
            provider: provider.to_string(),
            local: local.to_path_buf(),
            user: user.to_path_buf(),
            inner: RwLock::new(loaded),
        }
    }

    fn get(&self) -> Option<LoadedModels> {
        self.inner.read().ok().and_then(|guard| guard.clone())
    }

    /// The reload path: re-read the provider's models config from disk.
    /// A corrupt file keeps the previous load (warn, never crash).
    fn reload(&self) {
        match models_config::load_from_dirs(&self.local, &self.user, &self.provider) {
            Ok(loaded) => {
                if let Ok(mut guard) = self.inner.write() {
                    *guard = loaded;
                }
            }
            Err(e) => warn!("provider-models reload ignored: {e:#}"),
        }
    }

    /// Context window for one model id, if the config knows it.
    fn context_window(&self, model_id: &str) -> Option<u32> {
        self.get().and_then(|l| l.context_window(model_id))
    }
}

#[derive(Clone)]
struct AppState {
    web_root: PathBuf,
    /// jsonc config (roster validation for /api/model and agent rebuilds).
    config: Arc<AppConfig>,
    /// Per-provider models config (item41): context windows + costs + offers,
    /// `.axonerai/models/<provider>-models.jsonc` (local masks user). Reloaded
    /// on demand by `GET /api/models?reload=1`.
    models_cfg: Arc<ModelsConfigState>,
    /// Sessions dir: rollout + per-session agent-state home.
    sessions_dir: PathBuf,
    /// Swappable model/agent/system-prompt state (see [`Runtime`]).
    runtime: Arc<RwLock<Runtime>>,
    verbose: u8,
    /// Swappable session: rollout + id behind a lock (see [`SessionHandle`]).
    session: Arc<RwLock<SessionHandle>>,
    /// Registry clone sharing tool instances and suppression state with the
    /// agent's registry (both fields are Arc-backed in `ToolRegistry`).
    registry: ToolRegistry,
    /// Directory holding the per-session agent-state files
    /// (`<sessions_dir>/agent-state/<uuid>/messages.json`), the same files
    /// the agent persists via `FileSessionManager`.
    agent_state_dir: PathBuf,
    /// Settings file this server reads/writes (item54 test seam: tests point
    /// it at a temp file; production uses `.axonerai/settings.jsonc`).
    settings_path: PathBuf,
    /// item57 test seam: the API-key lookup used for service resolution
    /// (production reads the process env, seeded from .env; tests inject a
    /// map so resolution is deterministic without env mutation).
    key_lookup: Arc<dyn Fn(&str) -> Option<String> + Send + Sync>,
}

impl AppState {
    /// The current session rollout (clone of the Arc behind the lock).
    fn rollout(&self) -> Arc<Rollout> {
        self.session
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .rollout
            .clone()
    }

    /// The current session id.
    fn session_id(&self) -> String {
        self.session
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .session_id
            .clone()
    }

    /// Load the settings from this server's settings path.
    fn load_settings(&self) -> Settings {
        Settings::load_from(&self.settings_path)
    }

    /// Durable-first append to the rollout.
    fn append_event(&self, value: &serde_json::Value) {
        let _ = self.rollout().append_event(value);
    }

    /// Raw-JSON variant of [`AppState::append_event`] (payload-last field
    /// order preserved on disk).
    fn append_json(&self, json: &str) {
        let _ = self.rollout().append_json(json);
    }

    /// item57: resolve the API key for a service — config `api_key`
    /// override first, then the per-service env chain (zen/go fall back to
    /// the shared OPENCODE_API_KEY) through the injectable key lookup.
    fn resolve_service_key(&self, service: &str) -> Option<String> {
        let config_override = self
            .config
            .providers
            .get(service)
            .and_then(|p| p.api_key.clone());
        axonerai::services::resolve_key(service, config_override.as_deref(), |key| {
            (self.key_lookup)(key)
        })
    }
}

/// Load environment variables from a .env file if it exists
fn load_env_file() {
    use std::fs;
    use std::io::{BufRead, BufReader};

    let env_file = ".env";
    if let Ok(file) = fs::File::open(env_file) {
        let reader = BufReader::new(file);
        for line in reader.lines() {
            if let Ok(line) = line {
                // Skip empty lines and comments
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }

                // Parse key=value pairs
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();

                    // Only set if not already set in environment
                    if std::env::var(key).is_err() {
                        // std::env::set_var is unsafe but we're using it safely here
                        unsafe {
                            std::env::set_var(key, value);
                        }
                    }
                }
            }
        }
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Slash-leading prompts are control plane (the composer contract: "commands
/// NEVER go to the model"). The data plane enforces the same rule so a raw
/// WS client cannot burn a model call on a control-plane op: the prompt is
/// refused with an error frame instead of being forwarded to the agent.
fn is_slash_command(text: &str) -> bool {
    text.trim_start().starts_with('/')
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env_file();

    let cli = Cli::parse();

    let log_level = match cli.verbose {
        0 => "warn",
        1 => "axonerai=debug,axoner_web=debug,warn",
        _ => "axonerai=trace,axoner_web=trace,debug",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::fmt().with_env_filter(filter).init();

    match cli.command {
        Commands::Serve {
            host,
            port,
            web_root,
            provider,
            model,
            continue_,
            session,
        } => {
            serve(
                host,
                port,
                web_root,
                provider,
                model,
                continue_,
                session,
                cli.verbose,
            )
            .await
        }
        Commands::Session { action } => match action {
            SessionAction::List => session_list(),
        },
    }
}

/// Resolve (or create) the session rollout for this server run.
///
/// `--session <uuid>` opens that rollout (error if missing); `--continue`
/// opens the newest rollout by index (error if none); otherwise a fresh
/// session (uuid v7, default title = rightmost cwd component) is created and
/// a typed `ServerMsg::SessionMeta` event is written as its first line (the
/// on-disk shape is identical to item25's provisional JSON line so existing
/// rollouts stay readable).
fn resolve_session(
    sessions_dir: &std::path::Path,
    continue_: bool,
    session: Option<String>,
) -> anyhow::Result<(Arc<Rollout>, String)> {
    match session {
        Some(id) => {
            if !rollout::is_session_id(&id) {
                anyhow::bail!("not a valid session id (must be a lowercase UUID v7): {id}");
            }
            let rollout = Rollout::open(sessions_dir, &id)
                .with_context(|| format!("session rollout not found: {id}"))?;
            Ok((Arc::new(rollout), id))
        }
        None if continue_ => {
            let index = rollout::sessions_index(sessions_dir).with_context(|| {
                format!("failed to index sessions dir {}", sessions_dir.display())
            })?;
            let newest = index
                .first()
                .with_context(|| format!("no sessions found in {}", sessions_dir.display()))?;
            let id = newest.uuid.clone();
            let rollout = Rollout::open(sessions_dir, &id)
                .with_context(|| format!("session rollout not found: {id}"))?;
            Ok((Arc::new(rollout), id))
        }
        None => {
            let (rollout, id) = fresh_session(sessions_dir)?;
            Ok((Arc::new(rollout), id))
        }
    }
}

/// Create a brand-new session: uuid v7, default title = rightmost cwd
/// component, and a typed `ServerMsg::SessionMeta` event as the rollout's
/// first line (the on-disk shape is identical to item25's provisional JSON
/// line so existing rollouts stay readable). Shared by boot and by
/// POST /api/session/reset (item54).
fn fresh_session(sessions_dir: &std::path::Path) -> anyhow::Result<(Rollout, String)> {
    let id = uuid::Uuid::now_v7().to_string();
    let title = std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
        .unwrap_or_else(|| "session".to_string());
    let rollout = Rollout::create(sessions_dir, &id)?;
    let meta_value = serde_json::to_value(&ServerMsg::SessionMeta {
        session_id: id.as_str(),
        title: title.as_str(),
        created_at: now_ms(),
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "_type": "session_meta",
            "session_id": id.clone(),
            "title": title.clone(),
            "created_at": now_ms(),
        })
    });
    rollout.append_event(&meta_value)?;
    Ok((rollout, id))
}

async fn serve(
    host: Option<String>,
    port: Option<u16>,
    web_root: Option<PathBuf>,
    provider_override: Option<String>,
    model_override: Option<String>,
    continue_: bool,
    session: Option<String>,
    verbose: u8,
) -> anyhow::Result<()> {
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.unwrap_or(0);
    let web_root = web_root.unwrap_or_else(|| PathBuf::from("./web"));

    let config = AppConfig::load()?;

    let provider_name = provider_override.unwrap_or_else(|| {
        std::env::var("AXONERAI_PROVIDER").unwrap_or_else(|_| config.default_provider.clone())
    });

    let model_id = model_override
        .or_else(|| std::env::var("AXONERAI_MODEL").ok())
        .unwrap_or_else(|| {
            config
                .default_model_id(&provider_name)
                .unwrap_or_default()
                .to_string()
        });
    let registry = build_registry();

    // The system prompt is fixed for the run (no API key needed to load it);
    // it is sent with every completion and included in the context estimate.
    let system_prompt = Some(axonerai::prompt::load_system_prompt(
        &provider_name,
        &model_id,
    ));

    let sessions_dir = rollout::default_dir();
    let (session_rollout, session_id) = resolve_session(&sessions_dir, continue_, session)?;

    let agent = build_agent_from_config(
        &config,
        &provider_name,
        &model_id,
        resolve_boot_service_key(&config, &provider_name),
        registry.clone(),
        &sessions_dir,
        &session_id,
        system_prompt.clone(),
    )
    .ok();

    let agent_state_dir = sessions_dir.join("agent-state");
    let models_cfg = Arc::new(ModelsConfigState::load(&provider_name));
    let runtime = Arc::new(RwLock::new(Runtime {
        service: provider_name.clone(),
        model: model_id.clone(),
        agent,
        system_prompt: system_prompt.clone(),
    }));
    let state = AppState {
        web_root,
        config: Arc::new(config),
        models_cfg,
        sessions_dir: sessions_dir.clone(),
        runtime,
        verbose,
        session: Arc::new(RwLock::new(SessionHandle {
            rollout: session_rollout,
            session_id: session_id.clone(),
        })),
        registry,
        agent_state_dir,
        settings_path: PathBuf::from(axonerai::settings::DEFAULT_SETTINGS_PATH),
        key_lookup: Arc::new(|key| std::env::var(key).ok()),
    };

    let assets_dir = state.web_root.join("assets");
    let assets_service = tower_http::services::ServeDir::new(assets_dir);
    let src_service = tower_http::services::ServeDir::new(state.web_root.join("src"));
    let test_service = tower_http::services::ServeDir::new(state.web_root.join("test"));
    let generated_service = tower_http::services::ServeDir::new(state.web_root.join("generated"));

    let app = build_router(state.clone())
        .nest_service("/assets", assets_service)
        .nest_service("/src", src_service)
        .nest_service("/test", test_service)
        .nest_service("/generated", generated_service);

    let bind_addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .with_context(|| format!("invalid bind address: {host}:{port}"))?;

    let listener = tokio::net::TcpListener::bind(bind_addr)
        .await
        .with_context(|| format!("failed to bind {bind_addr}"))?;

    let actual_addr = listener
        .local_addr()
        .with_context(|| "failed to read local_addr()")?;

    println!();
    println!("agt serve");
    println!("  Web UI:     http://{actual_addr}/");
    println!("  WebSocket:  ws://{actual_addr}/ws");
    println!("  Web root:   {}", state.web_root.display());
    if state.runtime.read().unwrap().agent.is_none() {
        println!(
            "  Note: no service configured (set MISTRAL_API_KEY / GROQ_API_KEY / OPENCODE_API_KEY)"
        );
    }
    println!();
    println!("  Service:    {}", provider_name);
    println!("  Model:      {}", model_id);
    println!("  Session:    {session_id}");
    println!();

    axum::serve(listener, app)
        .await
        .with_context(|| "server exited with error")?;

    Ok(())
}

/// `agt session list` — opencode-style padded table, newest first.
fn session_list() -> anyhow::Result<()> {
    let sessions_dir = rollout::default_dir();
    let sessions = rollout::sessions_index(&sessions_dir)?;

    if sessions.is_empty() {
        println!(
            "No sessions yet. Run `agt serve` to start one (rollouts live in {}).",
            sessions_dir.display()
        );
        return Ok(());
    }

    let rows: Vec<(String, String, String)> = sessions
        .iter()
        .map(|s| (s.uuid.clone(), s.title.clone(), format_updated(s.updated)))
        .collect();

    let id_w = rows
        .iter()
        .map(|r| r.0.len())
        .max()
        .unwrap_or(0)
        .max("Session ID".len());
    let title_w = rows
        .iter()
        .map(|r| r.1.len())
        .max()
        .unwrap_or(0)
        .max("Title".len());
    let updated_w = rows
        .iter()
        .map(|r| r.2.len())
        .max()
        .unwrap_or(0)
        .max("Updated".len());

    println!(
        "{:<id_w$}  {:<title_w$}  {:<updated_w$}",
        "Session ID", "Title", "Updated"
    );
    for (id, title, updated) in &rows {
        println!(
            "{:<id_w$}  {:<title_w$}  {:<updated_w$}",
            id, title, updated
        );
    }
    Ok(())
}

/// Format an updated timestamp like `1:36 PM` for today, else
/// `11:27 PM · 9/1/2026`.
fn format_updated(updated_ms: u64) -> String {
    let Some(utc) = chrono::DateTime::from_timestamp_millis(updated_ms as i64) else {
        return String::from("?");
    };
    let dt = utc.with_timezone(&chrono::Local);
    if dt.date_naive() == chrono::Local::now().date_naive() {
        dt.format("%-I:%M %p").to_string()
    } else {
        format!("{} · {}", dt.format("%-I:%M %p"), dt.format("%-m/%-d/%Y"))
    }
}

/// The REST + WS routes (shared by `serve` and the tests, which construct
/// the router to prove every route registers without a matchit conflict —
/// e.g. static `/api/session/reset` next to parameterized
/// `/api/session/:uuid`).
fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/console.html", get(console_page))
        .route("/ws", get(ws_upgrade))
        .route("/api/sessions", get(api_sessions))
        .route("/api/session/reset", post(api_session_reset))
        .route("/api/session/:uuid", get(api_session_catchup))
        .route("/api/session/:uuid/tail", get(api_session_tail))
        .route("/api/state", get(api_state))
        .route("/api/services", get(api_services))
        .route("/api/models", get(api_models))
        .route("/api/skills", get(api_skills))
        .route("/api/tools", post(api_tools_toggle))
        .route("/api/mcp", post(api_mcp_toggle))
        .route("/api/model", post(api_model_swap))
        .route("/openapi.yaml", get(openapi_yaml))
        .fallback(get(index))
        .with_state(state)
}

async fn index(State(state): State<AppState>) -> Response {
    let disk_path = state.web_root.join("index.html");

    match tokio::fs::read_to_string(&disk_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html(include_str!("../web/index.html").to_string()).into_response(),
    }
}

/// `GET /console.html` — the devtools console popup (item32). Mirrors
/// `index`: disk copy first, embedded copy as the fallback. Without this
/// route the `.fallback(get(index))` would serve the CHAT page for
/// `/console.html`, breaking the `/console` popup on this origin.
async fn console_page(State(state): State<AppState>) -> Response {
    let disk_path = state.web_root.join("console.html");

    match tokio::fs::read_to_string(&disk_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html(include_str!("../web/console.html").to_string()).into_response(),
    }
}

/// GET /api/sessions — index of all rollouts, newest first.
async fn api_sessions() -> Response {
    let sessions: Vec<SessionInfo> =
        rollout::sessions_index(&rollout::default_dir()).unwrap_or_default();
    Json(sessions).into_response()
}

// --- Control-plane state snapshot (GET /api/state) ------------------------

#[derive(serde::Serialize)]
struct SessionSnapshot {
    id: String,
    title: String,
}

#[derive(serde::Serialize)]
struct RepoSnapshot {
    path: String,
    branch: Option<String>,
}

#[derive(serde::Serialize)]
struct ContextSnapshot {
    tokens: u64,
    /// Current model's context window in tokens from the provider-models
    /// config (item41); null when the config doesn't know the model. The
    /// browser's hardcoded window map stays as the fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    context_window: Option<u32>,
}

#[derive(serde::Serialize)]
struct McpServerInfo {
    name: String,
    status: String,
    /// Whether the server's tools are currently exposed to the model
    /// (item48: the panel's per-server toggle; false when the server's
    /// tools are all suppressed via POST /api/mcp).
    enabled: bool,
}

/// The control-plane snapshot the UI panel renders. Field order matches the
/// pinned API contract. item57: `provider` was renamed to `service` (the
/// services model; no lying aliases) — the server reports its SERVICE short
/// name.
#[derive(serde::Serialize)]
struct StateSnapshot {
    service: String,
    model: String,
    session: SessionSnapshot,
    repo: RepoSnapshot,
    context: ContextSnapshot,
    tools: Vec<ToolInfo>,
    mcp: Vec<McpServerInfo>,
    lsp: Vec<serde_json::Value>,
    todo: serde_json::Value,
}

/// Rename-aware session title for the control-plane snapshot: look the
/// session up in the sessions index (last `session_rename` wins), falling
/// back to the meta-only scan, then the session id.
fn session_title(state: &AppState) -> String {
    let session_id = state.session_id();
    if let Some(dir) = state.rollout().path().parent() {
        if let Ok(sessions) = rollout::sessions_index(dir) {
            if let Some(info) = sessions.iter().find(|s| s.uuid == session_id) {
                return info.title.clone();
            }
        }
    }
    state.rollout().title().unwrap_or_else(|_| session_id)
}

/// GET /api/state — service/model/session/repo/context/tools/mcp snapshot.
/// Context tokens are the model's context estimate: bytes/4 (floor) over the
/// JSON serialization of the session's provider messages (the per-session
/// agent-state file, system prompt included when loaded). Protocol events
/// (ready/session_meta/echo) are excluded entirely; a session with no
/// messages reports 0.
async fn api_state(State(state): State<AppState>) -> Response {
    Json(state_snapshot(&state)).into_response()
}

/// Build the control-plane snapshot the UI panel renders. Field order
/// matches the pinned API contract. Service/model/system-prompt state is
/// read from the shared [`Runtime`] so a /api/model swap is reflected
/// immediately.
fn state_snapshot(state: &AppState) -> StateSnapshot {
    let runtime = state.runtime.read().unwrap();
    // Title resolution must match the sessions index: the last `session_rename`
    // wins over the first `session_meta` (Rollout::title() only sees the meta).
    let title = session_title(state);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    StateSnapshot {
        service: runtime.service.clone(),
        model: runtime.model.clone(),
        session: SessionSnapshot {
            id: state.session_id(),
            title,
        },
        repo: RepoSnapshot {
            path: cwd.display().to_string(),
            branch: current_branch(&cwd),
        },
        context: ContextSnapshot {
            tokens: context_tokens_on_disk(
                &state.agent_state_dir,
                &state.session_id(),
                runtime.system_prompt.as_deref(),
            ),
            context_window: state.models_cfg.context_window(&runtime.model),
        },
        tools: state.registry.list_tools_info(),
        mcp: mcp_servers(&state.registry),
        lsp: vec![],
        todo: serde_json::Value::Null,
    }
}

/// Registered (fake) MCP servers, driven by the shared registry: a server
/// is listed exactly when it has registered `ToolSource::Mcp` facade tools
/// (in `build_registry` that coincides with its API key being present);
/// there is no MCP host process. `enabled` mirrors the registry's
/// per-server suppression (item48): false when POST /api/mcp has
/// suppressed all of the server's tools.
fn mcp_servers(registry: &ToolRegistry) -> Vec<McpServerInfo> {
    registry
        .mcp_servers()
        .into_iter()
        .map(|name| McpServerInfo {
            status: "connected".to_string(),
            enabled: registry.mcp_server_enabled(&name),
            name,
        })
        .collect()
}

/// Current git branch, parsed straight from `.git/HEAD` — NO subprocess.
/// `ref: refs/heads/<branch>` → branch; detached HEAD → short sha; not a
/// repo → None.
fn current_branch(cwd: &std::path::Path) -> Option<String> {
    let head = std::fs::read_to_string(cwd.join(".git").join("HEAD")).ok()?;
    let head = head.trim();
    if let Some(branch) = head.strip_prefix("ref: refs/heads/") {
        if branch.is_empty() {
            return None;
        }
        Some(branch.to_string())
    } else if head.is_empty() {
        None
    } else {
        Some(head.chars().take(7).collect())
    }
}

// --- Models config (GET /api/models) ----------------------------------------

/// One model row of the /api/models response (the item41 config shape).
#[derive(serde::Serialize)]
struct ModelsConfigEntry {
    id: String,
    display: String,
    context_window: u32,
    costs: Option<axonerai::models_config::ModelCosts>,
    offer: Option<String>,
}

#[derive(serde::Serialize)]
struct ModelsConfigResponse {
    provider: String,
    /// "local" | "user" | null — which file won (local masks user).
    source: Option<String>,
    /// Empty when no config file exists for the provider (missing-safe).
    models: Vec<ModelsConfigEntry>,
}

/// GET /api/models — the current provider's model list from the per-provider
/// models config (item41), for the /models tree. `?reload=1` re-reads the
/// config from disk first (the on-demand reload path). No config file →
/// `{provider, source: null, models: []}`.
async fn api_models(State(state): State<AppState>, Query(query): Query<ModelsQuery>) -> Response {
    if query.reload.unwrap_or(false) {
        state.models_cfg.reload();
    }

    let response = match state.models_cfg.get() {
        Some(loaded) => ModelsConfigResponse {
            provider: loaded.config.provider.clone(),
            source: Some(loaded.source.as_str().to_string()),
            models: loaded
                .config
                .models
                .iter()
                .map(|m| ModelsConfigEntry {
                    id: m.id.clone(),
                    display: m.display.clone(),
                    context_window: m.context_window,
                    costs: m.costs.clone(),
                    offer: m.offer.clone(),
                })
                .collect(),
        },
        None => ModelsConfigResponse {
            provider: state.runtime.read().unwrap().service.clone(),
            source: None,
            models: vec![],
        },
    };
    Json(response).into_response()
}

#[derive(serde::Deserialize)]
struct ModelsQuery {
    reload: Option<bool>,
}

// --- Services registry (GET /api/services) -----------------------------------

/// One entry of the /api/services response (item57).
#[derive(serde::Serialize)]
struct ServiceEntry {
    /// Service short name ("mistral", "groq", "opencode-zen", "opencode-go").
    service: String,
    /// Not named in the settings `disabled_services` list.
    enabled: bool,
    /// API key resolvable (config override or per-service env var, with the
    /// shared OPENCODE_API_KEY fallback for zen/go).
    connected: bool,
    /// The service's models-config roster (item41 files); empty array when
    /// no config file exists (missing-safe).
    models: Vec<ModelsConfigEntry>,
}

/// GET /api/services — the item57 services registry: one entry per known
/// service. Recomputed per request (settings + key lookup re-read — this IS
/// the reload path). N services = enabled AND key present; the browser
/// offers exactly those for activation.
async fn api_services(State(state): State<AppState>) -> Response {
    let disabled_services = state.load_settings().disabled_services;
    let entries = axonerai::services::KNOWN_SERVICES
        .iter()
        .map(|service| {
            let models = match models_config::load_from_dirs(
                &state.models_cfg.local,
                &state.models_cfg.user,
                service,
            ) {
                Ok(Some(loaded)) => loaded
                    .config
                    .models
                    .iter()
                    .map(|m| ModelsConfigEntry {
                        id: m.id.clone(),
                        display: m.display.clone(),
                        context_window: m.context_window,
                        costs: m.costs.clone(),
                        offer: m.offer.clone(),
                    })
                    .collect(),
                _ => vec![],
            };
            ServiceEntry {
                service: service.to_string(),
                enabled: axonerai::services::is_enabled(service, &disabled_services),
                connected: state.resolve_service_key(service).is_some(),
                models,
            }
        })
        .collect::<Vec<ServiceEntry>>();
    Json(entries).into_response()
}

// --- Skills listing (GET /api/skills) ----------------------------------------

/// GET /api/skills — every skill listed from `.axonerai/skills`,
/// `~/.axonerai/skills` and the built-in skills shipped in the binary
/// (LOCAL MASKS USER MASKS BUILTIN; item49 + item50). Missing-safe: a
/// missing dir contributes nothing; a broken SKILL.md is skipped server-side
/// with a stderr note. item54: built-ins named in the settings
/// `disabled_skills` list are dropped from the listing (source builtin ONLY
/// — local/user folder skills of the same name are unaffected).
async fn api_skills(State(state): State<AppState>) -> Response {
    let disabled: std::collections::HashSet<String> =
        state.load_settings().disabled_skills.into_iter().collect();
    Json(axonerai::skills::list_skills_filtered(&disabled)).into_response()
}

// --- Tool toggle (POST /api/tools) -----------------------------------------

#[derive(serde::Deserialize)]
struct ToolToggleBody {
    name: String,
    enabled: bool,
}

/// POST /api/tools `{"name": "<tool>", "enabled": bool}` — updates the
/// (shared) registry suppression and persists it to settings.jsonc.
async fn api_tools_toggle(
    State(state): State<AppState>,
    Json(body): Json<ToolToggleBody>,
) -> Response {
    if state.registry.get(&body.name).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": "unknown tool"})),
        )
            .into_response();
    }

    state.registry.set_suppressed(&body.name, body.enabled);

    // Load-modify-save: a full-settings rewrite must never reset the other
    // keys (item54 mcp_toggle_persist / disabled_mcp_servers /
    // disabled_skills).
    let mut settings = state.load_settings();
    settings.suppressed_tools = state.registry.suppressed_names();
    if let Err(e) = settings.save_to(&state.settings_path) {
        warn!("failed to persist settings: {e}");
    }

    Json(serde_json::json!({"ok": true})).into_response()
}

// --- MCP server toggle (POST /api/mcp) --------------------------------------

#[derive(serde::Deserialize)]
struct McpToggleBody {
    server: String,
    enabled: bool,
}

/// POST /api/mcp `{"server": "<name>", "enabled": bool}` — the panel-level
/// MCP toggle (item48). Reuses the proven suppression registry: it applies
/// per-tool suppression to EVERY `ToolSource::Mcp` tool reporting that
/// server name, so a disabled server's tools are hidden from the model and
/// error on execution for this session. Unknown server → 400 (same error
/// shape as /api/tools); the accepted set is exactly the servers with
/// registered facade tools (`registry.mcp_servers()`).
///
/// Restart semantics: by DEFAULT the MCP toggles are NOT persisted to
/// settings.jsonc — the BROWSER is the durable store (per-folder
/// localStorage, see web/src/mcp-prefs.mjs) and re-applies the stored
/// disabled servers by POSTing each one on boot. A server restart therefore
/// re-exposes all MCP tools until the browser reconnects and re-applies its
/// preference. item54 opt-in: when settings.jsonc sets
/// `"mcp_toggle_persist": true`, the toggle ALSO persists the server to
/// `disabled_mcp_servers` (load-modify-save; a re-enable removes it) so a
/// restart honours it without a browser re-apply — `build_registry` seeds
/// the suppression from that list at boot.
async fn api_mcp_toggle(
    State(state): State<AppState>,
    Json(body): Json<McpToggleBody>,
) -> Response {
    if !state.registry.mcp_servers().contains(&body.server) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"ok": false, "error": "unknown mcp server"})),
        )
            .into_response();
    }

    state
        .registry
        .set_mcp_suppressed(&body.server, body.enabled);

    // item54: server-side persistence is opt-in via the settings flag.
    let mut settings = state.load_settings();
    if settings.mcp_toggle_persist {
        if body.enabled {
            settings.disabled_mcp_servers.retain(|s| *s != body.server);
        } else if !settings.disabled_mcp_servers.contains(&body.server) {
            settings.disabled_mcp_servers.push(body.server.clone());
            settings.disabled_mcp_servers.sort();
        }
        if let Err(e) = settings.save_to(&state.settings_path) {
            warn!("failed to persist mcp toggle: {e}");
        }
    }

    Json(serde_json::json!({"ok": true})).into_response()
}

// --- Model/service swap (POST /api/model) ------------------------------------

#[derive(serde::Deserialize)]
struct ModelSwapBody {
    /// item57: the TARGET service. Absent = swap within the CURRENT service
    /// (back-compat with the pre-item57 `{"model": "..."}` body).
    service: Option<String>,
    model: String,
}

/// Is the model id known to EITHER the jsonc config roster for the service
/// OR the service's per-provider models config (item41)? Config ADDS to the
/// roster (the /api/model rule since item41).
fn service_knows_model(state: &AppState, service: &str, model: &str) -> bool {
    state.config.find_model(service, model).is_ok()
        || models_config::load_from_dirs(&state.models_cfg.local, &state.models_cfg.user, service)
            .ok()
            .flatten()
            .is_some_and(|loaded| loaded.find(model).is_some())
}

/// POST /api/model — hot-swap the service+model used for SUBSEQUENT agent
/// runs. item57 body `{"service": "<name>", "model": "<id>"}` swaps BOTH:
/// the provider is rebuilt from the service (mistral/groq dedicated
/// providers; opencode-zen/opencode-go → OpenCodeProvider with the
/// service's base URL) and the agent rebuilt with it (same tool registry,
/// same per-session FileSessionManager, freshly composed system prompt).
/// Back-compat: `{"model": "..."}` alone swaps within the CURRENT service.
///
/// Validation → 400: unknown service; service disabled in settings
/// (`disabled_services` — an unsubscribed service stays off even with the
/// shared key present); service with no API key (a swap would leave the
/// runtime agent-less); model not in the TARGET service's roster (config
/// roster OR models config). The swap is PROCESS-GLOBAL, which is also
/// per-session here (one session per server process), and NOT persisted —
/// a restart returns to the configured/default service+model. Responds
/// with the updated /api/state snapshot.
async fn api_model_swap(
    State(state): State<AppState>,
    Json(body): Json<ModelSwapBody>,
) -> Response {
    let current_service = state.runtime.read().unwrap().service.clone();
    let target_service = match &body.service {
        Some(requested) => {
            if !axonerai::services::is_known(requested) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "ok": false,
                        "error": format!("unknown service '{requested}'"),
                    })),
                )
                    .into_response();
            }
            let settings = state.load_settings();
            if !axonerai::services::is_enabled(requested, &settings.disabled_services) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "ok": false,
                        "error": format!(
                            "service '{requested}' is disabled in settings"
                        ),
                    })),
                )
                    .into_response();
            }
            if state.resolve_service_key(requested).is_none() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "ok": false,
                        "error": format!("no API key for service '{requested}'"),
                    })),
                )
                    .into_response();
            }
            requested.clone()
        }
        None => current_service,
    };

    if !service_knows_model(&state, &target_service, &body.model) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "ok": false,
                "error": format!(
                    "unknown model '{}' for service '{}'",
                    body.model, target_service
                ),
            })),
        )
            .into_response();
    }

    let system_prompt = Some(axonerai::prompt::load_system_prompt(
        &target_service,
        &body.model,
    ));
    let agent = build_agent_from_config(
        &state.config,
        &target_service,
        &body.model,
        state.resolve_service_key(&target_service),
        state.registry.clone(),
        &state.sessions_dir,
        &state.session_id(),
        system_prompt.clone(),
    )
    .ok();

    {
        let mut runtime = state.runtime.write().unwrap();
        runtime.service = target_service.clone();
        runtime.model = body.model.clone();
        runtime.system_prompt = system_prompt;
        runtime.agent = agent;
    }

    Json(state_snapshot(&state)).into_response()
}

// --- Session reset (POST /api/session/reset) ---------------------------------

/// POST /api/session/reset — start a FRESH session without a server restart
/// (item54, the REST-idiomatic eval seam): mints a new uuid v7 session +
/// rollout (session_meta first line, same shape as boot), rebuilds the agent
/// against the new per-session agent-state dir, and swaps both into the
/// shared session lock behind [`SessionHandle`]. Provider/model and the tool
/// registry (suppression state included) are untouched. Responds with the
/// updated /api/state snapshot. Repeat eval runs call this instead of
/// restarting the server; the reset is NOT persisted — a restart follows the
/// ordinary `--continue`/`--session`/boot semantics.
async fn api_session_reset(State(state): State<AppState>) -> Response {
    let (rollout, session_id) = match fresh_session(&state.sessions_dir) {
        Ok(pair) => pair,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "ok": false, "error": e.to_string() })),
            )
                .into_response();
        }
    };

    // Rebuild the agent for the new session id (same service/model and
    // system prompt — only the per-session FileSessionManager changes).
    let (service, model, system_prompt) = {
        let runtime = state.runtime.read().unwrap();
        (
            runtime.service.clone(),
            runtime.model.clone(),
            runtime.system_prompt.clone(),
        )
    };
    let agent = build_agent_from_config(
        &state.config,
        &service,
        &model,
        state.resolve_service_key(&service),
        state.registry.clone(),
        &state.sessions_dir,
        &session_id,
        system_prompt,
    )
    .ok();
    {
        let mut runtime = state.runtime.write().unwrap();
        runtime.agent = agent;
    }
    {
        let mut session = state
            .session
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *session = SessionHandle {
            rollout: Arc::new(rollout),
            session_id,
        };
    }

    Json(state_snapshot(&state)).into_response()
}

// --- OpenAPI document ------------------------------------------------------

const OPENAPI_YAML: &str = include_str!("openapi.yaml");

/// GET /openapi.yaml — hand-written OpenAPI 3.1 document for the REST API.
async fn openapi_yaml() -> Response {
    (
        [(
            HeaderName::from_static("content-type"),
            "application/yaml".to_string(),
        )],
        OPENAPI_YAML,
    )
        .into_response()
}

#[derive(serde::Deserialize)]
struct CatchupQuery {
    after: Option<u64>,
}

/// Format one rollout line for the catch-up stream: `<ts>\0<type>\0<text>\n`.
///
/// Linear scans only — NO serde_json parse of the payload on this path. The
/// `_type` is extracted with the shared literal scan from the `lineformat`
/// crate, and the payload is truncated at [`EGRESS_LINE_BYTES`] (char-boundary
/// safe) when oversized. `tool_trace` records are NEVER streamed (returns
/// `None`).
fn rollout_frame(ts: u64, json: &str) -> Option<String> {
    let event_type = lineformat::extract_type(json);
    if event_type == "tool_trace" {
        return None;
    }
    let (text, _truncated) = lineformat::truncate_payload(json, EGRESS_LINE_BYTES);
    Some(format!("{ts}\0{event_type}\0{text}\n"))
}

/// GET /api/session/:uuid?after=<ms> — chunked line-format catch-up stream
/// (`application/x-rollout-line`).
///
/// Streams `ts\0type\0text` frames (ts > after) straight off a streaming scan
/// of the rollout; the file is never loaded whole and the payload is never
/// JSON-parsed. The scan halts cleanly at the first corrupt line — the client
/// keeps everything received up to that point.
async fn api_session_catchup(
    Path(uuid): Path<String>,
    Query(query): Query<CatchupQuery>,
) -> Response {
    if !rollout::is_session_id(&uuid) {
        return (StatusCode::BAD_REQUEST, "invalid session id").into_response();
    }
    let after = query.after.unwrap_or(0);
    let sessions_dir = rollout::default_dir();
    let session_rollout = match Rollout::open(&sessions_dir, &uuid) {
        Ok(r) => r,
        Err(_) => return (StatusCode::NOT_FOUND, "session not found").into_response(),
    };

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<String, std::io::Error>>(64);
    tokio::task::spawn_blocking(move || {
        let result = session_rollout.scan_from(after, |ts, json| {
            if let Some(frame) = rollout_frame(ts, json) {
                tx.blocking_send(Ok(frame))
                    .map_err(|_| anyhow::anyhow!("catch-up stream closed"))?;
            }
            Ok(())
        });
        match result {
            Ok(()) => {}
            Err(e) if e.downcast_ref::<rollout::RolloutCorrupted>().is_some() => {
                // Corruption: halt the stream cleanly at the bad line. Data
                // before it has already been sent; the client's ts+type
                // high-watermark accepts exactly that prefix.
                warn!("catch-up stream halted: {e}");
            }
            Err(e) => {
                let _ = tx.blocking_send(Err(std::io::Error::other(e.to_string())));
            }
        }
    });

    let stream = futures_util::stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|item| (item, rx))
    });

    match Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/x-rollout-line")
        .body(Body::from_stream(stream))
    {
        Ok(response) => response.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// GET /api/session/:uuid/tail — `{"_ts":<last_ts>}` for frontier checks.
async fn api_session_tail(Path(uuid): Path<String>) -> Response {
    if !rollout::is_session_id(&uuid) {
        return (StatusCode::BAD_REQUEST, "invalid session id").into_response();
    }
    let sessions_dir = rollout::default_dir();
    match Rollout::open(&sessions_dir, &uuid).and_then(|r| r.last_ts()) {
        Ok(ts) => Json(serde_json::json!({ "_ts": ts })).into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "session not found").into_response(),
    }
}

async fn ws_upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(state, socket))
}

async fn ws_session(state: AppState, mut socket: WebSocket) {
    info!("[{}] ws client connected", state.session_id());

    // Outbound frames produced by spawned tasks (tool-trace forwarder, agent
    // run) flow through this channel; the main select loop writes them to the
    // socket while staying responsive to inbound client messages.
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Durable-first: append the ready frame to the rollout before sending it.
    let ready_value = serde_json::to_value(&ServerMsg::Ready {
        version: env!("CARGO_PKG_VERSION"),
        websocket_path: "/ws",
    })
    .unwrap_or_else(
        |_| serde_json::json!({"_type":"ready","version":"unknown","websocket_path":"/ws"}),
    );
    let _ = state.append_event(&ready_value);
    let _ = socket.send(WsMessage::Text(ready_value.to_string())).await;

    // Broadcast session metadata so the browser knows the session title.
    // Fresh sessions already carry a session_meta line (written at creation
    // in resolve_session); reopenings get one appended here so the frame is
    // durable too.
    let session_id = state.session_id();
    let meta_title = state
        .rollout()
        .title()
        .unwrap_or_else(|_| session_id.clone());
    let meta_value = serde_json::to_value(&ServerMsg::SessionMeta {
        session_id: &session_id,
        title: &meta_title,
        created_at: now_ms(),
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "_type": "session_meta",
            "session_id": session_id,
            "title": meta_title,
            "created_at": now_ms(),
        })
    });
    let _ = state.append_event(&meta_value);
    let _ = socket.send(WsMessage::Text(meta_value.to_string())).await;

    loop {
        tokio::select! {
            out = out_rx.recv() => match out {
                Some(text) => {
                    let _ = socket.send(WsMessage::Text(text)).await;
                }
                None => break,
            },
            msg = socket.recv() => match msg {
                Some(Ok(WsMessage::Text(text))) => {
                    let value: serde_json::Value = match serde_json::from_str(&text) {
                        Ok(v) => v,
                        Err(e) => {
                            send_error(
                                &state,
                                &mut socket,
                                None,
                                &format!("invalid message: {e}"),
                            )
                            .await;
                            continue;
                        }
                    };

                    // Durable-first: record the client frame before acting on it.
                    if value.is_object() {
                        let _ = state.append_event(&value);
                    }

                    let client_msg: ClientMsg = match serde_json::from_value(value) {
                        Ok(m) => m,
                        Err(e) => {
                            send_error(
                                &state,
                                &mut socket,
                                None,
                                &format!("invalid message: {e}"),
                            )
                            .await;
                            continue;
                        }
                    };

                    match client_msg {
                        ClientMsg::Ping { id } => {
                            let pong_value =
                                serde_json::to_value(&ServerMsg::Pong { id: id.as_deref() })
                                    .unwrap_or_else(|_| serde_json::json!({"_type":"pong"}));
                            let _ = state.append_event(&pong_value);
                            let _ =
                                socket.send(WsMessage::Text(pong_value.to_string())).await;
                        }
                        ClientMsg::Rename { title } => {
                            // Rollout-internal record — not broadcast to other
                            // clients; the renaming client gets an ack.
                            let record_value = serde_json::to_value(
                                &RolloutRecord::SessionRename { title, ts: now_ms() },
                            )
                            .unwrap_or_else(|_| {
                                serde_json::json!({"_type":"session_rename","ts":now_ms()})
                            });
                            let _ = state.append_event(&record_value);

                            let ack_value = serde_json::to_value(&ServerMsg::Ack {
                                for_type: "rename",
                                ok: true,
                                message: None,
                            })
                            .unwrap_or_else(|_| {
                                serde_json::json!({"_type":"ack","for_type":"rename","ok":true,"message":null})
                            });
                            let _ = state.append_event(&ack_value);
                            let _ = socket
                                .send(WsMessage::Text(ack_value.to_string()))
                                .await;
                        }
                        ClientMsg::Prompt { id, text } => {
                            if is_slash_command(&text) {
                                send_error(
                                    &state,
                                    &mut socket,
                                    id.as_deref(),
                                    "slash commands are control plane and never reach the model",
                                )
                                .await;
                                continue;
                            }

                            // Read the (swappable) agent from the shared
                            // runtime at prompt time so a /api/model swap
                            // applies to this and every subsequent run.
                            let agent = state
                                .runtime
                                .read()
                                .ok()
                                .and_then(|runtime| runtime.agent.clone());
                            let Some(agent) = &agent else {
                                send_error(
                                    &state,
                                    &mut socket,
                                    id.as_deref(),
                                    "No provider configured. Set MISTRAL_API_KEY / OPENCODE_API_KEY / GROQ_API_KEY.",
                                )
                                .await;
                                continue;
                            };

                            // Per-prompt trace channel: each tool execution
                            // streams a ToolTrace which is forwarded as an
                            // abridged tool_call frame (durable-first) while
                            // the rollout also keeps a full-fidelity record.
                            let (trace_tx, mut trace_rx) =
                                tokio::sync::mpsc::unbounded_channel::<ToolTrace>();
                            let fwd_state = state.clone();
                            let fwd_out = out_tx.clone();
                            let fwd_session_id = state.session_id();
                            tokio::spawn(async move {
                                while let Some(trace) = trace_rx.recv().await {
                                    let args_pretty = rollout::abridge(
                                        &trace.args_json,
                                        EGRESS_ABRIDGE_CHARS,
                                    );
                                    let result_pretty = rollout::abridge(
                                        &trace.result_json,
                                        EGRESS_ABRIDGE_CHARS,
                                    );
                                    let frame = match serde_json::to_string(&ServerMsg::ToolCall {
                                        id: None,
                                        session_id: &fwd_session_id,
                                        tool: &trace.tool,
                                        args_pretty: &args_pretty,
                                        result_pretty: &result_pretty,
                                        bytes_up: trace.bytes_up,
                                        bytes_down: trace.bytes_down,
                                        duration_ms: trace.duration_ms,
                                        ts: trace.ts,
                                    }) {
                                        Ok(frame) => frame,
                                        Err(_) => {
                                            serde_json::json!({"_type":"tool_call","tool":trace.tool}).to_string()
                                        }
                                    };
                                    // Durable-first: append (raw, so the manual
                                    // payload-last field order survives on
                                    // disk), then send the same bytes.
                                    let _ = fwd_state.append_json(&frame);
                                    let _ = fwd_out.send(frame);

                                    // Separate full-fidelity record — never
                                    // abridged, so the rollout keeps the whole
                                    // tool result. Raw serialization keeps the
                                    // payload-last byte order on disk.
                                    let full_json = match serde_json::to_string(
                                        &RolloutRecord::ToolTrace {
                                            tool: trace.tool,
                                            args_json: trace.args_json,
                                            result_json: trace.result_json,
                                            bytes_up: trace.bytes_up,
                                            bytes_down: trace.bytes_down,
                                            duration_ms: trace.duration_ms,
                                            ts: trace.ts,
                                        },
                                    ) {
                                        Ok(json) => json,
                                        Err(_) => serde_json::json!({"_type":"tool_trace"}).to_string(),
                                    };
                                    let _ = fwd_state.append_json(&full_json);
                                }
                            });

                            let run_agent = agent.clone();
                            let run_state = state.clone();
                            let run_out = out_tx.clone();
                            let run_id = id.clone();
                            tokio::spawn(async move {
                                let result =
                                    run_agent.run_with_traces(text.trim(), trace_tx).await;
                                match result {
                                    Ok(reply) => {
                                        let timestamp = chrono::DateTime::<chrono::Utc>::from(
                                            SystemTime::now(),
                                        )
                                        .to_rfc3339_opts(
                                            chrono::SecondsFormat::Millis,
                                            true,
                                        );

                                        match run_state.verbose {
                                            0 => info!(
                                                "[{}] Response: {} bytes",
                                                timestamp,
                                                reply.len()
                                            ),
                                            1 => {
                                                let preview = if reply.len() > 77 {
                                                    format!("{}...", &reply[..77])
                                                } else {
                                                    reply.clone()
                                                };
                                                info!(
                                                    "[{}] Response: {} bytes - {}",
                                                    timestamp,
                                                    reply.len(),
                                                    preview
                                                );
                                            }
                                            _ => info!(
                                                "[{}] Response: {} bytes\n{}",
                                                timestamp,
                                                reply.len(),
                                                reply
                                            ),
                                        }

                                        let assistant_value = serde_json::to_value(
                                            &ServerMsg::Assistant {
                                                id: run_id.as_deref(),
                                                text: &reply,
                                            },
                                        )
                                        .unwrap_or_else(|_| {
                                            serde_json::json!({"_type":"assistant","text":"(serialization error)"})
                                        });
                                        let _ =
                                            run_state.append_event(&assistant_value);
                                        let _ = run_out.send(assistant_value.to_string());
                                    }
                                    Err(e) => {
                                        let error_value = serde_json::to_value(&ServerMsg::Error {
                                            id: run_id.as_deref(),
                                            message: &format!("agent error: {e}"),
                                        })
                                        .unwrap_or_else(|_| {
                                            serde_json::json!({"_type":"error","message":"agent error"})
                                        });
                                        let _ =
                                            run_state.append_event(&error_value);
                                        let _ = run_out.send(error_value.to_string()).ok();
                                    }
                                }
                            });
                        }
                    }
                }
                Some(Ok(_)) => {}
                _ => break,
            },
        }
    }
}

/// Build, append (durable-first) and send an error frame.
async fn send_error(state: &AppState, socket: &mut WebSocket, id: Option<&str>, message: &str) {
    let error_value = serde_json::to_value(&ServerMsg::Error { id, message })
        .unwrap_or_else(|_| serde_json::json!({"_type":"error","message":"error"}));
    let _ = state.append_event(&error_value);
    let _ = socket.send(WsMessage::Text(error_value.to_string())).await;
}

/// Build the tool registry: builtins plus the Tavily-backed web tools and
/// the fake Tavily MCP facade tools (only when an API key is available).
/// Suppressed tool names are seeded from the persisted local settings.
fn build_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(Calculator));
    registry.register(Box::new(WriteFile::default()));
    registry.register(Box::new(ReadFile::default()));
    registry.register(Box::new(ListDir::default()));
    registry.register(Box::new(ReadSkill::default()));
    registry.register(Box::new(ModelsConfig::new()));

    // Only register the Tavily-backed web tools when an API key is available.
    if std::env::var("TAVILY_API_KEY").is_ok() {
        registry.register(Box::new(WebSearch::new()));
        registry.register(Box::new(WebFetch::new()));
        // Fake Tavily MCP facade: MCP-style tools with no MCP host process.
        registry.register(Box::new(TavilyMcpSearch::new()));
        registry.register(Box::new(TavilyMcpExtract::new()));
    }

    // Only register the Context7 MCP facade tools when an API key is available.
    if axonerai::tools::context7_mcp::is_configured() {
        // Fake Context7 MCP facade: MCP-style tools with no MCP host process.
        registry.register(Box::new(Context7McpResolveLibraryId::new()));
        registry.register(Box::new(Context7McpGetLibraryDocs::new()));
    }

    // Seed per-tool suppression from the persisted settings so a restart
    // restores the previous on/off state. item54: persisted disabled MCP
    // servers (written only when `mcp_toggle_persist` is on) seed the same
    // way; unknown servers are a no-op.
    let settings = Settings::load();
    for name in &settings.suppressed_tools {
        registry.set_suppressed(name, false);
    }
    for server in &settings.disabled_mcp_servers {
        registry.set_mcp_suppressed(server, false);
    }

    registry
}

/// Resolve the boot service key from the config override + process env
/// (the production key-lookup path; tests inject their own via
/// `AppState.key_lookup`).
fn resolve_boot_service_key(config: &AppConfig, service: &str) -> Option<String> {
    let config_override = config
        .providers
        .get(service)
        .and_then(|p| p.api_key.clone());
    axonerai::services::resolve_key(service, config_override.as_deref(), |key| {
        std::env::var(key).ok()
    })
}

fn build_agent_from_config(
    config: &AppConfig,
    service: &str,
    model_id: &str,
    api_key: Option<String>,
    registry: ToolRegistry,
    sessions_dir: &std::path::Path,
    session_id: &str,
    system_prompt: Option<String>,
) -> anyhow::Result<Arc<Agent>> {
    let api_key = api_key.ok_or_else(|| anyhow::anyhow!("no API key for service '{service}'"))?;
    // The service → provider mapping (item57): mistral/groq have dedicated
    // providers, the two opencode endpoints share OpenCodeProvider differing
    // only by base URL. An axonerai.jsonc endpoint override wins.
    let provider = axonerai::services::build_provider(
        service,
        &api_key,
        model_id,
        config.endpoint(service).ok(),
    )?;

    let session_manager =
        FileSessionManager::new(session_id.to_string(), sessions_dir.join("agent-state"))?;

    Ok(Arc::new(Agent::new(
        provider,
        registry,
        system_prompt,
        Some(session_manager),
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollout_frame_emits_line_format_for_normal_events() {
        let ts = 1_717_238_400_000u64;
        let json = r#"{"_type":"assistant","text":"hi"}"#;
        let frame = rollout_frame(ts, json).unwrap();
        assert_eq!(frame, format!("{ts}\0assistant\0{json}\n"));

        // No `_type` in the payload → empty type field, payload intact.
        let frame = rollout_frame(ts, r#"{"x":1}"#).unwrap();
        assert_eq!(frame, format!("{ts}\0\0{{\"x\":1}}\n"));
    }

    #[test]
    fn rollout_frame_never_streams_tool_trace_records() {
        let ts = 1_717_238_400_000u64;
        let json = r#"{"_type":"tool_trace","tool":"WebSearch","result_json":"big"}"#;
        assert!(rollout_frame(ts, json).is_none());
    }

    #[test]
    fn rollout_frame_truncates_oversized_payload_detectably() {
        let ts = 1_717_238_400_000u64;
        let big = format!(
            "{{\"_type\":\"tool_call\",\"tool\":\"WebSearch\",\"result_json\":\"{}\"}}",
            "x".repeat(3000)
        );
        let frame = rollout_frame(ts, &big).unwrap();
        assert!(frame.ends_with('\n'));
        let payload = frame.trim_end_matches('\n');
        let mut parts = payload.split('\0');
        assert_eq!(parts.next().unwrap(), ts.to_string());
        assert_eq!(parts.next().unwrap(), "tool_call");
        let text = parts.next().unwrap();
        assert_eq!(text.len(), EGRESS_LINE_BYTES, "cut exactly at the limit");
        assert!(
            text.contains("\"tool\":\"WebSearch\""),
            "metadata survives: {text}"
        );
        // Truncation-detection rule: strict parse of a cut payload fails, so
        // the browser can deterministically mark it partial.
        assert!(serde_json::from_str::<serde_json::Value>(text).is_err());
    }

    #[test]
    fn rollout_frame_keeps_small_payloads_untouched() {
        let ts = 1_717_238_400_000u64;
        let json = r#"{"_type":"tool_call","result_pretty":"\"x\""}"#;
        let frame = rollout_frame(ts, json).unwrap();
        assert!(frame.ends_with(&format!("\0{json}\n")));
    }

    #[test]
    fn session_uuid_gate_rejects_adversarial_ids() {
        let valid = "01890a5d-ac96-774b-bcce-b302099a8057";
        assert!(rollout::is_session_id(valid));
        for bad in [
            "550e8400-e29b-41d4-a716-446655440000", // v4
            "01890A5D-AC96-774B-BCCE-B302099A8057", // uppercase
            "../../etc/passwd",                     // traversal
            "01890a5d",                             // wrong length
        ] {
            assert!(!rollout::is_session_id(bad), "{bad} must be rejected");
        }
    }

    #[test]
    fn slash_prompts_are_control_plane() {
        for cmd in ["/model", "/help", "/rename foo", "/  ", "/unknown"] {
            assert!(is_slash_command(cmd), "{cmd} must be control plane");
        }
        for chat in ["", "what is 2^4", "the /model flag", "  use /help maybe  "] {
            assert!(!is_slash_command(chat), "'{chat}' must be chat");
        }
    }

    /// Build a minimal AppState for handler tests: temp rollout dir, mistral
    /// config with a direct api_key (no env/network), agent prebuilt for the
    /// default roster model, and a per-provider models config loaded from a
    /// temp `.axonerai/models` dir (item41) that knows a context window for
    /// the roster's second model only — mirroring "config ADDS to the
    /// fallbacks". The injected key lookup (`keys`) backs service
    /// resolution (item57) deterministically without env mutation.
    fn test_state() -> AppState {
        test_state_with_keys(build_registry(), &[("MISTRAL_API_KEY", "test-key")])
    }

    /// [`test_state`] with an explicit registry (item48 tests inject one
    /// with the MCP facade tools registered regardless of API-key env).
    fn test_state_with(registry: ToolRegistry) -> AppState {
        test_state_with_keys(registry, &[("MISTRAL_API_KEY", "test-key")])
    }

    /// [`test_state`] with an explicit key lookup map (item57 test seam).
    fn test_state_with_keys(registry: ToolRegistry, keys: &[(&str, &str)]) -> AppState {
        let dir = std::env::temp_dir().join(format!("axoner-web-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        let session_id = uuid::Uuid::now_v7().to_string();
        let rollout = Rollout::create(&dir, &session_id).unwrap();
        let mut config = AppConfig::defaults();
        {
            // Mirror the .axonerai/axonerai.jsonc mistral roster (the
            // built-in defaults only carry the one model).
            let mistral = config.providers.get_mut("mistral").unwrap();
            mistral.api_key = Some("test-key".to_string());
            mistral.models.push(axonerai::config::ModelConfig {
                id: "mistral-medium-latest".to_string(),
                name: Some("Mistral Medium".to_string()),
                thinking: false,
                thinking_levels: vec![],
            });
        }

        // item41: a models config file that knows zai-glm-5-2's window.
        let models_dir = dir.join(".axonerai/models");
        std::fs::create_dir_all(&models_dir).unwrap();
        std::fs::write(
            models_dir.join("mistral-models.jsonc"),
            r#"{
                "provider": "mistral",
                "updated": "2026-09-03",
                "models": [
                    {"id": "zai-glm-5-2", "display": "GLM-5.2", "context_window": 32768,
                     "costs": {"input_per_mtok": "$0.50", "output_per_mtok": "$1.50"}}
                ]
            }"#,
        )
        .unwrap();
        let models_cfg = Arc::new(ModelsConfigState::load_from(
            &models_dir,
            &dir.join("user-models"),
            "mistral",
        ));

        let agent = build_agent_from_config(
            &config,
            "mistral",
            "zai-glm-5-2",
            Some("test-key".to_string()),
            registry.clone(),
            &dir,
            &session_id,
            None,
        )
        .ok();
        let key_map: std::collections::HashMap<String, String> = keys
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        AppState {
            web_root: dir.clone(),
            config: Arc::new(config),
            models_cfg,
            sessions_dir: dir.clone(),
            runtime: Arc::new(RwLock::new(Runtime {
                service: "mistral".to_string(),
                model: "zai-glm-5-2".to_string(),
                agent,
                system_prompt: None,
            })),
            verbose: 0,
            session: Arc::new(RwLock::new(SessionHandle {
                rollout: Arc::new(rollout),
                session_id,
            })),
            registry,
            agent_state_dir: dir.join("agent-state"),
            settings_path: dir.join("settings.jsonc"),
            key_lookup: Arc::new(move |key| key_map.get(key).cloned()),
        }
    }

    /// [`test_state`] with a settings file pre-seeded from `jsonc` (item54
    /// test seam: the state's settings_path points at the temp file).
    fn test_state_with_settings(registry: ToolRegistry, jsonc: &str) -> AppState {
        let state = test_state_with(registry);
        std::fs::write(&state.settings_path, jsonc).expect("write settings");
        state
    }

    #[tokio::test]
    async fn api_model_swap_updates_state_and_subsequent_runs() {
        let state = test_state();
        let response = api_model_swap(
            State(state.clone()),
            Json(ModelSwapBody {
                service: None,
                model: "mistral-medium-latest".to_string(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["model"], "mistral-medium-latest");
        assert_eq!(json["service"], "mistral");
        assert_eq!(json["session"]["id"], state.session_id());

        // The swap is visible to any later reader of the shared runtime: the
        // next prompt run rebuilds from this and /api/state reports it.
        let runtime = state.runtime.read().unwrap();
        assert_eq!(runtime.model, "mistral-medium-latest");
        assert!(runtime.agent.is_some(), "agent rebuilt for the new model");
        assert!(runtime.system_prompt.is_some());
    }

    #[tokio::test]
    async fn api_model_swap_unknown_model_is_400() {
        let state = test_state();
        let response = api_model_swap(
            State(state.clone()),
            Json(ModelSwapBody {
                service: None,
                model: "not-a-model".to_string(),
            }),
        )
        .await;

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], false);
        assert!(
            json["error"].as_str().unwrap().contains("not-a-model"),
            "error names the rejected model: {json}"
        );

        // A refused swap leaves the runtime untouched.
        assert_eq!(state.runtime.read().unwrap().model, "zai-glm-5-2");
    }

    // --- item41: models config on the wire ----------------------------------

    /// GET /api/state footer percent follows the models config: the snapshot
    /// carries the current model's `context_window` from the config (the
    /// item40 hardcoded browser fallback only fires when this is absent).
    #[tokio::test]
    async fn api_state_context_window_follows_models_config() {
        let state = test_state();
        let snapshot = state_snapshot(&state);

        assert_eq!(
            snapshot.context.context_window,
            Some(32768),
            "the config's window (not the hardcoded 131072) wins"
        );

        // A model the config does not know reports no window (the browser
        // then falls back to its hardcoded map).
        state.runtime.write().unwrap().model = "mistral-medium-latest".to_string();
        let snapshot = state_snapshot(&state);
        assert_eq!(snapshot.context.context_window, None);
    }

    /// GET /api/models serves the current provider's config list, noting the
    /// winning source; `?reload=1` re-reads from disk.
    #[tokio::test]
    async fn api_models_serves_config_with_source() {
        let state = test_state();

        let response = api_models(State(state.clone()), Query(ModelsQuery { reload: None })).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["provider"], "mistral");
        assert_eq!(json["source"], "local");
        let models = json["models"].as_array().unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0]["id"], "zai-glm-5-2");
        assert_eq!(models[0]["context_window"], 32768);
        assert_eq!(models[0]["costs"]["input_per_mtok"], "$0.50");

        // The reload path picks up an on-disk change.
        let new_body = r#"{
            "provider": "mistral",
            "updated": "2026-09-03",
            "models": [{"id": "zai-glm-5-2", "display": "GLM-5.2", "context_window": 65536}]
        }"#;
        let models_dir = state.web_root.join(".axonerai/models");
        std::fs::write(models_dir.join("mistral-models.jsonc"), new_body).unwrap();

        let response = api_models(
            State(state.clone()),
            Query(ModelsQuery { reload: Some(true) }),
        )
        .await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            json["models"][0]["context_window"], 65536,
            "?reload=1 re-reads disk"
        );
    }

    /// POST /api/model accepts any model id in the provider's models config
    /// list even when the jsonc roster does not know it; still 400 when
    /// neither config knows the id.
    #[tokio::test]
    async fn api_model_swap_accepts_models_config_only_ids() {
        let state = test_state();
        // Extend the models config (not the roster) with a new model id.
        let new_body = r#"{
            "provider": "mistral",
            "updated": "2026-09-03",
            "models": [
                {"id": "zai-glm-5-2", "display": "GLM-5.2", "context_window": 32768},
                {"id": "mistral-large-latest", "display": "Mistral Large", "context_window": 131072}
            ]
        }"#;
        let models_dir = state.web_root.join(".axonerai/models");
        std::fs::write(models_dir.join("mistral-models.jsonc"), new_body).unwrap();
        state.models_cfg.reload();

        assert!(
            state
                .config
                .find_model("mistral", "mistral-large-latest")
                .is_err(),
            "precondition: the roster does NOT know this id"
        );

        let response = api_model_swap(
            State(state.clone()),
            Json(ModelSwapBody {
                service: None,
                model: "mistral-large-latest".to_string(),
            }),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "config ADDS to the roster"
        );
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["model"], "mistral-large-latest");
        assert_eq!(state.runtime.read().unwrap().model, "mistral-large-latest");

        // Unknown to BOTH configs → 400, unchanged.
        let response = api_model_swap(
            State(state.clone()),
            Json(ModelSwapBody {
                service: None,
                model: "not-in-any-config".to_string(),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(state.runtime.read().unwrap().model, "mistral-large-latest");
    }

    // --- item57: services model ----------------------------------------------

    /// Handler test shorthand: POST /api/model and parse the JSON body.
    async fn swap(
        state: &AppState,
        service: Option<&str>,
        model: &str,
    ) -> (StatusCode, serde_json::Value) {
        let response = api_model_swap(
            State(state.clone()),
            Json(ModelSwapBody {
                service: service.map(str::to_string),
                model: model.to_string(),
            }),
        )
        .await;
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    /// /api/state carries `service` (the honest rename); the old `provider`
    /// field is GONE, not aliased.
    #[tokio::test]
    async fn api_state_reports_service_and_drops_provider() {
        let state = test_state();
        let bytes = axum::body::to_bytes(
            api_state(State(state.clone())).await.into_body(),
            usize::MAX,
        )
        .await
        .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["service"], "mistral");
        assert_eq!(json["model"], "zai-glm-5-2");
        assert!(
            json.get("provider").is_none(),
            "no lying alias: `provider` must not be on the wire"
        );
    }

    /// GET /api/services lists every known service with its connection
    /// state, the settings disabled list, and the per-service models
    /// roster (empty array when no config file).
    #[tokio::test]
    async fn api_services_lists_known_services_with_connection_state() {
        let state = test_state_with_keys(
            build_registry(),
            &[
                ("MISTRAL_API_KEY", "m-key"),
                ("GROQ_API_KEY", "g-key"),
                // Only the SHARED opencode key: both zen and go connect
                // through the fallback (the glossing).
                ("OPENCODE_API_KEY", "shared"),
            ],
        );
        std::fs::write(&state.settings_path, r#"{ "disabled_services": ["groq"] }"#).unwrap();

        let response = api_services(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let entries = json.as_array().expect("a JSON array of service entries");

        let by_service = |name: &str| {
            entries
                .iter()
                .find(|s| s["service"] == name)
                .unwrap_or_else(|| panic!("service {name} missing from {json}"))
        };
        assert_eq!(entries.len(), 4, "every known service is listed");

        let mistral = by_service("mistral");
        assert_eq!(mistral["enabled"], true);
        assert_eq!(mistral["connected"], true);
        let models = mistral["models"].as_array().unwrap();
        assert!(!models.is_empty(), "mistral has a roster from the config");
        assert_eq!(models[0]["id"], "zai-glm-5-2");
        assert_eq!(models[0]["display"], "GLM-5.2");
        assert_eq!(models[0]["context_window"], 32768);
        assert_eq!(models[0]["costs"]["input_per_mtok"], "$0.50");

        let groq = by_service("groq");
        assert_eq!(groq["enabled"], false, "settings disabled_services wins");
        assert_eq!(groq["connected"], true, "disabled ≠ unconnected");
        assert!(
            groq["models"].as_array().unwrap().is_empty(),
            "no groq models file → empty roster array"
        );

        // zen/go glossing: the shared key connects both.
        for name in ["opencode-zen", "opencode-go"] {
            let entry = by_service(name);
            assert_eq!(entry["enabled"], true);
            assert_eq!(entry["connected"], true, "{name} via shared key");
            assert!(entry["models"].is_array());
        }
    }

    /// GET /api/services recomputes per request (the reload path): a
    /// settings change and a key change are visible on the next GET with
    /// no restart.
    #[tokio::test]
    async fn api_services_rereads_settings_each_request() {
        let state = test_state_with_keys(build_registry(), &[("MISTRAL_API_KEY", "m-key")]);

        let read_enabled = |json: &serde_json::Value, name: &str| {
            json.as_array()
                .unwrap()
                .iter()
                .find(|s| s["service"] == name)
                .unwrap()["enabled"]
                .as_bool()
                .unwrap()
        };
        let read_connected = |json: &serde_json::Value, name: &str| {
            json.as_array()
                .unwrap()
                .iter()
                .find(|s| s["service"] == name)
                .unwrap()["connected"]
                .as_bool()
                .unwrap()
        };

        let response = api_services(State(state.clone())).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(read_enabled(&json, "groq"));
        assert!(!read_connected(&json, "groq"), "no GROQ_API_KEY injected");

        // Persist a disable, then GET again — no restart.
        std::fs::write(
            &state.settings_path,
            r#"{ "disabled_services": ["groq", "opencode-zen"] }"#,
        )
        .unwrap();
        let response = api_services(State(state.clone())).await;
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(!read_enabled(&json, "groq"));
        assert!(!read_enabled(&json, "opencode-zen"));
        assert!(read_enabled(&json, "mistral"), "others unaffected");
    }

    /// POST /api/model {"service","model"} hot-swaps BOTH: the runtime
    /// carries the new service + model and the agent is rebuilt against
    /// the SAME session (registry/FileSessionManager carried over).
    #[tokio::test]
    async fn api_model_swap_service_and_model_hot_swaps_runtime() {
        let state = test_state_with_keys(build_registry(), &[("GROQ_API_KEY", "g-key")]);
        let session_id = state.session_id();

        let (status, json) = swap(&state, Some("groq"), "openai/gpt-oss-120b").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(json["service"], "groq");
        assert_eq!(json["model"], "openai/gpt-oss-120b");
        assert!(json.get("provider").is_none());

        let runtime = state.runtime.read().unwrap();
        assert_eq!(runtime.service, "groq");
        assert_eq!(runtime.model, "openai/gpt-oss-120b");
        let agent = runtime.agent.as_ref().expect("agent rebuilt");
        assert_eq!(
            agent.session_id().expect("session-backed agent"),
            session_id,
            "the rebuilt agent keeps the same per-session manager"
        );
        assert!(runtime.system_prompt.is_some());
    }

    /// The zen/go shared-key fallback works at the handler level: swapping
    /// to opencode-zen with ONLY OPENCODE_API_KEY set succeeds.
    #[tokio::test]
    async fn api_model_swap_to_zen_falls_back_to_shared_key() {
        let state = test_state_with_keys(build_registry(), &[("OPENCODE_API_KEY", "shared")]);
        let (status, json) = swap(&state, Some("opencode-zen"), "glm-5.2").await;
        assert_eq!(status, StatusCode::OK, "{json}");
        assert_eq!(json["service"], "opencode-zen");
        assert_eq!(state.runtime.read().unwrap().service, "opencode-zen");
    }

    /// Unknown service → 400, runtime untouched.
    #[tokio::test]
    async fn api_model_swap_unknown_service_is_400() {
        let state = test_state();
        let (status, json) = swap(&state, Some("nope"), "whatever").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(json["ok"], false);
        assert!(
            json["error"].as_str().unwrap().contains("nope"),
            "error names the rejected service: {json}"
        );
        let runtime = state.runtime.read().unwrap();
        assert_eq!(runtime.service, "mistral");
        assert_eq!(runtime.model, "zai-glm-5-2");
    }

    /// Model not in the TARGET service's roster → 400, runtime untouched.
    #[tokio::test]
    async fn api_model_swap_model_not_in_service_roster_is_400() {
        let state = test_state_with_keys(build_registry(), &[("GROQ_API_KEY", "g-key")]);
        let (status, json) = swap(&state, Some("groq"), "not-a-groq-model").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json["error"].as_str().unwrap().contains("groq"),
            "error names the target service: {json}"
        );
        assert_eq!(state.runtime.read().unwrap().service, "mistral");
    }

    /// A service disabled in settings is refused even with its key present
    /// (the shared-key subscriber protection).
    #[tokio::test]
    async fn api_model_swap_disabled_service_is_400() {
        let state = test_state_with_keys(
            build_registry(),
            &[("MISTRAL_API_KEY", "m"), ("GROQ_API_KEY", "g")],
        );
        std::fs::write(&state.settings_path, r#"{ "disabled_services": ["groq"] }"#).unwrap();
        let (status, json) = swap(&state, Some("groq"), "openai/gpt-oss-120b").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json["error"].as_str().unwrap().contains("disabled"),
            "error names the disable reason: {json}"
        );
        assert_eq!(state.runtime.read().unwrap().service, "mistral");
    }

    /// A service with no resolvable key is refused (a swap would leave the
    /// runtime agent-less).
    #[tokio::test]
    async fn api_model_swap_unconnected_service_is_400() {
        let state = test_state_with_keys(build_registry(), &[("MISTRAL_API_KEY", "m")]);
        let (status, json) = swap(&state, Some("groq"), "openai/gpt-oss-120b").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(
            json["error"].as_str().unwrap().contains("no API key"),
            "error names the missing key: {json}"
        );
        assert_eq!(state.runtime.read().unwrap().service, "mistral");
    }

    // --- item49 + item50: skills listing (GET /api/skills) -------------------

    /// GET /api/skills lists the repo's `.axonerai/skills` entries (all
    /// `local`), including the item49 deterministic `greeting` test skill,
    /// plus the item50 built-in skills (source `builtin`). Deterministic
    /// despite any user-dir content: local masks user masks builtin, and the
    /// names asserted here come from the repo/binary itself.
    #[tokio::test]
    async fn api_skills_lists_local_repo_skills() {
        let state = test_state();
        let response = api_skills(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let skills = json.as_array().expect("a JSON array of skill entries");

        let by_name = |name: &str| {
            skills
                .iter()
                .find(|s| s["name"] == name)
                .unwrap_or_else(|| panic!("skill {name} missing from {json}"))
        };
        for expected in ["greeting", "lint", "update-model-costs"] {
            let entry = by_name(expected);
            assert_eq!(entry["source"], "local", "{expected} is a repo skill");
            assert!(
                entry["description"].as_str().unwrap_or("").len() > 0,
                "{expected} carries a frontmatter description"
            );
            assert!(
                entry["path"].as_str().unwrap_or("").ends_with("SKILL.md"),
                "{expected} points at its SKILL.md"
            );
        }
        // item50: the built-in flagship ships in the listing too.
        let deepresearch = by_name("deepresearch");
        assert_eq!(deepresearch["source"], "builtin");
        assert!(
            deepresearch["path"]
                .as_str()
                .unwrap_or("")
                .starts_with("skills/builtin/"),
            "the builtin points at its in-code source: {deepresearch}"
        );
    }

    // --- item54: per-skill activation toggles (GET /api/skills) --------------

    /// A builtin named in settings `disabled_skills` is dropped from the
    /// /api/skills listing; local folder skills are untouched.
    #[tokio::test]
    async fn api_skills_drops_settings_disabled_builtins_only() {
        let state = test_state_with_settings(
            build_registry(),
            r#"{ "disabled_skills": ["deepresearch", "no-such-builtin"] }"#,
        );

        let response = api_skills(State(state)).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let skills = json.as_array().expect("a JSON array of skill entries");

        assert!(
            !skills.iter().any(|s| s["name"] == "deepresearch"),
            "the disabled builtin must not be listed: {json}"
        );
        // Local folder skills are unaffected (repo .axonerai/skills).
        for expected in ["greeting", "lint", "update-model-costs"] {
            let entry = skills
                .iter()
                .find(|s| s["name"] == expected)
                .unwrap_or_else(|| panic!("local skill {expected} missing from {json}"));
            assert_eq!(entry["source"], "local", "{expected} unaffected");
        }
    }

    // --- item48: MCP server toggle (POST /api/mcp) ---------------------------

    /// Registry with the MCP facade tools registered regardless of the
    /// API-key environment (the /api/mcp tests must be deterministic).
    fn mcp_test_registry() -> ToolRegistry {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(Calculator));
        registry.register(Box::new(TavilyMcpSearch::new()));
        registry.register(Box::new(TavilyMcpExtract::new()));
        registry.register(Box::new(Context7McpResolveLibraryId::new()));
        registry.register(Box::new(Context7McpGetLibraryDocs::new()));
        registry
    }

    /// POST /api/mcp disable suppresses ALL of that server's tools: the
    /// LLM-facing list shrinks (an agent run would not see them) and
    /// /api/state reports the server as disabled for the panel.
    #[tokio::test]
    async fn api_mcp_toggle_disable_suppresses_server_tools() {
        let state = test_state_with(mcp_test_registry());
        assert_eq!(state.registry.get_all_for_llm().len(), 5);

        let response = api_mcp_toggle(
            State(state.clone()),
            Json(McpToggleBody {
                server: "tavily".to_string(),
                enabled: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let for_llm: Vec<String> = state
            .registry
            .get_all_for_llm()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(for_llm.len(), 3, "both tavily facade tools disappear");
        assert!(!for_llm.contains(&"tavily_search".to_string()));
        assert!(!for_llm.contains(&"tavily_extract".to_string()));
        assert!(for_llm.contains(&"context7_resolve_library_id".to_string()));
        assert!(for_llm.contains(&"context7_get_library_docs".to_string()));

        // /api/state exposes the per-server toggle state for the panel.
        let snapshot = state_snapshot(&state);
        let by_name = |name: &str| {
            snapshot
                .mcp
                .iter()
                .find(|m| m.name == name)
                .unwrap_or_else(|| panic!("server {name} missing from snapshot"))
        };
        assert!(!by_name("tavily").enabled);
        assert!(by_name("context7").enabled);
    }

    /// POST /api/mcp re-enable restores the server's tools.
    #[tokio::test]
    async fn api_mcp_toggle_reenable_restores_server_tools() {
        let state = test_state_with(mcp_test_registry());
        for enabled in [false, true] {
            let response = api_mcp_toggle(
                State(state.clone()),
                Json(McpToggleBody {
                    server: "tavily".to_string(),
                    enabled,
                }),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
        }

        assert_eq!(state.registry.get_all_for_llm().len(), 5);
        let snapshot = state_snapshot(&state);
        let tavily = snapshot
            .mcp
            .iter()
            .find(|m| m.name == "tavily")
            .expect("tavily in snapshot");
        assert!(tavily.enabled, "re-enabled server reported enabled");
    }

    /// Unknown server → 400 + error body; suppression state untouched.
    #[tokio::test]
    async fn api_mcp_toggle_unknown_server_is_400() {
        let state = test_state_with(mcp_test_registry());
        let response = api_mcp_toggle(
            State(state.clone()),
            Json(McpToggleBody {
                server: "github".to_string(),
                enabled: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(json["ok"], false);
        assert_eq!(json["error"], "unknown mcp server");

        assert_eq!(state.registry.get_all_for_llm().len(), 5);
        assert!(
            state_snapshot(&state).mcp.iter().all(|m| m.enabled),
            "no server was suppressed"
        );
    }

    // --- item54 part 1: opt-in MCP toggle persistence (POST /api/mcp) -------

    /// Flag OFF (default): a POST /api/mcp disable applies session
    /// suppression but does NOT touch settings.jsonc — the browser stays
    /// the durable store.
    #[tokio::test]
    async fn api_mcp_toggle_without_persist_flag_writes_nothing() {
        let state = test_state_with_settings(
            mcp_test_registry(),
            r#"{ "suppressed_tools": ["WebSearch"] }"#,
        );
        let before =
            std::fs::read_to_string(&state.settings_path).expect("seeded settings readable");

        let response = api_mcp_toggle(
            State(state.clone()),
            Json(McpToggleBody {
                server: "tavily".to_string(),
                enabled: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !state.registry.mcp_server_enabled("tavily"),
            "session suppression applied"
        );

        let after = std::fs::read_to_string(&state.settings_path).expect("settings readable");
        assert_eq!(before, after, "flag off → settings untouched");
        let settings = Settings::load_from(&state.settings_path);
        assert!(settings.disabled_mcp_servers.is_empty());
        assert!(!settings.mcp_toggle_persist);
    }

    /// Flag ON: the toggle ALSO persists to settings.jsonc (disable adds,
    /// re-enable removes), the other keys survive, and a fresh registry
    /// seeds the suppression from the persisted list.
    #[tokio::test]
    async fn api_mcp_toggle_with_persist_flag_round_trips() {
        let state = test_state_with_settings(
            mcp_test_registry(),
            r#"{
                // comments stay valid JSONC
                "suppressed_tools": ["WebSearch"],
                "mcp_toggle_persist": true,
                "disabled_skills": ["no-such-builtin"]
            }"#,
        );

        // Disable tavily → persisted.
        let response = api_mcp_toggle(
            State(state.clone()),
            Json(McpToggleBody {
                server: "tavily".to_string(),
                enabled: false,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let settings = Settings::load_from(&state.settings_path);
        assert!(settings.mcp_toggle_persist, "flag survives the rewrite");
        assert_eq!(
            settings.suppressed_tools,
            vec!["WebSearch".to_string()],
            "other keys survive"
        );
        assert_eq!(
            settings.disabled_skills,
            vec!["no-such-builtin".to_string()]
        );
        assert_eq!(
            settings.disabled_mcp_servers,
            vec!["tavily".to_string()],
            "the disabled server is persisted"
        );

        // Re-enable → removed again.
        let response = api_mcp_toggle(
            State(state.clone()),
            Json(McpToggleBody {
                server: "tavily".to_string(),
                enabled: true,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let settings = Settings::load_from(&state.settings_path);
        assert!(
            settings.disabled_mcp_servers.is_empty(),
            "re-enable removes the persisted entry"
        );

        // A fresh registry seeds the suppression from the persisted list —
        // restart semantics with the flag on.
        let registry = mcp_test_registry();
        let settings = Settings {
            disabled_mcp_servers: vec!["tavily".to_string(), "unknown".to_string()],
            ..Settings::load_from(&state.settings_path)
        };
        for server in &settings.disabled_mcp_servers {
            registry.set_mcp_suppressed(server, false);
        }
        assert!(!registry.mcp_server_enabled("tavily"), "seeded disabled");
        assert!(
            registry.mcp_server_enabled("context7"),
            "the other server stays enabled"
        );
        assert_eq!(registry.get_all_for_llm().len(), 3);
        // An unknown persisted server is a no-op (no tools to suppress).
        assert!(registry.mcp_servers().contains(&"tavily".to_string()));
    }

    // --- item54 part 4: fresh session for repeat evals (POST /api/session/reset)

    /// POST /api/session/reset starts a FRESH session in place: a new uuid,
    /// a new rollout whose first line is a session_meta, the agent rebuilt
    /// against the new per-session agent-state dir, and the returned
    /// snapshot reporting the new id. Repeat resets keep minting new ids.
    #[tokio::test]
    async fn api_session_reset_starts_a_fresh_session() {
        let state = test_state();
        let old_id = state.session_id();

        let response = api_session_reset(State(state.clone())).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let new_id = json["session"]["id"].as_str().expect("session id");
        assert_ne!(new_id, old_id, "a fresh session id is minted");
        assert_eq!(json["session"]["id"], state.session_id());
        assert_eq!(
            json["model"], "zai-glm-5-2",
            "provider/model are untouched by a reset"
        );
        assert!(
            axonerai::rollout::is_session_id(new_id),
            "the new id is a lowercase uuid v7"
        );

        // The new rollout exists with a session_meta first line.
        let rollout = axonerai::rollout::Rollout::open(&state.sessions_dir, new_id)
            .expect("the fresh rollout exists");
        let title = rollout.title().expect("a session_meta line is present");
        assert_eq!(
            title, "axonerai",
            "default title = rightmost cwd component (cargo test cwd = crate root)"
        );

        // The agent was rebuilt against the new session (agent-state dir).
        let runtime = state.runtime.read().unwrap();
        let agent = runtime.agent.as_ref().expect("agent rebuilt");
        assert_eq!(
            agent.session_id().expect("session-backed agent"),
            new_id,
            "the agent's session manager points at the fresh session"
        );
        drop(runtime);

        // Repeat resets keep minting fresh sessions.
        let second = api_session_reset(State(state.clone())).await;
        assert_eq!(second.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(second.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_ne!(
            json["session"]["id"].as_str().expect("id"),
            new_id,
            "each reset is a new session"
        );
    }

    /// The router accepts POST /api/session/reset alongside the
    /// parameterized GET /api/session/:uuid (static segments win).
    #[tokio::test]
    async fn router_registers_the_session_reset_route() {
        let state = test_state();
        let _router = build_router(state);
    }
}
