use axonerai::{Agent, GroqProvider, AnthropicProvider, OpenAIProvider, ToolRegistry, FileSessionManager};
use axonerai::tools::{Calculator, WebSearch, WebScrape};
use axonerai::provider::Provider;
use clap::{Parser, Subcommand};
use std::env;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "agt")]
#[command(about = "AxonerAI command-line interface", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the web-based chat interface server
    Serve {
        /// Port to listen on (default: 4096, fallback to any available)
        #[arg(short, long, default_value_t = 4096)]
        port: u16,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { port } => {
            // Initialize provider
            let provider_type = env::var("PROVIDER_TYPE").unwrap_or("groq".to_string());

            let provider: Box<dyn Provider> = match provider_type.as_str() {
                "anthropic" => {
                    let api_key = env::var("ANTHROPIC_API_KEY")
                        .expect("ANTHROPIC_API_KEY environment variable not set");
                    Box::new(AnthropicProvider::new(api_key))
                }
                "openai" => {
                    let api_key = env::var("OPENAI_API_KEY")
                        .expect("OPENAI_API_KEY environment variable not set");
                    Box::new(OpenAIProvider::new(api_key))
                }
                "groq" | _ => {
                    let api_key = env::var("GROQ_API_KEY")
                        .expect("GROQ_API_KEY environment variable not set");
                    Box::new(GroqProvider::new(api_key))
                }
            };

            // Initialize tools
            let mut tools = ToolRegistry::new();
            tools.register(Box::new(Calculator));
            tools.register(Box::new(WebSearch));
            tools.register(Box::new(WebScrape));

            // Initialize session manager
            let session_id = Uuid::new_v4().to_string();
            let file_session_manager = FileSessionManager::new(session_id, PathBuf::from("./tmp/"))?;

            // Initialize agent
            let system_prompt = "You are a helpful assistant. You have several tools at your disposal. \
                Do not give information without proper usage of tools. You are smart, but you rely on tools \
                for information.".to_string();

            let agent = Agent::new(
                provider,
                tools,
                Some(system_prompt),
                Some(file_session_manager),
            );

            // Start the server
            axonerai::serve::start_server(agent, port).await?;
        }
    }

    Ok(())
}
