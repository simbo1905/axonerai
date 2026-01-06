//! Web server module for the agent.
//!
//! Provides HTTP and WebSocket endpoints for interacting with the agent,
//! including a chat interface and session management.

use axum::{
    extract::{
        ws::{Message as WsMessage, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post, delete},
    Router,
};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::provider::{Message, Provider};
use crate::session::Session;
use crate::session_storage::{SessionStorage, MemorySessionStorage, NullDelimitedSessionStorage};
use crate::tool::ToolRegistry;

/// Server configuration
#[derive(Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: Option<u16>,
    pub static_dir: PathBuf,
    pub sessions_dir: PathBuf,
    pub use_null_delimited: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: None, // Auto-find a high port
            static_dir: PathBuf::from("./static"),
            sessions_dir: PathBuf::from("./sessions"),
            use_null_delimited: true,
        }
    }
}

/// Shared state for the server
pub struct AppState {
    pub storage: Arc<dyn SessionStorage>,
    pub provider: Arc<RwLock<Option<Box<dyn Provider>>>>,
    pub registry: Arc<RwLock<ToolRegistry>>,
    pub system_prompt: Arc<RwLock<Option<String>>>,
    pub config: ServerConfig,
}

/// WebSocket message types
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsClientMessage {
    #[serde(rename = "chat")]
    Chat { session_id: String, message: String },
    #[serde(rename = "new_session")]
    NewSession,
    #[serde(rename = "load_session")]
    LoadSession { session_id: String },
    #[serde(rename = "list_sessions")]
    ListSessions,
    #[serde(rename = "delete_session")]
    DeleteSession { session_id: String },
    #[serde(rename = "ping")]
    Ping,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum WsServerMessage {
    #[serde(rename = "chat_response")]
    ChatResponse { session_id: String, message: String },
    #[serde(rename = "chat_chunk")]
    ChatChunk { session_id: String, chunk: String },
    #[serde(rename = "chat_complete")]
    ChatComplete { session_id: String },
    #[serde(rename = "thinking")]
    Thinking { session_id: String, message: String },
    #[serde(rename = "tool_use")]
    ToolUse { session_id: String, tool: String, input: String },
    #[serde(rename = "tool_result")]
    ToolResult { session_id: String, tool: String, result: String },
    #[serde(rename = "session_created")]
    SessionCreated { session_id: String },
    #[serde(rename = "session_loaded")]
    SessionLoaded { session_id: String, messages: Vec<MessageDto> },
    #[serde(rename = "sessions_list")]
    SessionsList { sessions: Vec<String> },
    #[serde(rename = "session_deleted")]
    SessionDeleted { session_id: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "pong")]
    Pong,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct MessageDto {
    pub role: String,
    pub content: String,
}

impl From<&Message> for MessageDto {
    fn from(msg: &Message) -> Self {
        Self {
            role: msg.role.clone(),
            content: msg.content.clone(),
        }
    }
}

/// REST API responses
#[derive(Serialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    pub endpoints: Vec<EndpointInfo>,
}

#[derive(Serialize)]
pub struct EndpointInfo {
    pub method: String,
    pub path: String,
    pub description: String,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub id: String,
    pub message_count: usize,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct ChatRequest {
    pub message: String,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub response: String,
    pub session_id: String,
}

/// Find an available high port
pub async fn find_available_port(host: &str, preferred: Option<u16>) -> std::io::Result<u16> {
    use tokio::net::TcpListener;
    
    if let Some(port) = preferred {
        // Try the preferred port first
        if TcpListener::bind(format!("{}:{}", host, port)).await.is_ok() {
            return Ok(port);
        }
    }
    
    // Try ports in the high range (49152-65535)
    for port in 49152..65535 {
        if TcpListener::bind(format!("{}:{}", host, port)).await.is_ok() {
            return Ok(port);
        }
    }
    
    Err(std::io::Error::new(
        std::io::ErrorKind::AddrNotAvailable,
        "No available ports found",
    ))
}

/// Create the server router
pub fn create_router(state: Arc<AppState>) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);
    
    // Static file service
    let static_service = ServeDir::new(&state.config.static_dir);
    
    Router::new()
        // API endpoints
        .route("/", get(root_handler))
        .route("/api/info", get(info_handler))
        .route("/api/sessions", get(list_sessions_handler))
        .route("/api/sessions", post(create_session_handler))
        .route("/api/sessions/{session_id}", get(get_session_handler))
        .route("/api/sessions/{session_id}", delete(delete_session_handler))
        .route("/api/sessions/{session_id}/chat", post(chat_handler))
        // WebSocket endpoint
        .route("/ws", get(ws_handler))
        // Static files (fallback)
        .fallback_service(static_service)
        .layer(cors)
        .with_state(state)
}

