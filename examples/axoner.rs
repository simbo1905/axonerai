use axonerai::{Agent, AppConfig, GroqProvider, MistralProvider, OpenAIProvider, OpenCodeProvider, ToolRegistry};
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
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((key, value)) = line.split_once('=') {
                    let key = key.trim();
                    let value = value.trim();
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

    let config = AppConfig::load()?;

    let provider_type = env::var("AXONERAI_PROVIDER")
        .unwrap_or_else(|_| config.default_provider.clone());

    let model_id = env::var("AXONERAI_MODEL")
        .unwrap_or_else(|_| config.default_model_id(&provider_type).unwrap_or_default().to_string());

    let api_key = config.resolve_api_key(&provider_type)?;
    let endpoint = config.endpoint(&provider_type)?;

    let provider: Box<dyn Provider> = match provider_type.as_str() {
        "mistral" => {
            let mut p = MistralProvider::new(api_key);
            if !model_id.is_empty() { p = p.with_model(model_id); }
            Box::new(p)
        }
        "groq" => {
            let mut p = GroqProvider::new(api_key);
            if !model_id.is_empty() { p = p.with_model(model_id); }
            Box::new(p)
        }
        "openai" => {
            let mut p = OpenAIProvider::new(api_key);
            if !model_id.is_empty() { p = p.with_model(model_id); }
            Box::new(p)
        }
        _ => {
            Box::new(OpenCodeProvider::new(api_key, endpoint.to_string(), model_id))
        }
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
