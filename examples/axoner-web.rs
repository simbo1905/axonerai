use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Context;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::State;
use axum::response::{Html, IntoResponse, Response};
use axum::routing::get;
use axum::Router;
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;
use tracing::info;

use axonerai::{Agent, AppConfig, GroqProvider, MistralProvider, OpenAIProvider, OpenCodeProvider, ToolRegistry};
use axonerai::tools::{Calculator, WebScrape, WebSearch, WriteFile};
use axonerai::wire::{ClientMsg, ServerMsg};

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
     },
}

#[derive(Clone)]
struct AppState {
    web_root: PathBuf,
    agent: Option<Arc<Agent>>,
    verbose: u8,
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

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    load_env_file();

    let cli = Cli::parse();
    
    let log_level = match cli.verbose {
        0 => "warn",
        1 => "axonerai=debug,axoner_web=debug,warn",
        _ => "axonerai=trace,axoner_web=trace,debug",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));
    
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .init();

    match cli.command {
        Commands::Serve {
            host,
            port,
            web_root,
            provider,
            model,
        } => serve(host, port, web_root, provider, model).await,
    }
}

async fn serve(
    host: Option<String>,
    port: Option<u16>,
    web_root: Option<PathBuf>,
    provider_override: Option<String>,
    model_override: Option<String>,
) -> anyhow::Result<()> {
    let host = host.unwrap_or_else(|| "127.0.0.1".to_string());
    let port = port.unwrap_or(0);
    let web_root = web_root.unwrap_or_else(|| PathBuf::from("./web"));

    let config = AppConfig::load()?;

    let provider_name = provider_override
        .unwrap_or_else(|| std::env::var("AXONERAI_PROVIDER").unwrap_or_else(|_| config.default_provider.clone()));

    let model_id = model_override
        .or_else(|| std::env::var("AXONERAI_MODEL").ok())
        .unwrap_or_else(|| config.default_model_id(&provider_name).unwrap_or_default().to_string());

    let agent = build_agent_from_config(&config, &provider_name, &model_id).ok();

    let cli = Cli::parse();
    let state = AppState { web_root, agent, verbose: cli.verbose };

    let assets_dir = state.web_root.join("assets");
    let assets_service = tower_http::services::ServeDir::new(assets_dir);
    let src_service = tower_http::services::ServeDir::new(state.web_root.join("src"));
    let test_service = tower_http::services::ServeDir::new(state.web_root.join("test"));
    let generated_service =
        tower_http::services::ServeDir::new(state.web_root.join("generated"));

    let app = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/ws", get(ws_upgrade))
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
        println!("  Note: no provider configured (set MISTRAL_API_KEY / OPENCODE_API_KEY / GROQ_API_KEY)");
    }
    println!();
    println!("  Provider:   {}", provider_name);
    println!("  Model:      {}", model_id);
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
        Err(_) => Html(include_str!("../web/index.html").to_string()).into_response(),
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
            .unwrap_or_else(|_| r#"{"_type":"ready","version":"unknown","websocket_path":"/ws"}"#.to_string()),
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
                        .unwrap_or_else(|_| r#"{"_type":"error","message":"invalid message"}"#.to_string()),
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
                        .unwrap_or_else(|_| r#"{"_type":"pong"}"#.to_string()),
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
                                    "No provider configured. Set MISTRAL_API_KEY / OPENCODE_API_KEY / GROQ_API_KEY.",
                            })
                            .unwrap_or_else(|_| {
                                r#"{"_type":"error","message":"No provider configured"}"#.to_string()
                            }),
                        ))
                        .await;
                    continue;
                };

                let result = agent.run(text.trim()).await;
                match result {
                    Ok(reply) => {
                        let timestamp = chrono::DateTime::<chrono::Utc>::from(SystemTime::now())
                            .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                        
                        match state.verbose {
                            0 => info!("[{}] Response: {} bytes", timestamp, reply.len()),
                            1 => {
                                let preview = if reply.len() > 77 {
                                    format!("{}...", &reply[..77])
                                } else {
                                    reply.clone()
                                };
                                info!("[{}] Response: {} bytes - {}", timestamp, reply.len(), preview);
                            }
                            _ => info!("[{}] Response: {} bytes\n{}", timestamp, reply.len(), reply),
                        }

                        let _ = socket
                            .send(WsMessage::Text(
                                serde_json::to_string(&ServerMsg::Assistant {
                                    id: id.as_deref(),
                                    text: &reply,
                                })
                                .unwrap_or_else(|_| {
                                    r#"{"_type":"assistant","text":"(serialization error)"}"#.to_string()
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
                                .unwrap_or_else(|_| r#"{"_type":"error","message":"agent error"}"#.to_string()),
                            ))
                            .await;
                    }
                }
            }
        }
    }
}

fn build_agent_from_config(config: &AppConfig, provider_name: &str, model_id: &str) -> anyhow::Result<Arc<Agent>> {
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

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(Calculator));
    registry.register(Box::new(WebScrape));
    registry.register(Box::new(WriteFile));

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