/// Root handler - serves index.html
async fn root_handler(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let index_path = state.config.static_dir.join("index.html");
    
    if index_path.exists() {
        match tokio::fs::read_to_string(&index_path).await {
            Ok(content) => Html(content).into_response(),
            Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "Failed to read index.html").into_response(),
        }
    } else {
        // Return embedded fallback HTML
        Html(FALLBACK_INDEX_HTML).into_response()
    }
}

/// Server info endpoint
async fn info_handler() -> Json<ServerInfo> {
    Json(ServerInfo {
        name: "AxonerAI Agent Server".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        endpoints: vec![
            EndpointInfo {
                method: "GET".to_string(),
                path: "/".to_string(),
                description: "Serve the chat UI".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/ws".to_string(),
                description: "WebSocket endpoint for real-time chat".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/info".to_string(),
                description: "Server information".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/sessions".to_string(),
                description: "List all sessions".to_string(),
            },
            EndpointInfo {
                method: "POST".to_string(),
                path: "/api/sessions".to_string(),
                description: "Create a new session".to_string(),
            },
            EndpointInfo {
                method: "GET".to_string(),
                path: "/api/sessions/{id}".to_string(),
                description: "Get a session by ID".to_string(),
            },
            EndpointInfo {
                method: "DELETE".to_string(),
                path: "/api/sessions/{id}".to_string(),
                description: "Delete a session".to_string(),
            },
            EndpointInfo {
                method: "POST".to_string(),
                path: "/api/sessions/{id}/chat".to_string(),
                description: "Send a message to the agent".to_string(),
            },
        ],
    })
}

