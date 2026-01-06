use anyhow::{anyhow, Result};
use axonerai::provider::Provider;
use axonerai::server::{serve, ServeConfig, SessionMode};
use axonerai::tool::ToolRegistry;
use axonerai::tools::Calculator;
use axonerai::{AnthropicProvider, GroqProvider, OpenAIProvider};
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;
use std::sync::Arc;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "agt", version, about = "AxonerAI CLI")]
struct Cli {
    #[command(subcommand)]
    cmd: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start the local web server + websocket UI.
    Serve {
        /// Bind host (default 127.0.0.1)
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Bind port (0 picks an available high port)
        #[arg(long, default_value_t = 0)]
        port: u16,

        /// Directory containing `index.html` and `assets/`
        #[arg(long, default_value = "./web")]
        web_dir: PathBuf,

        /// Session storage directory
        #[arg(long, default_value = "./sessions")]
        sessions_dir: PathBuf,

        /// Session storage mode
        #[arg(long, value_enum, default_value_t = SessionModeArg::NullDelimitedFile)]
        session_mode: SessionModeArg,

        /// Optional system prompt
        #[arg(long)]
        system_prompt: Option<String>,
    },
}

#[derive(Copy, Clone, Debug, ValueEnum)]
enum SessionModeArg {
    Stateless,
    JsonFile,
    NullDelimitedFile,
}

impl From<SessionModeArg> for SessionMode {
    fn from(v: SessionModeArg) -> Self {
        match v {
            SessionModeArg::Stateless => SessionMode::Stateless,
            SessionModeArg::JsonFile => SessionMode::JsonFile,
            SessionModeArg::NullDelimitedFile => SessionMode::NullDelimitedFile,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();

    match cli.cmd {
        Command::Serve {
            host,
            port,
            web_dir,
            sessions_dir,
            session_mode,
            system_prompt,
        } => {
            let provider = pick_provider_from_env()?;

            let mut registry = ToolRegistry::new();
            registry.register(Box::new(Calculator));

            let cfg = ServeConfig {
                host,
                port,
                web_dir,
                sessions_dir,
                session_mode: session_mode.into(),
                system_prompt,
            };

            serve(provider, registry, cfg).await?;
        }
    }

    Ok(())
}

fn pick_provider_from_env() -> Result<Arc<dyn Provider>> {
    if let Ok(k) = std::env::var("GROQ_API_KEY") {
        return Ok(Arc::new(GroqProvider::new(k)));
    }
    if let Ok(k) = std::env::var("OPENAI_API_KEY") {
        return Ok(Arc::new(OpenAIProvider::new(k)));
    }
    if let Ok(k) = std::env::var("ANTHROPIC_API_KEY") {
        return Ok(Arc::new(AnthropicProvider::new(k)));
    }
    Err(anyhow!(
        "No provider API key found. Set one of GROQ_API_KEY, OPENAI_API_KEY, ANTHROPIC_API_KEY."
    ))
}

