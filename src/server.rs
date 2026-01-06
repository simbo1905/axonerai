use crate::agent::Agent;
use crate::file_session_manager::FileSessionManager;
use crate::null_delimited_file_session_manager::NullDelimitedFileSessionManager;
use crate::provider::Provider;
use crate::session_manager::SessionManager;
use crate::tool::ToolRegistry;
use anyhow::{anyhow, Result};
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use axum::response::{Html, IntoResponse};
use axum::routing::get;
use axum::Router;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tracing::info;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub enum SessionMode {
    Stateless,
    JsonFile,
    NullDelimitedFile,
}

#[derive(Clone)]
pub struct ServeConfig {
    pub host: String,
    pub port: u16, // 0 => pick
    pub web_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub session_mode: SessionMode,
    pub system_prompt: Option<String>,
}

#[derive(Clone)]
struct AppState {
    provider: Arc<dyn Provider>,
    registry: Arc<ToolRegistry>,
    cfg: ServeConfig,
    // serialize runs per-connection (Agent is not explicitly async-safe re: session manager)
    run_lock: Arc<Mutex<()>>,
    tui_prompt: Arc<Mutex<String>>,
    tui_queue: Arc<Mutex<VecDeque<serde_json::Value>>>,
}

pub async fn serve(provider: Arc<dyn Provider>, registry: ToolRegistry, cfg: ServeConfig) -> Result<()> {
    let web_dir = cfg.web_dir.clone();
    if !web_dir.exists() {
        return Err(anyhow!("web_dir does not exist: {}", web_dir.display()));
    }
    let assets_dir = web_dir.join("assets");

    let state = AppState {
        provider,
        registry: Arc::new(registry),
        cfg: cfg.clone(),
        run_lock: Arc::new(Mutex::new(())),
        tui_prompt: Arc::new(Mutex::new(String::new())),
        tui_queue: Arc::new(Mutex::new(VecDeque::new())),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_headers(Any)
        .allow_methods(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/ws", get(ws_upgrade))
        // Minimal opencode-ish TUI control surface (enough for a SPA / simple client)
        .route("/tui/append-prompt", axum::routing::post(tui_append_prompt))
        .route("/tui/clear-prompt", axum::routing::post(tui_clear_prompt))
        .route("/tui/submit-prompt", axum::routing::post(tui_submit_prompt))
        .route("/tui/open-help", axum::routing::post(tui_ok))
        .route("/tui/open-sessions", axum::routing::post(tui_ok))
        .route("/tui/open-themes", axum::routing::post(tui_ok))
        .route("/tui/open-models", axum::routing::post(tui_ok))
        .route("/tui/execute-command", axum::routing::post(tui_ok))
        .route("/tui/show-toast", axum::routing::post(tui_ok))
        .route("/tui/publish", axum::routing::post(tui_ok))
        .route("/tui/control/next", axum::routing::get(tui_control_next))
        .route("/tui/control/response", axum::routing::post(tui_control_response))
        // Stub PTY endpoints (not implemented in this crate yet)
        .route("/pty", axum::routing::get(pty_list).post(pty_not_implemented))
        .route("/pty/{ptyID}", axum::routing::get(pty_not_found).put(pty_not_implemented).delete(pty_not_found))
        .route("/pty/{ptyID}/connect", axum::routing::get(pty_not_implemented))
        .nest_service("/assets", ServeDir::new(assets_dir))
        .layer(cors)
        .with_state(state);

    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    let local_addr = listener.local_addr()?;

    info!("Listening on http://{}", local_addr);
    println!("agt serve");
    println!("  web:  http://{}/", local_addr);
    println!("  ws:   ws://{}/ws", local_addr);
    println!("  dir:  {}", cfg.sessions_dir.display());
    println!("  mode: {:?}", cfg.session_mode);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn index(State(state): State<AppState>) -> Result<impl IntoResponse, (StatusCode, String)> {
    let p = state.cfg.web_dir.join("index.html");
    read_file_to_response(&p).await
}

async fn read_file_to_response(path: &Path) -> Result<impl IntoResponse, (StatusCode, String)> {
    match tokio::fs::read(path).await {
        Ok(bytes) => Ok((HeaderMap::new(), Html(String::from_utf8_lossy(&bytes).to_string()))),
        Err(e) => Err((StatusCode::NOT_FOUND, format!("{}: {}", path.display(), e))),
    }
}

async fn ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_handle(socket, state))
}

async fn ws_handle(mut socket: WebSocket, state: AppState) {
    let session_id = Uuid::new_v4().to_string();

    let _ = socket
        .send(WsMessage::Text(
            serde_json::json!({
                "type": "server.connected",
                "sessionID": session_id,
            })
            .to_string()
            .into(),
        ))
        .await;

    while let Some(Ok(msg)) = socket.recv().await {
        match msg {
            WsMessage::Text(t) => {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) else {
                    let _ = socket
                        .send(WsMessage::Text(
                            serde_json::json!({"type":"error","message":"invalid json"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    continue;
                };
                let msg_type = v.get("type").and_then(|x| x.as_str()).unwrap_or("");
                if msg_type != "chat" {
                    let _ = socket
                        .send(WsMessage::Text(
                            serde_json::json!({"type":"error","message":"unknown message type"})
                                .to_string()
                                .into(),
                        ))
                        .await;
                    continue;
                }
                let text = v.get("text").and_then(|x| x.as_str()).unwrap_or("").trim();
                if text.is_empty() {
                    continue;
                }

                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::json!({"type":"chat.started"})
                            .to_string()
                            .into(),
                    ))
                    .await;

                let reply = run_agent_once(&state, &session_id, text).await;
                match reply {
                    Ok(out) => {
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::json!({"type":"message","role":"assistant","content":out})
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::json!({"type":"chat.finished"})
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                    }
                    Err(e) => {
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::json!({"type":"error","message":e.to_string()})
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::json!({"type":"chat.finished"})
                                    .to_string()
                                    .into(),
                            ))
                            .await;
                    }
                }
            }
            WsMessage::Binary(_b) => {
                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::json!({"type":"error","message":"binary messages not supported"})
                            .to_string()
                            .into(),
                    ))
                    .await;
            }
            WsMessage::Close(_) => break,
            _ => {}
        }
    }
}

