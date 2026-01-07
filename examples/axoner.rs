use std::path::PathBuf;

use axonerai::{Agent, AnthropicProvider, GroqProvider, OpenAIProvider, ToolRegistry};
use axonerai::provider::Provider;
use axonerai::tools::{Calculator, WebScrape, WebSearch, WriteFile};
use uuid::Uuid;

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
                    if env::var(key).is_err() {
                        unsafe {
                            env::set_var(key, value);
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

    let provider_type = env::var("PROVIDER_TYPE").unwrap_or("groq".to_string());

    let provider: Box<dyn Provider> = match provider_type.as_str() {
        "anthropic" => {
            let api_key = env::var("ANTHROPIC_API_KEY")
                .expect("ANTHROPIC_API_KEY environment variable not set");
            Box::new(AnthropicProvider::new(api_key))
        },
        "openai" => {
            let api_key = env::var("OPENAI_API_KEY")
                .expect("OPENAI_API_KEY environment variable not set");
            Box::new(OpenAIProvider::new(api_key))
        },
        "groq" | _ => {
            let api_key = env::var("GROQ_API_KEY")
                .expect("GROQ_API_KEY environment variable not set");
            Box::new(GroqProvider::new(api_key))
        },
    };

    let mut tools = ToolRegistry::new();
    tools.register(Box::new(Calculator));
    tools.register(Box::new(WebSearch));
    tools.register(Box::new(WebScrape));
    tools.register(Box::new(WriteFile));
    println!("Available tools: {:?}", &tools.list_tools());

    let session_id = Uuid::new_v4().to_string();
    println!("Session ID: {}", session_id);

    let agent = Agent::new(provider, tools, None, None);

    let prompt = "Calculate 2 + 2".to_string();
    println!("User: {}", prompt);

    let response = agent.run(&prompt).await?;
    println!("Assistant: {}", response);

    Ok(())
}
