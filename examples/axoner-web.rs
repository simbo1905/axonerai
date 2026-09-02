use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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
use axonerai::rollout::{self, Rollout, SessionInfo};
use axonerai::settings::Settings;
use axonerai::tool::{ToolInfo, ToolRegistry};
use axonerai::tools::{
    Calculator, TavilyMcpExtract, TavilyMcpSearch, WebFetch, WebSearch, WriteFile,
};
use axonerai::wire::{ClientMsg, RolloutRecord, ServerMsg};
use axonerai::{Agent, AppConfig, GroqProvider, MistralProvider, OpenAIProvider, OpenCodeProvider};

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

#[derive(Clone)]
struct AppState {
    web_root: PathBuf,
    agent: Option<Arc<Agent>>,
    verbose: u8,
    rollout: Arc<Rollout>,
    session_id: String,
    provider: String,
    model: String,
    /// Registry clone sharing tool instances and suppression state with the
    /// agent's registry (both fields are Arc-backed in `ToolRegistry`).
    registry: ToolRegistry,
    /// Running total of rollout bytes appended this run; context tokens are
    /// bytes/4 (floor). Bumped at every rollout append site.
    context_bytes: Arc<AtomicU64>,
}

impl AppState {
    fn bump_context_bytes(&self, n: u64) {
        if n > 0 {
            self.context_bytes.fetch_add(n, Ordering::Relaxed);
        }
    }

    /// Durable-first append with a context-byte counter bump: the counter
    /// tracks every byte written to the rollout (payload + newline).
    fn append_event(&self, value: &serde_json::Value) {
        self.bump_context_bytes(
            serde_json::to_string(value)
                .map(|s| s.len() as u64 + 1)
                .unwrap_or(0),
        );
        let _ = self.rollout.append_event(value);
    }

    /// Raw-JSON variant of [`AppState::append_event`] (payload-last field
    /// order preserved on disk).
    fn append_json(&self, json: &str) {
        self.bump_context_bytes(json.len() as u64 + 1);
        let _ = self.rollout.append_json(json);
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
            Ok((Arc::new(rollout), id))
        }
    }
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
    let agent = build_agent_from_config(&config, &provider_name, &model_id, registry.clone()).ok();

    let sessions_dir = rollout::default_dir();
    let (session_rollout, session_id) = resolve_session(&sessions_dir, continue_, session)?;

    // Seed the context counter with the rollout size so bytes appended
    // before AppState existed (session creation + meta lines) are counted.
    let rollout_bytes_so_far = std::fs::metadata(session_rollout.path())
        .map(|m| m.len())
        .unwrap_or(0);

    let state = AppState {
        web_root,
        agent,
        verbose,
        rollout: session_rollout,
        session_id: session_id.clone(),
        provider: provider_name.clone(),
        model: model_id.clone(),
        registry,
        context_bytes: Arc::new(AtomicU64::new(rollout_bytes_so_far)),
    };

    let assets_dir = state.web_root.join("assets");
    let assets_service = tower_http::services::ServeDir::new(assets_dir);
    let src_service = tower_http::services::ServeDir::new(state.web_root.join("src"));
    let test_service = tower_http::services::ServeDir::new(state.web_root.join("test"));
    let generated_service = tower_http::services::ServeDir::new(state.web_root.join("generated"));

    let app = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/ws", get(ws_upgrade))
        .route("/api/sessions", get(api_sessions))
        .route("/api/session/:uuid", get(api_session_catchup))
        .route("/api/session/:uuid/tail", get(api_session_tail))
        .route("/api/state", get(api_state))
        .route("/api/tools", post(api_tools_toggle))
        .route("/openapi.yaml", get(openapi_yaml))
        .nest_service("/assets", assets_service)
        .nest_service("/src", src_service)
        .nest_service("/test", test_service)
        .nest_service("/generated", generated_service)
        .fallback(get(index))
        .with_state(state.clone());

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
    if state.agent.is_none() {
        println!(
            "  Note: no provider configured (set MISTRAL_API_KEY / OPENCODE_API_KEY / GROQ_API_KEY)"
        );
    }
    println!();
    println!("  Provider:   {}", provider_name);
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