async fn run_agent_once(state: &AppState, session_id: &str, prompt: &str) -> Result<String> {
    // Protect the (file) session manager from concurrent rewrites.
    let _guard = state.run_lock.lock().await;

    let sm: Option<Arc<dyn SessionManager>> = match state.cfg.session_mode {
        SessionMode::Stateless => None,
        SessionMode::JsonFile => {
            let sm = FileSessionManager::new(session_id.to_string(), state.cfg.sessions_dir.clone())?;
            Some(Arc::new(sm))
        }
        SessionMode::NullDelimitedFile => {
            let sm = NullDelimitedFileSessionManager::new(
                session_id.to_string(),
                state.cfg.sessions_dir.clone(),
            )?;
            Some(Arc::new(sm))
        }
    };

    let agent = Agent::new_with_session_manager(
        state.provider.clone(),
        state.registry.clone(),
        state.cfg.system_prompt.clone(),
        sm,
    );

    agent.run(prompt).await
}

// --------------------------
// TUI endpoints (opencode-ish)
// --------------------------

#[derive(Debug, Deserialize)]
struct AppendPromptBody {
    text: String,
}

async fn tui_append_prompt(
    State(state): State<AppState>,
    Json(body): Json<AppendPromptBody>,
) -> impl IntoResponse {
    let mut p = state.tui_prompt.lock().await;
    p.push_str(&body.text);
    Json(true)
}

async fn tui_clear_prompt(State(state): State<AppState>) -> impl IntoResponse {
    let mut p = state.tui_prompt.lock().await;
    p.clear();
    Json(true)
}

async fn tui_submit_prompt(State(state): State<AppState>) -> impl IntoResponse {
    let prompt = {
        let mut p = state.tui_prompt.lock().await;
        let out = p.trim().to_string();
        p.clear();
        out
    };

    if prompt.is_empty() {
        return Json(true);
    }

    // Push a "user message" event (simple shape for consumers).
    {
        let mut q = state.tui_queue.lock().await;
        q.push_back(serde_json::json!({
            "path": "/event",
            "body": { "type": "message", "role": "user", "content": prompt },
        }));
    }

    // Run agent and push assistant reply.
    let reply = run_agent_once(&state, "tui", &prompt).await;
    match reply {
        Ok(content) => {
            let mut q = state.tui_queue.lock().await;
            q.push_back(serde_json::json!({
                "path": "/event",
                "body": { "type": "message", "role": "assistant", "content": content },
            }));
            Json(true)
        }
        Err(e) => {
            let mut q = state.tui_queue.lock().await;
            q.push_back(serde_json::json!({
                "path": "/event",
                "body": { "type": "error", "message": e.to_string() },
            }));
            Json(true)
        }
    }
}

async fn tui_control_next(State(state): State<AppState>) -> impl IntoResponse {
    let mut q = state.tui_queue.lock().await;
    if let Some(v) = q.pop_front() {
        return Json(v);
    }
    Json(serde_json::json!({"path":"","body":null}))
}

async fn tui_control_response(Json(_body): Json<serde_json::Value>) -> impl IntoResponse {
    // This server doesn't track request IDs yet; this is an ACK hook for compatibility.
    Json(true)
}

async fn tui_ok() -> impl IntoResponse {
    Json(true)
}

// --------------------------
// PTY stubs (compatibility)
// --------------------------

#[derive(Debug, Serialize)]
struct Pty {
    id: String,
    title: String,
    command: String,
    args: Vec<String>,
    cwd: String,
    status: String,
    pid: i64,
}

async fn pty_list() -> impl IntoResponse {
    Json(Vec::<Pty>::new())
}

async fn pty_not_found() -> impl IntoResponse {
    (StatusCode::NOT_FOUND, Json(serde_json::json!({"name":"NotFoundError","data":{"message":"not found"}})))
}

async fn pty_not_implemented() -> impl IntoResponse {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"name":"NotImplemented","data":{"message":"PTY is not implemented in agt serve yet"}})),
    )
}

