use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::{Parser, Subcommand};

use axonerai::{Agent, AnthropicProvider, GroqProvider, OpenAIProvider, ToolRegistry};
use axonerai::tools::{Calculator, WebScrape, WebSearch};

#[derive(Parser, Debug)]
#[command(name = "agt", version, about = "AxonerAI tooling")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start a local web UI + websocket server
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
    },
}

#[derive(Clone)]
struct AppState {
    web_root: PathBuf,
    agent: Option<Arc<Agent>>,
}

#[derive(serde::Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ClientMsg {
    Prompt { id: Option<String>, text: String },
    Ping { id: Option<String> },
}

#[derive(serde::Serialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ServerMsg<'a> {
    Ready {
        version: &'a str,
        websocket_path: &'a str,
    },
    Pong {
        id: Option<&'a str>,
    },
    Assistant {
        id: Option<&'a str>,
        text: &'a str,
    },
    Error {
        id: Option<&'a str>,
        message: &'a str,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve {
            host,
            port,
            web_root,
        } => serve(host, port, web_root).await,
    }
}

async fn serve(host: Option<String>, port: Option<u16>, web_root: Option<PathBuf>) -> anyhow::Result<()> {
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.unwrap_or(0);
    let web_root = web_root.unwrap_or_else(|| PathBuf::from("./web"));

    let agent = build_agent_from_env().ok();

    let state = AppState { web_root, agent };

    let assets_dir = state.web_root.join("assets");
    let assets_service = tower_http::services::ServeDir::new(assets_dir);

    let app = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/ws", get(ws_upgrade))
        .nest_service("/assets", assets_service)
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
        println!("  Note: no provider configured (set one of OPENAI_API_KEY / GROQ_API_KEY / ANTHROPIC_API_KEY)");
    }
    println!();

    axum::serve(listener, app)
        .await
        .with_context(|| "server exited with error")?;

    Ok(())
}

async fn index(State(state): State<AppState>) -> Response {
    let disk_path = state.web_root.join("index.html");

    match tokio::fs::read_to_string(&disk_path).await {
        Ok(html) => Html(html).into_response(),
        Err(_) => Html(include_str!("../../web/index.html").to_string()).into_response(),
    }
}

async fn ws_upgrade(State(state): State<AppState>, ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_session(state, socket))
}

async fn ws_session(state: AppState, mut socket: WebSocket) {
    let _ = socket
        .send(WsMessage::Text(
            serde_json::to_string(&ServerMsg::Ready {
                version: env!("CARGO_PKG_VERSION"),
                websocket_path: "/ws",
            })
            .unwrap_or_else(|_| r#"{"type":"ready","version":"unknown","websocket_path":"/ws"}"#.to_string()),
        ))
        .await;

    while let Some(Ok(msg)) = socket.recv().await {
        let WsMessage::Text(text) = msg else {
            continue;
        };

        let parsed: Result<ClientMsg, _> = serde_json::from_str(&text);
        let client_msg = match parsed {
            Ok(m) => m,
            Err(e) => {
                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::to_string(&ServerMsg::Error {
                            id: None,
                            message: &format!("invalid message: {e}"),
                        })
                        .unwrap_or_else(|_| r#"{"type":"error","message":"invalid message"}"#.to_string()),
                    ))
                    .await;
                continue;
            }
        };

        match client_msg {
            ClientMsg::Ping { id } => {
                let _ = socket
                    .send(WsMessage::Text(
                        serde_json::to_string(&ServerMsg::Pong {
                            id: id.as_deref(),
                        })
                        .unwrap_or_else(|_| r#"{"type":"pong"}"#.to_string()),
                    ))
                    .await;
            }
            ClientMsg::Prompt { id, text } => {
                let Some(agent) = &state.agent else {
                    let _ = socket
                        .send(WsMessage::Text(
                            serde_json::to_string(&ServerMsg::Error {
                                id: id.as_deref(),
                                message:
                                    "No provider configured. Set OPENAI_API_KEY / GROQ_API_KEY / ANTHROPIC_API_KEY.",
                            })
                            .unwrap_or_else(|_| {
                                r#"{"type":"error","message":"No provider configured"}"#.to_string()
                            }),
                        ))
                        .await;
                    continue;
                };

                let result = agent.run(text.trim()).await;
                match result {
                    Ok(reply) => {
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::to_string(&ServerMsg::Assistant {
                                    id: id.as_deref(),
                                    text: &reply,
                                })
                                .unwrap_or_else(|_| {
                                    r#"{"type":"assistant","text":"(serialization error)"}"#.to_string()
                                }),
                            ))
                            .await;
                    }
                    Err(e) => {
                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::to_string(&ServerMsg::Error {
                                    id: id.as_deref(),
                                    message: &format!("agent error: {e}"),
                                })
                                .unwrap_or_else(|_| r#"{"type":"error","message":"agent error"}"#.to_string()),
                            ))
                            .await;
                    }
                }
            }
        }
    }
}

fn build_agent_from_env() -> anyhow::Result<Arc<Agent>> {
    let provider: Box<dyn axonerai::provider::Provider> = if let Ok(key) = std::env::var("OPENAI_API_KEY") {
        Box::new(OpenAIProvider::new(key))
    } else if let Ok(key) = std::env::var("GROQ_API_KEY") {
        Box::new(GroqProvider::new(key))
    } else if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        Box::new(AnthropicProvider::new(key))
    } else {
        anyhow::bail!("no provider env var found");
    };

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(Calculator));
    registry.register(Box::new(WebScrape));

    // Only register WebSearch if it can run without immediately failing on missing env vars.
    let has_search_env =
        std::env::var("SEARCH_API_KEY").is_ok() && std::env::var("CX_ENGINE").is_ok();
    if has_search_env {
        registry.register(Box::new(WebSearch));
    }

    let system_prompt = Some(
        "You are a helpful assistant. You have several tools at your disposal. Use tools when needed."
            .to_string(),
    );

    Ok(Arc::new(Agent::new(provider, registry, system_prompt, None)))
}

