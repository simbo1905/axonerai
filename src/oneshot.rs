//! `--oneshot` CLI mode (item45): run the agent ONCE for a single prompt and
//! exit, printing the final assistant text to stdout (exit 0; exit 1 on run
//! error) so the output is pipeable. Tool traces are collected here and the
//! caller prints them to STDERR when `--verbose` is set — stdout stays the
//! final answer only.
//!
//! Session scope: STATELESS. No session file is created or reused; the
//! interactive web session store is untouched.

use std::path::Path;

use anyhow::Result;

use crate::agent::{Agent, ToolTrace};
use crate::provider::Provider;
use crate::tool::ToolRegistry;

/// The single line prefixed to a skill file's content when the `--oneshot`
/// argument resolves to an existing `.md` path. Nothing else is added.
pub const SKILL_PROMPT_PREFIX: &str = "Follow this skill exactly.";

/// Resolve the `--oneshot` argument into the prompt actually sent to the
/// model: an argument that is a path to an EXISTING `.md` file becomes the
/// file's content prefixed with the single [`SKILL_PROMPT_PREFIX`] line;
/// anything else (including a non-existent path or an existing non-`.md`
/// file) is treated as a literal prompt.
pub fn resolve_prompt(arg: &str) -> String {
    let path = Path::new(arg);
    if arg.ends_with(".md") && path.is_file() {
        if let Ok(content) = std::fs::read_to_string(path) {
            return format!("{SKILL_PROMPT_PREFIX}\n{content}");
        }
    }
    arg.to_string()
}

/// Run the agent ONCE with the given (already resolved) prompt.
///
/// Returns `(final_text, traces)`: the final assistant text and one
/// [`ToolTrace`] per tool execution. The agent runs STATELESS (no
/// `FileSessionManager` — nothing is persisted) and in quiet mode so no
/// loop chatter reaches stdout; the caller decides where traces go (the
/// CLI prints them to stderr under `--verbose`).
pub async fn run_oneshot(
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    system_prompt: Option<String>,
    prompt: &str,
) -> Result<(String, Vec<ToolTrace>)> {
    let mut agent = Agent::new(provider, registry, system_prompt, None);
    agent.set_quiet(true);

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ToolTrace>();
    let text = agent.run_with_traces(prompt, tx).await?;

    // The run owns the sender and drops it on return, so this drains the
    // traces collected during the run and then ends.
    let mut traces = Vec::new();
    while let Ok(trace) = rx.try_recv() {
        traces.push(trace);
    }
    Ok((text, traces))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        CompletionResponse, Message, StopReason, Tool as ProviderTool, ToolCall,
    };
    use crate::tool::Tool as ToolTrait;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    struct EchoTool;

    #[async_trait]
    impl ToolTrait for EchoTool {
        fn name(&self) -> String {
            "echo_tool".to_string()
        }

        fn description(&self) -> String {
            "echoes its input".to_string()
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, input: Value) -> Result<String> {
            Ok(input.to_string())
        }
    }

    #[derive(Clone, Default)]
    struct SharedCalls(Arc<Mutex<Vec<Vec<Message>>>>);

    impl SharedCalls {
        fn push(&self, messages: Vec<Message>) {
            self.0.lock().unwrap().push(messages);
        }

        fn len(&self) -> usize {
            self.0.lock().unwrap().len()
        }
    }

    /// One ToolUse round (echo_tool) then EndTurn with the fixed final text.
    struct MockProvider {
        calls: SharedCalls,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn complete(
            &self,
            messages: Vec<Message>,
            _tools: Option<Vec<ProviderTool>>,
            _max_tokens: Option<u32>,
            _system_prompt: Option<String>,
        ) -> Result<CompletionResponse> {
            self.calls.push(messages);
            if self.calls.len() == 1 {
                Ok(CompletionResponse {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({"expression": "2^7.2"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                })
            } else {
                Ok(CompletionResponse {
                    text: Some("147.03".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                })
            }
        }
    }

    #[tokio::test]
    async fn oneshot_returns_final_text_and_traces_to_sink() {
        let calls = SharedCalls::default();
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let (text, traces) = run_oneshot(
            Box::new(MockProvider {
                calls: calls.clone(),
            }),
            registry,
            None,
            "what is 2^7.2",
        )
        .await
        .expect("oneshot run");

        assert_eq!(text, "147.03", "final assistant text is returned");
        assert_eq!(calls.len(), 2, "one ToolUse round + one EndTurn round");
        assert_eq!(traces.len(), 1, "one trace per tool execution");
        assert_eq!(traces[0].tool, "echo_tool");
        assert!(traces[0].result_json.contains("expression"));
    }

    #[tokio::test]
    async fn oneshot_without_tool_use_returns_text_and_no_traces() {
        struct PlainProvider;

        #[async_trait]
        impl Provider for PlainProvider {
            async fn complete(
                &self,
                _messages: Vec<Message>,
                _tools: Option<Vec<ProviderTool>>,
                _max_tokens: Option<u32>,
                _system_prompt: Option<String>,
            ) -> Result<CompletionResponse> {
                Ok(CompletionResponse {
                    text: Some("42".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                })
            }
        }

        let (text, traces) = run_oneshot(Box::new(PlainProvider), ToolRegistry::new(), None, "hi")
            .await
            .expect("oneshot run");
        assert_eq!(text, "42");
        assert!(traces.is_empty());
    }

    // --- prompt resolution --------------------------------------------------

    fn temp_md(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("axoner-oneshot-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn existing_md_path_resolves_to_skill_content_with_single_prefix_line() {
        let path = temp_md("SKILL.md");
        std::fs::write(&path, "Step 1: do the thing.\nStep 2: report.").unwrap();

        let prompt = resolve_prompt(path.to_str().unwrap());
        assert_eq!(
            prompt,
            "Follow this skill exactly.\nStep 1: do the thing.\nStep 2: report."
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_path_is_treated_as_literal_prompt() {
        assert_eq!(resolve_prompt("hello world"), "hello world");
        assert_eq!(
            resolve_prompt("no/such/dir/SKILL.md"),
            "no/such/dir/SKILL.md",
            "a non-existent .md path is NOT a skill — literal prompt"
        );
    }

    #[test]
    fn existing_non_md_file_is_literal_prompt() {
        let path = temp_md("notes.txt");
        std::fs::write(&path, "not a skill").unwrap();
        let literal = path.to_str().unwrap().to_string();
        assert_eq!(resolve_prompt(&literal), literal);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
