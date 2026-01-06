//! AxonerAI Agent CLI
//!
//! Usage:
//!   agt serve [OPTIONS]    Start the web server
//!   agt chat               Start interactive chat mode

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::env;

use axonerai::provider::Provider;
use axonerai::anthropic::AnthropicProvider;
use axonerai::openai::OpenAIProvider;
use axonerai::groq::GroqProvider;
use axonerai::tool::ToolRegistry;
use axonerai::tools::{Calculator, WebSearch, WebScrape};
use axonerai::server::{ServerConfig, start_server};

#[derive(Parser)]
#[command(name = "agt")]
#[command(author = "AxonerAI Team")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "AxonerAI Agent - A type-safe, blazing fast agentic AI framework")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the web server with chat interface
    Serve {
        /// Host address to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        
        /// Port to listen on (auto-selects high port if not specified)
        #[arg(short, long)]
        port: Option<u16>,
        
        /// Directory for static files (index.html, assets/)
        #[arg(long, default_value = "./static")]
        static_dir: PathBuf,
        
        /// Directory for session storage
        #[arg(long, default_value = "./sessions")]
        sessions_dir: PathBuf,
        
        /// Use in-memory session storage instead of file-based
        #[arg(long)]
        memory_storage: bool,
        
        /// LLM provider to use (groq, openai, anthropic)
        #[arg(long, default_value = "groq")]
        provider: String,
        
        /// System prompt for the agent
        #[arg(long)]
        system_prompt: Option<String>,
        
        /// Disable built-in tools
        #[arg(long)]
        no_tools: bool,
    },
    
    /// Start interactive chat mode (CLI)
    Chat {
        /// LLM provider to use (groq, openai, anthropic)
        #[arg(long, default_value = "groq")]
        provider: String,
        
        /// Session ID to continue (optional)
        #[arg(long)]
        session: Option<String>,
        
        /// System prompt for the agent
        #[arg(long)]
        system_prompt: Option<String>,
    },
}

fn create_provider(provider_type: &str) -> anyhow::Result<Box<dyn Provider>> {
    match provider_type.to_lowercase().as_str() {
        "anthropic" => {
            let api_key = env::var("ANTHROPIC_API_KEY")
                .map_err(|_| anyhow::anyhow!("ANTHROPIC_API_KEY environment variable not set"))?;
            Ok(Box::new(AnthropicProvider::new(api_key)))
        }
        "openai" => {
            let api_key = env::var("OPENAI_API_KEY")
                .map_err(|_| anyhow::anyhow!("OPENAI_API_KEY environment variable not set"))?;
            Ok(Box::new(OpenAIProvider::new(api_key)))
        }
        "groq" => {
            let api_key = env::var("GROQ_API_KEY")
                .map_err(|_| anyhow::anyhow!("GROQ_API_KEY environment variable not set"))?;
            Ok(Box::new(GroqProvider::new(api_key)))
        }
        _ => Err(anyhow::anyhow!("Unknown provider: {}. Use groq, openai, or anthropic", provider_type)),
    }
}

fn create_tool_registry(include_tools: bool) -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    
    if include_tools {
        registry.register(Box::new(Calculator));
        registry.register(Box::new(WebSearch));
        registry.register(Box::new(WebScrape));
    }
    
    registry
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    
    match cli.command {
        Commands::Serve {
            host,
            port,
            static_dir,
            sessions_dir,
            memory_storage,
            provider,
            system_prompt,
            no_tools,
        } => {
            // Create provider
            let llm_provider = match create_provider(&provider) {
                Ok(p) => Some(p),
                Err(e) => {
                    eprintln!("⚠️  Warning: {}", e);
                    eprintln!("   Server will start but chat will be unavailable until a provider is configured.");
                    None
                }
            };
            
            // Create tool registry
            let registry = create_tool_registry(!no_tools);
            
            // Default system prompt
            let default_system_prompt = "You are a helpful assistant. You have several tools at your disposal. \
                Use them to provide accurate and helpful information.".to_string();
            let system_prompt = system_prompt.or(Some(default_system_prompt));
            
            // Create server config
            let config = ServerConfig {
                host,
                port,
                static_dir,
                sessions_dir,
                use_null_delimited: !memory_storage,
            };
            
            // Ensure static directory exists
            std::fs::create_dir_all(&config.static_dir)?;
            std::fs::create_dir_all(&config.sessions_dir)?;
            
            // Start server
            start_server(config, llm_provider, registry, system_prompt).await?;
        }
        
        Commands::Chat {
            provider,
            session,
            system_prompt,
        } => {
            use axonerai::agent::Agent;
            use axonerai::file_session_manager::FileSessionManager;
            use std::io::{self, Write};
            
            // Create provider
            let llm_provider = create_provider(&provider)?;
            
            // Create tool registry
            let registry = create_tool_registry(true);
            
            // Create session manager
            let session_id = session.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
            let file_session_manager = FileSessionManager::new(
                session_id.clone(),
                PathBuf::from("./sessions"),
            )?;
            
            // Default system prompt
            let default_system_prompt = "You are a helpful assistant. You have several tools at your disposal. \
                Use them to provide accurate and helpful information.".to_string();
            let system_prompt = system_prompt.or(Some(default_system_prompt));
            
            // Create agent
            let agent = Agent::new(
                llm_provider,
                registry,
                system_prompt,
                Some(file_session_manager),
            );
            
            println!("🤖 AxonerAI Agent (type 'quit' or 'exit' to stop)");
            println!("   Session: {}", session_id);
            println!();
            
            let mut input = String::new();
            loop {
                print!("You: ");
                io::stdout().flush()?;
                
                input.clear();
                io::stdin().read_line(&mut input)?;
                
                let trimmed = input.trim();
                if trimmed.eq_ignore_ascii_case("quit") || trimmed.eq_ignore_ascii_case("exit") {
                    println!("Goodbye! 👋");
                    break;
                }
                
                if trimmed.is_empty() {
                    continue;
                }
                
                let start = std::time::Instant::now();
                match agent.run(trimmed).await {
                    Ok(response) => {
                        println!();
                        println!("Agent: {}", response);
                        println!("       [{:.2}s]", start.elapsed().as_secs_f64());
                        println!();
                    }
                    Err(e) => {
                        eprintln!("Error: {}", e);
                    }
                }
            }
        }
    }
    
    Ok(())
}
