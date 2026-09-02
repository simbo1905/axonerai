# AxonerAI

A type-safe, high-performance agentic AI framework in Rust.

**Why AxonerAI?**
- 🦀 **Type-safe** - Catch errors at compile time, not runtime
- ⚡ **Fast** - Native Rust performance, no Python overhead
- 🔧 **Simple** - Clean API, minimal boilerplate
- 📦 **Single binary** - No dependency hell, easy deployment

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
axonerai = "0.1"
tokio = { version = "1", features = ["full"] }
uuid = { version = "1", features = ["v4"] }
```

## Quick Start

```rust
use axonerai::agent::Agent;
use axonerai::mistral::MistralProvider;
use axonerai::tool::ToolRegistry;
use axonerai::tools::{Calculator, WebSearch};
use axonerai::file_session_manager::FileSessionManager;
use std::path::PathBuf;
use std::time::Instant;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Create a provider
    let provider = MistralProvider::new(
        std::env::var("MISTRAL_API_KEY")?
    );

    // 2. Register tools
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(Calculator));
    registry.register(Box::new(WebSearch::new()));

    // 3. Create session manager (optional - pass None for stateless)
    let session_manager = FileSessionManager::new(
        uuid::Uuid::new_v4().to_string(),
        PathBuf::from("./sessions"),
    )?;

    // 4. Create agent
    let system_prompt = "You are a helpful assistant. You have several tools at your disposal. \
        Do not give information without proper usage of tools.".to_string();
    
    let agent = Agent::new(
        Box::new(provider),
        registry,
        Some(system_prompt),
        Some(session_manager), // Or None for stateless
    );

    // 5. Interactive loop
    let mut input = String::new();
    println!("🤖 Agent starting... (type 'quit' to exit)");
    
    loop {
        println!("\nYou 🦁: ");
        println!("---------------");
        std::io::stdin().read_line(&mut input).expect("Failed to read line");
        
        if input.trim().to_lowercase() == "quit" || input.trim().to_lowercase() == "exit" {
            println!("Bye! 👋");
            break;
        }

        let start_time = Instant::now();
        let response = agent.run(input.trim()).await?;
        let elapsed = start_time.elapsed();
        
        println!("----------------------------------------");
        println!("{}", response);
        println!("Time taken: {:?}", elapsed);
        
        input.clear();
    }

    Ok(())
}
```

## Supported Providers

Providers and models are configured in `.axonerai/axonerai.jsonc` (the config file
wins; built-in defaults are used otherwise).

| Provider | Models |
|----------|--------|
| **Mistral** (default) | `zai-glm-5-2`, `mistral-medium-latest` |
| **Groq** | `qwen/qwen3.8-27b`, `openai/gpt-oss-20b`, `openai/gpt-oss-120b` |
| **OpenCode Zen** | `glm-5.2` |
| **OpenCode Go** | `glm-5.2` |

```rust
// Mistral (default)
use axonerai::mistral::MistralProvider;
let provider = MistralProvider::new(api_key);

// Groq (free tier available!)
use axonerai::groq::GroqProvider;
let provider = GroqProvider::new(api_key);

// OpenCode Zen / OpenCode Go
use axonerai::opencode::OpenCodeProvider;
let provider = OpenCodeProvider::new(api_key, endpoint, model);
```

## Built-in Tools

- **Calculator** - Basic arithmetic operations
- **WebSearch** - Search the web via the Tavily API
- **WebFetch** - Fetch and extract the text content of a web page via the Tavily API
- **WriteFile** - Write content to files

The web tools (WebSearch and WebFetch) are registered when `TAVILY_API_KEY` is set.

## Running the Demo

AxonerAI includes two demo applications:

### Minimal Demo (`axoner`)
A stateless CLI agent with basic tool usage:

```bash
cargo run --example axoner --release
```

### Web UI Demo (`axoner-web`)
An interactive web UI with WebSocket support for real-time agent communication.

The web UI uses the VanillaJS approach: a plain ES-module JavaScript client (`web/assets/client.mjs`)
served as static files — no React, no Babel, no bundler, no build step. Type checking is done with
`tsc --noEmit` via JSDoc annotations.

```bash
# Default (warn level logging)
cargo run --example axoner-web --features web --release -- serve --port 9090

# With verbose logging (debug level)
cargo run --example axoner-web --features web --release -- -v serve --port 9090

