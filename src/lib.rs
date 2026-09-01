pub mod provider;
pub mod config;
pub mod mistral;
pub mod opencode;
pub mod groq;
pub mod openai;
pub mod tool;
pub mod tools;
pub mod executor;
pub mod agent;
pub mod session;
pub mod file_session_manager;

// Re-exporting main types for convenience
pub use agent::Agent;
pub use config::AppConfig;
pub use mistral::MistralProvider;
pub use opencode::OpenCodeProvider;
pub use groq::GroqProvider;
pub use openai::OpenAIProvider;
pub use tool::{Tool, ToolRegistry};
pub use tools::{Calculator, WebSearch, WebScrape};
pub use file_session_manager::FileSessionManager;
pub use session::Session;