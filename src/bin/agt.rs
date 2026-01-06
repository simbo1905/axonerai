use clap::{Parser, Subcommand};
use axum::{
    routing::{get, get_service},
    Router,
    extract::{ws::{Message as WsMessage, WebSocket, WebSocketUpgrade}},
    response::{Response},
};
use tower_http::services::{ServeDir, ServeFile};
use std::path::PathBuf;
use std::env;
use uuid::Uuid;
use axonerai::{
    Agent,
    provider::Provider,
    tool::ToolRegistry,
    tools::{WebScrape, WebSearch, Calculator},
    flat_file_session::FlatFileSessionManager,
    anthropic::AnthropicProvider,
    openai::OpenAIProvider,
    groq::GroqProvider,
};
use tracing::{info, error};

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the agent server
    Serve {
        /// Port to listen on (optional, defaults to random high port)
        #[arg(short, long)]
        port: Option<u16>,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => {
            serve(port).await;
        }
    }
}

async fn serve(port: Option<u16>) {
    let port = port.unwrap_or(0);
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    
    println!("🚀 Server running at: http://{}/", local_addr);
    
    let app = Router::new()
        .route("/", get_service(ServeFile::new("index.html")))
        .nest_service("/assets", ServeDir::new("assets"))
        .route("/ws", get(ws_handler));

    axum::serve(listener, app).await.unwrap();
}

async fn ws_handler(ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    let session_id = Uuid::new_v4().to_string();
    info!("New WebSocket connection: {}", session_id);

    // Initialize tools
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(Calculator));
    tools.register(Box::new(WebSearch));
    tools.register(Box::new(WebScrape));

    // Initialize Provider
    let provider_type = env::var("PROVIDER_TYPE").unwrap_or("groq".to_string());
    let provider: Box<dyn Provider> = match provider_type.as_str() {
        "anthropic" => {
            let api_key = env::var("ANTHROPIC_API_KEY").expect("ANTHROPIC_API_KEY not set");
            Box::new(AnthropicProvider::new(api_key))
        },
        "openai" => {
            let api_key = env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY not set");
            Box::new(OpenAIProvider::new(api_key))
        },
        "groq" | _ => {
            let api_key = env::var("GROQ_API_KEY").expect("GROQ_API_KEY not set");
            Box::new(GroqProvider::new(api_key))
        },
    };

    // Initialize Session Manager
    let session_dir = PathBuf::from("sessions");
    let session_manager = Box::new(FlatFileSessionManager::new(session_id.clone(), session_dir));
    
    let system_prompt = "You are a helpful assistant. You have access to tools.".to_string();
    
    let agent = Agent::new(
        provider,
        tools,
        Some(system_prompt),
        Some(session_manager)
    );

    // Loop to handle messages
    while let Some(msg) = socket.recv().await {
        if let Ok(msg) = msg {
            match msg {
                WsMessage::Text(text) => {
                    // Expecting JSON: { "content": "..." }
                    let text_str = text.as_str(); // Utf8Bytes to &str
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(text_str) {
                        if let Some(content) = json.get("content").and_then(|v| v.as_str()) {
                            info!("Received message: {}", content);
                            
                            // Run agent
                            match agent.run(content).await {
                                Ok(response) => {
                                    let resp_json = serde_json::json!({
                                        "type": "response",
                                        "content": response
                                    });
                                    if let Err(e) = socket.send(WsMessage::Text(resp_json.to_string().into())).await {
                                        error!("Failed to send response: {}", e);
                                        break;
                                    }
                                }
                                Err(e) => {
                                    let err_json = serde_json::json!({
                                        "type": "error",
                                        "content": e.to_string()
                                    });
                                    let _ = socket.send(WsMessage::Text(err_json.to_string().into())).await;
                                }
                            }
                        }
                    }
                }
                WsMessage::Close(_) => {
                    break;
                }
                _ => {}
            }
        } else {
            break;
        }
    }
    
    info!("WebSocket connection closed: {}", session_id);
}