# Pick a specific provider and model
cargo run --example axoner-web --features web --release -- serve --port 9090 --provider mistral --model zai-glm-5-2
```

Open `http://127.0.0.1:9090/` in your browser.

The system prompt is composed from `prompts/` at build time (see
[System Prompts](#system-prompts) below) and is provider+model-specific when a
matching prompt exists, otherwise the default prompt is used.

The web demo requires:
- `MISTRAL_API_KEY`, `GROQ_API_KEY`, or `OPENCODE_API_KEY` environment variable (or in `.env` file)
- Optional: `TAVILY_API_KEY` for the web tools (WebSearch and WebFetch)

## System Prompts

- `prompts/base.txt` is the one true base prompt.
- `prompts/models/<provider>--<model>.patch` files adjust it per provider/model
  using `# PREPEND` / `# APPEND` / `# REPLACE` sections.
- `make prompts` composes the patches into `prompts/generated/` (the generated
  files are committed).
- The server uses the provider+model-specific prompt if present, else the
  default. Prompt composition happens at build time only.

## Evals

`make evals` runs [promptfoo](https://promptfoo.dev) over the 7-config matrix
(groq×3, mistral×2, opencode zen/go) against the local servers. Tests use
deterministic asserts. You need the API keys in `.env`. Results are written to
`.tmp/evals/`.

## Local server tooling

`make serve-up` / `make serve-down` / `make serve-status` / `make serve-logs`
manage local `axoner-web` server processes (one per provider/model/port, with
pidfiles and logs in `.tmp/run/`); `make build-server` builds the release
binary.

## Creating Custom Tools

```rust
use axonerai::tool::{Tool, ToolResult};
use serde_json::{json, Value};
use anyhow::Result;

pub struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> String {
        "my_tool".to_string()
    }

    fn description(&self) -> String {
        "Does something useful".to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": {
                    "type": "string",
                    "description": "The input to process"
                }
            },
            "required": ["input"]
        })
    }

    fn execute(&self, input: Value) -> Result<String> {
        let input_str = input["input"].as_str().unwrap_or("");
        Ok(format!("Processed: {}", input_str))
    }
}
```

Register it:

```rust
use axonerai::tool::ToolRegistry;

let mut registry = ToolRegistry::new();
registry.register(Box::new(MyTool));
```

## Environment Variables

```bash
# Required for your chosen provider
MISTRAL_API_KEY=your_mistral_key
GROQ_API_KEY=your_groq_key
OPENCODE_API_KEY=your_opencode_key

# For the WebSearch and WebFetch tools (Tavily)
TAVILY_API_KEY=your_tavily_key
```

## Features

- [x] Multi-provider support (Mistral, Groq, OpenCode Zen/Go)
- [x] Tool system with custom tool support
- [x] Session management (file-based)
- [x] System prompts

## Comparison with Python Frameworks

| Feature | AxonerAI | LangChain |
|---------|----------|----------|
| Language | Rust | Python |
| Type Safety |  Compile-time |  Runtime |
| Performance |  Fast |  Slow |
| Learning Curve | Moderate | Easy |

## Responsible Use

**Important:** AxonerAI provides tools for web search and web fetching. Users are responsible for ensuring their use complies with applicable laws, terms of service, and ethical guidelines.

### Web Tools Guidelines

**WebSearch and WebFetch Tools (Tavily):**
- Require a `TAVILY_API_KEY` environment variable
- Subject to Tavily's terms of service
- Respect rate limits on your Tavily plan

**WebFetch Tool:**
- Only fetches content the user or model explicitly requests via URL
- Check website Terms of Service before fetching content
- Some websites explicitly prohibit automated fetching
- Consider legal implications in your jurisdiction

### API Provider Terms

When using AxonerAI with LLM providers, you must comply with their respective terms:
- [OpenAI Terms of Use](https://openai.com/policies/terms-of-use)
- [Groq Terms of Service](https://groq.com/terms-of-service/)

### Your Responsibility

By using AxonerAI, you agree that:
- You are responsible for your use of the framework and its tools
- You will comply with all applicable laws and third-party terms of service
- The maintainers of AxonerAI are not liable for misuse of the framework

### Use responsibly. Respect the web.

## License

MIT

## Contributing

Contributions welcome! Please open an issue or PR on GitHub.
