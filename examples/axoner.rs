use clap::Parser;
use uuid::Uuid;

use axonerai::provider::Provider;
use axonerai::tools::{
    Calculator, ListDir, ModelsConfig, ReadFile, TavilyMcpExtract, TavilyMcpSearch, WebFetch,
    WebSearch, WriteFile,
};
use axonerai::{
    Agent, AppConfig, GroqProvider, MistralProvider, OpenAIProvider, OpenCodeProvider, ToolRegistry,
};

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
                        // Safe even post-`#[tokio::main]`: the env crate's
                        // set_var returns None instead of UB when the
                        // process is multi-threaded (checked at call time).
                        let _ = env::set_var(key, value);
                    }
                }
            }
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "axonerai",
    about = "AxonerAI agent CLI",
    after_help = "Run with no flags for the demo prompt."
)]
struct Cli {
    /// Run the agent ONCE with this prompt and exit: final text to stdout,
    /// exit 0 (exit 1 on run error). If the value is a path to an existing
    /// .md file, the file's CONTENT is the prompt prefixed with the single
    /// line "Follow this skill exactly." (skill mode); any other value is a
    /// literal prompt. Session scope: STATELESS — no session file is created
    /// or reused; the interactive web session store is untouched.
    #[arg(long)]
    oneshot: Option<String>,

    /// Expose only read-only tools (registry-level `Tool::is_read_only()`
    /// filter): write_file is excluded, every read tool stays. Composes with
    /// --oneshot.
    #[arg(long)]
    tools_readonly: bool,

    /// In --oneshot mode, print the tool traces (ToolTrace lines) to STDERR;
    /// stdout stays the final answer only, so the output is pipeable.
    #[arg(long)]
    verbose: bool,
}

/// Build the provider for a provider-type string (config + env resolved
/// upstream by the caller).
fn build_provider(
    provider_type: &str,
    api_key: String,
    endpoint: &str,
    model_id: &str,
) -> Box<dyn Provider> {
    match provider_type {
        "mistral" => {
            let mut p = MistralProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        "groq" => {
            let mut p = GroqProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        "openai" => {
            let mut p = OpenAIProvider::new(api_key);
            if !model_id.is_empty() {
                p = p.with_model(model_id.to_string());
            }
            Box::new(p)
        }
        _ => Box::new(OpenCodeProvider::new(
            api_key,
            endpoint.to_string(),
            model_id.to_string(),
        )),
    }
}

/// Build the tool registry: builtins plus the Tavily-backed web tools (only
/// when an API key is available). When `tools_readonly` is set the registry
/// is filtered down to tools whose `Tool::is_read_only()` is true — a
/// registry-level filter, no tool-name special-casing.
fn build_registry(tools_readonly: bool) -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    tools.register(Box::new(Calculator));
    tools.register(Box::new(WriteFile::default()));
    tools.register(Box::new(ReadFile::default()));
    tools.register(Box::new(ListDir::default()));
    tools.register(Box::new(ModelsConfig::new()));
    // Only register the Tavily-backed web tools when an API key is available.
    if env::var("TAVILY_API_KEY").is_ok() {
        tools.register(Box::new(WebSearch::new()));
        tools.register(Box::new(WebFetch::new()));
        // Fake Tavily MCP facade: MCP-style tools with no MCP host process.
        tools.register(Box::new(TavilyMcpSearch::new()));
        tools.register(Box::new(TavilyMcpExtract::new()));
    }
    if tools_readonly {
        tools = tools.into_read_only();
    }
    tools
}

/// `tokio::main` starts a multi-threaded runtime before the body runs, and
/// `env::set_var` silently no-ops in a multi-threaded process — so the .env
/// load must happen BEFORE the runtime starts (plain `main`), with the async
/// body entered afterwards on the runtime.
fn main() -> anyhow::Result<()> {
    load_env_file();

    let cli = Cli::parse();
    tokio::runtime::Runtime::new()?.block_on(run(cli))
}

async fn run(cli: Cli) -> anyhow::Result<()> {
    let config = AppConfig::load()?;

    let provider_type =
        env::var("AXONERAI_PROVIDER").unwrap_or_else(|_| config.default_provider.clone());

    let model_id = env::var("AXONERAI_MODEL").unwrap_or_else(|_| {
        config
            .default_model_id(&provider_type)
            .unwrap_or_default()
            .to_string()
    });

    let api_key = config.resolve_api_key(&provider_type)?;
    let endpoint = config.endpoint(&provider_type)?;

    let system_prompt = Some(axonerai::prompt::load_system_prompt(
        &provider_type,
        &model_id,
    ));

    if let Some(oneshot_arg) = cli.oneshot.as_deref() {
        return run_oneshot_cli(
            &cli,
            &provider_type,
            api_key,
            &endpoint,
            &model_id,
            system_prompt,
            oneshot_arg,
        )
        .await;
    }

    // Demo mode (no flags): one canned prompt, chatty stdout as before.
    let provider = build_provider(&provider_type, api_key, &endpoint, &model_id);
    let tools = build_registry(cli.tools_readonly);

    println!("Available tools: {:?}", &tools.list_tools());

    let session_id = Uuid::new_v4().to_string();
    println!("Session ID: {}", session_id);

    let agent = Agent::new(provider, tools, system_prompt, None);

    let prompt = "Calculate 2 + 2".to_string();
    println!("User: {}", prompt);

    let response = agent.run(&prompt).await?;
    println!("Assistant: {}", response);

    Ok(())
}

/// `--oneshot` mode: run the agent ONCE with the given prompt (or skill
/// file), print the final assistant text to STDOUT and exit 0. Diagnostics
/// (and tool traces under `--verbose`) go to STDERR so stdout is pipeable.
/// Exit 1 on run error (the anyhow::Err return of main).
async fn run_oneshot_cli(
    cli: &Cli,
    provider_type: &str,
    api_key: String,
    endpoint: &str,
    model_id: &str,
    system_prompt: Option<String>,
    oneshot_arg: &str,
) -> anyhow::Result<()> {
    let provider = build_provider(provider_type, api_key, endpoint, model_id);
    let tools = build_registry(cli.tools_readonly);

    let prompt = axonerai::oneshot::resolve_prompt(oneshot_arg);
    eprintln!(
        "axonerai --oneshot (provider: {provider_type}, model: {model_id}, tools: {:?})",
        tools.list_tools()
    );

    let (text, traces) =
        axonerai::oneshot::run_oneshot(provider, tools, system_prompt, &prompt).await?;

    if cli.verbose {
        for trace in &traces {
            eprintln!(
                "ToolTrace tool={} duration_ms={} bytes_up={} bytes_down={}",
                trace.tool, trace.duration_ms, trace.bytes_up, trace.bytes_down
            );
            eprintln!("  args: {}", trace.args_json);
            eprintln!("  result: {}", trace.result_json);
        }
    }

    println!("{text}");
    Ok(())
}
