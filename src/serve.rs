use anyhow::Result;
use axum::{
    extract::{ws::WebSocket, State, WebSocketUpgrade},
    response::{Html, Response},
    routing::{get, get_service},
    Router,
};
use futures::{SinkExt, StreamExt};
use std::net::{TcpListener, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

use crate::agent::Agent;

/// Shared application state
pub struct AppState {
    pub agent: Arc<Agent>,
}

/// Start the HTTP server with WebSocket support
pub async fn start_server(agent: Agent, preferred_port: u16) -> Result<()> {
    let state = Arc::new(AppState {
        agent: Arc::new(agent),
    });

    // Try to find an available port
    let port = find_available_port(preferred_port)?;
    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    // Configure CORS (permissive for development - restrict in production)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Determine the static files path
    let static_dir = PathBuf::from("static");

    // Build the application router
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/index.html", get(serve_index))
        .route("/ws", get(websocket_handler))
        .nest_service("/assets", get_service(ServeDir::new(static_dir.join("assets"))))
        .layer(cors)
        .with_state(state);

    // Print the server URL
    println!("agt server listening on http://127.0.0.1:{}", port);

    // Start the server
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Find an available port, trying the preferred port first, then falling back to port 0
fn find_available_port(preferred_port: u16) -> Result<u16> {
    // Try the preferred port first
    if let Ok(listener) = TcpListener::bind(("127.0.0.1", preferred_port)) {
        return Ok(listener.local_addr()?.port());
    }

    // Fallback to any available port
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    Ok(listener.local_addr()?.port())
}

/// Serve the index.html file
async fn serve_index() -> Html<String> {
    let html = include_str!("../static/index.html");
    Html(html.to_string())
}

/// WebSocket handler
async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> Response {
    ws.on_upgrade(|socket| handle_websocket(socket, state))
}

/// Handle WebSocket connection
async fn handle_websocket(socket: WebSocket, state: Arc<AppState>) {
    let (mut sender, mut receiver) = socket.split();

    // Send welcome message
    let welcome = serde_json::json!({
        "type": "system",
        "content": "Connected to AxonerAI agent"
    });
    if let Ok(msg) = serde_json::to_string(&welcome) {
        let _ = sender.send(axum::extract::ws::Message::Text(msg)).await;
    }

    // Handle incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            axum::extract::ws::Message::Text(text) => {
                // Parse the incoming message
                let user_message = match serde_json::from_str::<serde_json::Value>(&text) {
                    Ok(json) => json["content"].as_str().unwrap_or(&text).to_string(),
                    Err(_) => text,
                };

                // Echo user message back
                let user_echo = serde_json::json!({
                    "type": "user",
                    "content": user_message
                });
                if let Ok(msg_str) = serde_json::to_string(&user_echo) {
                    let _ = sender.send(axum::extract::ws::Message::Text(msg_str)).await;
                }

                // Process with agent
                match state.agent.run(&user_message).await {
                    Ok(response) => {
                        let response_msg = serde_json::json!({
                            "type": "assistant",
                            "content": response
                        });
                        if let Ok(msg_str) = serde_json::to_string(&response_msg) {
                            let _ = sender.send(axum::extract::ws::Message::Text(msg_str)).await;
                        }
                    }
                    Err(e) => {
                        let error_msg = serde_json::json!({
                            "type": "error",
                            "content": format!("Error: {}", e)
                        });
                        if let Ok(msg_str) = serde_json::to_string(&error_msg) {
                            let _ = sender.send(axum::extract::ws::Message::Text(msg_str)).await;
                        }
                    }
                }
            }
            axum::extract::ws::Message::Close(_) => {
                break;
            }
            _ => {}
        }
    }
}