/// List sessions handler
async fn list_sessions_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<String>>, (StatusCode, String)> {
    state.storage.list_sessions().await
        .map(Json)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Create session handler
async fn create_session_handler(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SessionInfo>, (StatusCode, String)> {
    let session_id = uuid::Uuid::new_v4().to_string();
    let session = Session::new(session_id.clone());
    
    state.storage.save(&session).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(Json(SessionInfo {
        id: session_id,
        message_count: 0,
        created_at: session.get_timestamp().to_string(),
    }))
}

/// Get session handler
async fn get_session_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionInfo>, (StatusCode, String)> {
    let session = state.storage.load(&session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "Session not found".to_string()))?;
    
    Ok(Json(SessionInfo {
        id: session.get_id().to_string(),
        message_count: session.get_messages().len(),
        created_at: session.get_timestamp().to_string(),
    }))
}

/// Delete session handler
async fn delete_session_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, (StatusCode, String)> {
    state.storage.delete(&session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(StatusCode::NO_CONTENT)
}

/// Chat handler (REST API)
async fn chat_handler(
    State(state): State<Arc<AppState>>,
    Path(session_id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Result<Json<ChatResponse>, (StatusCode, String)> {
    // Load or create session
    let mut session = state.storage.load(&session_id).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .unwrap_or_else(|| Session::new(session_id.clone()));
    
    // Add user message
    session.add_message(Message {
        role: "user".to_string(),
        content: req.message.clone(),
    });
    
    // Get provider and run agent
    let provider_guard = state.provider.read().await;
    let provider = provider_guard.as_ref()
        .ok_or((StatusCode::SERVICE_UNAVAILABLE, "No provider configured".to_string()))?;
    
    let registry_guard = state.registry.read().await;
    let system_prompt_guard = state.system_prompt.read().await;
    
    // Create a temporary agent for this request
    let response = run_agent_step(
        provider.as_ref(),
        &registry_guard,
        system_prompt_guard.clone(),
        session.get_messages().clone(),
    ).await.map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    // Add assistant response
    session.add_message(Message {
        role: "assistant".to_string(),
        content: response.clone(),
    });
    
    // Save session
    state.storage.save(&session).await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    
    Ok(Json(ChatResponse {
        response,
        session_id,
    }))
}

/// Run a single agent step
async fn run_agent_step(
    provider: &dyn Provider,
    registry: &ToolRegistry,
    system_prompt: Option<String>,
    messages: Vec<Message>,
) -> anyhow::Result<String> {
    use crate::executor::ToolExecutor;
    
    let tools = registry.get_all_for_llm();
    let executor = ToolExecutor::new(registry);
    
    let mut current_messages = messages;
    let max_iterations = 10;
    
    for _ in 0..max_iterations {
        let response = provider
            .complete(current_messages.clone(), Some(tools.clone()), None, system_prompt.clone())
            .await?;
        
        match response.stop_reason {
            crate::provider::StopReason::EndTurn => {
                return Ok(response.text.unwrap_or_else(|| "(No response)".to_string()));
            }
            crate::provider::StopReason::ToolUse => {
                if response.tool_calls.is_empty() {
                    return Ok("Agent wanted to use tools but didn't specify any".to_string());
                }
                
                // Execute tools
                let tool_results = executor.execute_all(&response.tool_calls).await?;
                
                // Add tool use to messages
                if let Some(text) = &response.text {
                    current_messages.push(Message {
                        role: "assistant".to_string(),
                        content: text.clone(),
                    });
                }
                
                current_messages.push(Message {
                    role: "assistant".to_string(),
                    content: format_tool_use(&response.tool_calls),
                });
                
                current_messages.push(Message {
                    role: "user".to_string(),
                    content: format_tool_results(&tool_results),
                });
            }
            crate::provider::StopReason::MaxTokens => {
                return Ok("Agent hit max tokens limit".to_string());
            }
            _ => {
                return Ok(format!("Agent stopped: {:?}", response.stop_reason));
            }
        }
    }
    
    Ok("Agent reached max iterations".to_string())
}

fn format_tool_use(tool_calls: &[crate::provider::ToolCall]) -> String {
    tool_calls
        .iter()
        .map(|call| format!("Using tool '{}' with input: {}", call.name, serde_json::to_string(&call.input).unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_tool_results(results: &[crate::executor::ToolResult]) -> String {
    results
        .iter()
        .map(|r| format!("Tool '{}' returned: {}", r.tool_name, r.result))
        .collect::<Vec<_>>()
        .join("\n")
}

/// WebSocket handler
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_websocket(socket, state))
}

/// Handle WebSocket connection
async fn handle_websocket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();
    
    while let Some(result) = receiver.next().await {
        let msg = match result {
            Ok(WsMessage::Text(text)) => text,
            Ok(WsMessage::Close(_)) => break,
            Ok(_) => continue,
            Err(_) => break,
        };
        
        let client_msg: WsClientMessage = match serde_json::from_str(&msg) {
            Ok(m) => m,
            Err(e) => {
                let error_response = WsServerMessage::Error {
                    message: format!("Invalid message format: {}", e),
                };
                let _ = sender.send(WsMessage::Text(serde_json::to_string(&error_response).unwrap().into())).await;
                continue;
            }
        };
        
        let response = handle_ws_message(client_msg, &state).await;
        
        if let Err(e) = sender.send(WsMessage::Text(serde_json::to_string(&response).unwrap().into())).await {
            eprintln!("WebSocket send error: {}", e);
            break;
        }
    }
}

/// Handle individual WebSocket messages
async fn handle_ws_message(msg: WsClientMessage, state: &Arc<AppState>) -> WsServerMessage {
    match msg {
        WsClientMessage::Ping => WsServerMessage::Pong,
        
        WsClientMessage::NewSession => {
            let session_id = uuid::Uuid::new_v4().to_string();
            let session = Session::new(session_id.clone());
            
            if let Err(e) = state.storage.save(&session).await {
                return WsServerMessage::Error { message: e.to_string() };
            }
            
            WsServerMessage::SessionCreated { session_id }
        }
        
        WsClientMessage::LoadSession { session_id } => {
            match state.storage.load(&session_id).await {
                Ok(Some(session)) => {
                    let messages: Vec<MessageDto> = session.get_messages().iter().map(MessageDto::from).collect();
                    WsServerMessage::SessionLoaded { session_id, messages }
                }
                Ok(None) => WsServerMessage::Error { message: "Session not found".to_string() },
                Err(e) => WsServerMessage::Error { message: e.to_string() },
            }
        }
        
        WsClientMessage::ListSessions => {
            match state.storage.list_sessions().await {
                Ok(sessions) => WsServerMessage::SessionsList { sessions },
                Err(e) => WsServerMessage::Error { message: e.to_string() },
            }
        }
        
        WsClientMessage::DeleteSession { session_id } => {
            match state.storage.delete(&session_id).await {
                Ok(_) => WsServerMessage::SessionDeleted { session_id },
                Err(e) => WsServerMessage::Error { message: e.to_string() },
            }
        }
        
        WsClientMessage::Chat { session_id, message } => {
            // Load or create session
            let mut session = match state.storage.load(&session_id).await {
                Ok(Some(s)) => s,
                Ok(None) => Session::new(session_id.clone()),
                Err(e) => return WsServerMessage::Error { message: e.to_string() },
            };
            
            // Add user message
            session.add_message(Message {
                role: "user".to_string(),
                content: message,
            });
            
            // Run agent
            let provider_guard = state.provider.read().await;
            let provider = match provider_guard.as_ref() {
                Some(p) => p,
                None => return WsServerMessage::Error { message: "No provider configured".to_string() },
            };
            
            let registry_guard = state.registry.read().await;
            let system_prompt_guard = state.system_prompt.read().await;
            
            let response = match run_agent_step(
                provider.as_ref(),
                &registry_guard,
                system_prompt_guard.clone(),
                session.get_messages().clone(),
            ).await {
                Ok(r) => r,
                Err(e) => return WsServerMessage::Error { message: e.to_string() },
            };
            
            // Add assistant response
            session.add_message(Message {
                role: "assistant".to_string(),
                content: response.clone(),
            });
            
            // Save session
            if let Err(e) = state.storage.save(&session).await {
                return WsServerMessage::Error { message: e.to_string() };
            }
            
            WsServerMessage::ChatResponse {
                session_id,
                message: response,
            }
        }
    }
}

/// Start the server
pub async fn start_server(
    config: ServerConfig,
    provider: Option<Box<dyn Provider>>,
    registry: ToolRegistry,
    system_prompt: Option<String>,
) -> anyhow::Result<()> {
    // Create storage
    let storage: Arc<dyn SessionStorage> = if config.use_null_delimited {
        Arc::new(NullDelimitedSessionStorage::new(config.sessions_dir.clone())?)
    } else {
        Arc::new(MemorySessionStorage::new())
    };
    
    // Find available port
    let port = find_available_port(&config.host, config.port).await?;
    let addr: SocketAddr = format!("{}:{}", config.host, port).parse()?;
    
    // Create state
    let state = Arc::new(AppState {
        storage,
        provider: Arc::new(RwLock::new(provider)),
        registry: Arc::new(RwLock::new(registry)),
        system_prompt: Arc::new(RwLock::new(system_prompt)),
        config: config.clone(),
    });
    
    // Create router
    let router = create_router(state);
    
    // Print server info
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║           🤖 AxonerAI Agent Server Started                   ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║                                                              ║");
    println!("║  🌐 Web UI:      http://{}:{:<24}  ║", config.host, port);
    println!("║  🔌 WebSocket:   ws://{}:{}/ws{:<19}  ║", config.host, port, "");
    println!("║  📡 API:         http://{}:{}/api/info{:<13}  ║", config.host, port, "");
    println!("║                                                              ║");
    println!("║  📁 Static dir:  {:<43} ║", config.static_dir.display());
    println!("║  💾 Sessions:    {:<43} ║", config.sessions_dir.display());
    println!("║                                                              ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!();
    
    // Start server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;
    
    Ok(())
}

/// Fallback HTML when no static files are present
const FALLBACK_INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>AxonerAI Agent</title>
    <style>
        body {
            font-family: system-ui, -apple-system, sans-serif;
            background: #111827;
            color: #e5e7eb;
            display: flex;
            align-items: center;
            justify-content: center;
            height: 100vh;
            margin: 0;
        }
        .container {
            text-align: center;
            padding: 2rem;
        }
        h1 { color: #60a5fa; }
        p { color: #9ca3af; }
        code {
            background: #374151;
            padding: 0.25rem 0.5rem;
            border-radius: 0.25rem;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🤖 AxonerAI Agent Server</h1>
        <p>Server is running! Place your <code>index.html</code> in the static directory.</p>
        <p>API available at <code>/api/info</code></p>
        <p>WebSocket available at <code>/ws</code></p>
    </div>
</body>
</html>"#;
