use std::path::PathBuf;
use axonerai::agent::Agent;
use axonerai::provider::{Provider};
use std::env;
use axonerai::groq::GroqProvider;
use axonerai::tool::ToolRegistry;
use axonerai::tools::{WriteFile};
use axonerai::file_session_manager::FileSessionManager;
use uuid::Uuid;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Determine provider (default to Groq as requested by user)
    let api_key = env::var("GROQ_API_KEY")
        .expect("GROQ_API_KEY environment variable not set. Please source your .env file.");
    let provider: Box<dyn Provider> = Box::new(GroqProvider::new(api_key));

    // Register tools
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(WriteFile));
    println!("Available tools: {:?}", &tools.list_tools());

    // Session
    let session_id = Uuid::new_v4().to_string();
    // Use a temp dir for session storage to avoid clutter
    let file_session_manager = FileSessionManager::new(session_id , PathBuf::from("target/tmp_sessions/"))?;

    // Agent
    let system_prompt = "You are a helpful assistant with file writing capabilities.".to_string();
    let agent = Agent::new(
        provider,
        tools,
        Some(system_prompt),
        Some(file_session_manager)
    );

    let prompt = "Please write a file named 'hello_agent.txt' with the content: 'Hello from AxonerAI! This file was created by the agent.'";
    println!("Sending prompt: {}", prompt);

    let response = agent.run(prompt).await?;
    println!("Response: {}", response);

    Ok(())
}