async fn index(State(state): State<AppState>) -> Response {
    let disk_path = state.web_root.join("index.html");

    match tokio::fs::read_to_string(&disk_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html(include_str!("../web/index.html").to_string()).into_response(),
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
}

#[derive(serde::Serialize)]
struct McpServerInfo {
    name: String,
    status: String,
}

/// The control-plane snapshot the UI panel renders. Field order matches the
/// pinned API contract.
#[derive(serde::Serialize)]
struct StateSnapshot {
    provider: String,
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
    if let Some(dir) = state.rollout.path().parent() {
        if let Ok(sessions) = rollout::sessions_index(dir) {
            if let Some(info) = sessions.iter().find(|s| s.uuid == state.session_id) {
                return info.title.clone();
            }
        }
    }
    state
        .rollout
        .title()
        .unwrap_or_else(|_| state.session_id.clone())
}

/// GET /api/state — provider/model/session/repo/context/tools/mcp snapshot.
/// Context tokens are the running rollout-bytes counter divided by 4 (floor);
/// it is NOT a rescan of the rollout.
async fn api_state(State(state): State<AppState>) -> Response {
    // Title resolution must match the sessions index: the last `session_rename`
    // wins over the first `session_meta` (Rollout::title() only sees the meta).
    let title = session_title(&state);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let snapshot = StateSnapshot {
        provider: state.provider.clone(),
        model: state.model.clone(),
        session: SessionSnapshot {
            id: state.session_id.clone(),
            title,
        },
        repo: RepoSnapshot {
            path: cwd.display().to_string(),
            branch: current_branch(&cwd),
        },
        context: ContextSnapshot {
            tokens: state.context_bytes.load(Ordering::Relaxed) / 4,
        },
        tools: state.registry.list_tools_info(),
        mcp: mcp_servers(),
        lsp: vec![],
        todo: serde_json::Value::Null,
    };
    Json(snapshot).into_response()
}

/// Registered (fake) MCP servers: `tavily` is "connected" iff its API key is
/// present; there is no MCP host process.
fn mcp_servers() -> Vec<McpServerInfo> {
    if std::env::var("TAVILY_API_KEY").is_ok() {
        vec![McpServerInfo {
            name: "tavily".to_string(),
            status: "connected".to_string(),
        }]
    } else {
        vec![]
    }
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

    let settings = Settings {
        suppressed_tools: state.registry.suppressed_names(),
    };
    if let Err(e) = settings.save() {
        warn!("failed to persist settings: {e}");
    }

    Json(serde_json::json!({"ok": true})).into_response()
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
    info!("[{}] ws client connected", state.session_id);

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
    let meta_title = state
        .rollout
        .title()
        .unwrap_or_else(|_| state.session_id.clone());
    let meta_value = serde_json::to_value(&ServerMsg::SessionMeta {
        session_id: &state.session_id,
        title: &meta_title,
        created_at: now_ms(),
    })
    .unwrap_or_else(|_| {
        serde_json::json!({
            "_type": "session_meta",
            "session_id": state.session_id,
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
                            let Some(agent) = &state.agent else {
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
                                        session_id: &fwd_state.session_id,
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
    registry.register(Box::new(WriteFile));

    // Only register the Tavily-backed web tools when an API key is available.
    if std::env::var("TAVILY_API_KEY").is_ok() {
        registry.register(Box::new(WebSearch::new()));
        registry.register(Box::new(WebFetch::new()));
        // Fake Tavily MCP facade: MCP-style tools with no MCP host process.
        registry.register(Box::new(TavilyMcpSearch::new()));
        registry.register(Box::new(TavilyMcpExtract::new()));
    }

    // Seed per-tool suppression from the persisted settings so a restart
    // restores the previous on/off state.
    let settings = Settings::load();
    for name in &settings.suppressed_tools {
        registry.set_suppressed(name, false);
    }

    registry
}

fn build_agent_from_config(
    config: &AppConfig,
    provider_name: &str,
    model_id: &str,
    registry: ToolRegistry,
) -> anyhow::Result<Arc<Agent>> {
    let api_key = config.resolve_api_key(provider_name)?;
    let endpoint = config.endpoint(provider_name)?;

    let provider: Box<dyn axonerai::provider::Provider> = match provider_name {
        "mistral" => {
            let mut p = MistralProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        "groq" => {
            let mut p = GroqProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        "openai" => {
            let mut p = OpenAIProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        // opencode-zen, opencode-go, or any other OpenAI-compatible provider
        _ => {
            let model = if model_id.is_empty() {
                config.default_model_id(provider_name)?.to_string()
            } else {
                model_id.to_string()
            };
            Box::new(OpenCodeProvider::new(api_key, endpoint.to_string(), model))
        }
    };

    let system_prompt = Some(axonerai::prompt::load_system_prompt(
        &provider_name,
        &model_id,
    ));

    Ok(Arc::new(Agent::new(
        provider,
        registry,
        system_prompt,
        None,
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
}
