pub mod agent;
pub mod config;
pub mod executor;
pub mod file_session_manager;
pub mod groq;
pub mod mistral;
pub mod openai;
pub mod opencode;
pub mod provider;
pub mod session;
pub mod tool;
pub mod tools;
pub mod wire;

// Re-exporting main types for convenience
pub use agent::Agent;
pub use config::AppConfig;
pub use file_session_manager::FileSessionManager;
pub use groq::GroqProvider;
pub use mistral::MistralProvider;
pub use openai::OpenAIProvider;
pub use opencode::OpenCodeProvider;
pub use session::Session;
pub use tool::{Tool, ToolRegistry};
pub use tools::{Calculator, WebFetch, WebSearch};
pub use wire::{ClientMsg, ServerMsg};
