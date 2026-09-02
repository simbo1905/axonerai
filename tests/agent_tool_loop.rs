//! TDD bar for item35: the agent loop must persist and replay NATIVE
//! tool-call protocol turns (assistant `tool_calls` + `role:"tool"` results),
//! not the old fake narration text.
//!
//! item37f extends the bar: a FAILING tool must not abort the run — the error
//! goes back to the model as the tool result content, the loop continues, and
//! the ToolTrace reflects the real (error) outcome.

use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;
use axonerai::agent::Agent;
use axonerai::provider::{CompletionResponse, Message, Provider, StopReason, Tool, ToolCall};
use axonerai::tool::{Tool as ToolTrait, ToolRegistry};
use serde_json::{Value, json};

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

struct FailingTool;

#[async_trait]
impl ToolTrait for FailingTool {
    fn name(&self) -> String {
        "failing_tool".to_string()
    }

    fn description(&self) -> String {
        "always fails".to_string()
    }

    fn input_schema(&self) -> Value {
        json!({"type": "object"})
    }

    async fn execute(&self, _input: Value) -> Result<String> {
        Err(anyhow::anyhow!("division by zero: cannot divide 5 by 0"))
    }
}

#[derive(Clone, Default)]
struct SharedRequests(Arc<Mutex<Vec<Vec<Message>>>>);

impl SharedRequests {
    fn push(&self, messages: Vec<Message>) {
        self.0.lock().unwrap().push(messages);
    }

    fn len(&self) -> usize {
        self.0.lock().unwrap().len()
    }

    fn request(&self, n: usize) -> Vec<Message> {
        self.0.lock().unwrap()[n].clone()
    }
}

struct MockProvider {
    requests: SharedRequests,
}

#[async_trait]
impl Provider for MockProvider {
    async fn complete(
        &self,
        messages: Vec<Message>,
        _tools: Option<Vec<Tool>>,
        _max_tokens: Option<u32>,
        _system_prompt: Option<String>,
    ) -> Result<CompletionResponse> {
        self.requests.push(messages);
        if self.requests.len() == 1 {
            Ok(CompletionResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: "call_abc123".to_string(),
                    name: "echo_tool".to_string(),
                    input: json!({"expression": "2^4"}),
                }],
                stop_reason: StopReason::ToolUse,
            })
        } else {
            Ok(CompletionResponse {
                text: Some("16".to_string()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
            })
        }
    }
}

#[tokio::test]
async fn second_request_carries_native_tool_calls_not_fake_text() {
    let requests = SharedRequests::default();
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(EchoTool));
    let agent = Agent::new(
        Box::new(MockProvider {
            requests: requests.clone(),
        }),
        registry,
        None,
        None,
    );

    let answer = agent.run("what is 2^4").await.expect("agent run");
    assert_eq!(answer, "16");
    assert_eq!(requests.len(), 2, "one ToolUse round + one EndTurn round");

    let second = requests.request(1);
    assert_eq!(
        second.len(),
        3,
        "user + assistant(tool_calls) + tool result"
    );

    assert_eq!(second[0].role, "user");
    assert_eq!(second[0].content, "what is 2^4");
    assert!(second[0].tool_calls.is_none());
    assert!(second[0].tool_call_id.is_none());

    let assistant = &second[1];
    assert_eq!(assistant.role, "assistant");
    assert!(
        !assistant.content.contains("Using tool"),
        "fake narration text must be gone, got: {:?}",
        assistant.content
    );
    let tool_calls = assistant
        .tool_calls
        .as_ref()
        .expect("assistant turn must carry NATIVE tool_calls");
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0].id, "call_abc123");
    assert_eq!(tool_calls[0].name, "echo_tool");
    assert_eq!(tool_calls[0].input, json!({"expression": "2^4"}));
    assert!(assistant.tool_call_id.is_none());

    let tool_msg = &second[2];
    assert_eq!(tool_msg.role, "tool");
    assert_eq!(
        tool_msg.tool_call_id.as_deref(),
        Some("call_abc123"),
        "tool result must reference the tool_call id"
    );
    assert_eq!(tool_msg.content, r#"{"expression":"2^4"}"#);
    assert!(tool_msg.tool_calls.is_none());
}

/// Provider that replays a fixed script of responses in order; the last
/// response repeats once the script is exhausted. Records every request.
struct ScriptedProvider {
    requests: SharedRequests,
    script: Vec<CompletionResponse>,
}

#[async_trait]
impl Provider for ScriptedProvider {
    async fn complete(
        &self,
        messages: Vec<Message>,
        _tools: Option<Vec<Tool>>,
        _max_tokens: Option<u32>,
        _system_prompt: Option<String>,
    ) -> Result<CompletionResponse> {
        self.requests.push(messages);
        let idx = (self.requests.len() - 1).min(self.script.len() - 1);
        Ok(self.script[idx].clone())
    }
}

fn tool_response(calls: Vec<ToolCall>) -> CompletionResponse {
    CompletionResponse {
        text: None,
        tool_calls: calls,
        stop_reason: StopReason::ToolUse,
    }
}

fn end_turn(text: &str) -> CompletionResponse {
    CompletionResponse {
        text: Some(text.to_string()),
        tool_calls: vec![],
        stop_reason: StopReason::EndTurn,
    }
}

fn failing_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(FailingTool));
    registry
}

// item37f RED: a failing tool must NOT abort the run — the error text goes
// back to the model as the tool result content and the loop completes.
#[tokio::test]
async fn failing_tool_becomes_tool_message_not_run_abort() {
    let requests = SharedRequests::default();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let agent = Agent::new(
        Box::new(ScriptedProvider {
            requests: requests.clone(),
            script: vec![
                tool_response(vec![ToolCall {
                    id: "call_fail1".to_string(),
                    name: "failing_tool".to_string(),
                    input: json!({"a": 5, "b": 0}),
                }]),
                end_turn("5 divided by 0 is undefined — the tool errored."),
            ],
        }),
        failing_registry(),
        None,
        None,
    );

    let answer = agent
        .run_with_traces("divide 5 by 0", tx)
        .await
        .expect("failing tool must NOT abort the run");
    assert_eq!(answer, "5 divided by 0 is undefined — the tool errored.");
    assert_eq!(requests.len(), 2, "one ToolUse round + one EndTurn round");

    let second = requests.request(1);
    assert_eq!(
        second.len(),
        3,
        "user + assistant(tool_calls) + tool result (error)"
    );
    let tool_msg = &second[2];
    assert_eq!(tool_msg.role, "tool");
    assert_eq!(tool_msg.tool_call_id.as_deref(), Some("call_fail1"));
    assert!(
        tool_msg.content.contains("error:"),
        "tool result content must carry the error marker, got: {:?}",
        tool_msg.content
    );
    assert!(
        tool_msg
            .content
            .contains("division by zero: cannot divide 5 by 0"),
        "tool result content must carry the error text, got: {:?}",
        tool_msg.content
    );

    // The trace must reflect the real outcome: error text as result_json.
    let trace = rx.recv().await.expect("one ToolTrace per tool execution");
    assert_eq!(trace.tool, "failing_tool");
    assert!(
        trace
            .result_json
            .contains("division by zero: cannot divide 5 by 0"),
        "trace result_json must contain the error text, got: {:?}",
        trace.result_json
    );
}

// item37f: multiple tool calls with one failing + one succeeding → BOTH tool
// messages present (in call order), run completes.
#[tokio::test]
async fn one_failing_one_succeeding_call_both_reported() {
    let requests = SharedRequests::default();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut registry = failing_registry();
    registry.register(Box::new(EchoTool));
    let agent = Agent::new(
        Box::new(ScriptedProvider {
            requests: requests.clone(),
            script: vec![
                tool_response(vec![
                    ToolCall {
                        id: "call_bad".to_string(),
                        name: "failing_tool".to_string(),
                        input: json!({"a": 5, "b": 0}),
                    },
                    ToolCall {
                        id: "call_ok".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({"expression": "2^4"}),
                    },
                ]),
                end_turn("one failed, one worked"),
            ],
        }),
        registry,
        None,
        None,
    );

    let answer = agent
        .run_with_traces("divide 5 by 0 and echo 2^4", tx)
        .await
        .expect("mixed success/failure must NOT abort the run");
    assert_eq!(answer, "one failed, one worked");

    let second = requests.request(1);
    assert_eq!(
        second.len(),
        4,
        "user + assistant(tool_calls) + two tool results"
    );
    assert_eq!(second[2].role, "tool");
    assert_eq!(second[2].tool_call_id.as_deref(), Some("call_bad"));
    assert!(second[2].content.contains("division by zero"));
    assert_eq!(second[3].role, "tool");
    assert_eq!(second[3].tool_call_id.as_deref(), Some("call_ok"));
    assert_eq!(second[3].content, r#"{"expression":"2^4"}"#);

    let mut traces = Vec::new();
    while let Ok(t) = rx.try_recv() {
        traces.push(t);
    }
    assert_eq!(traces.len(), 2, "one trace per call");
    assert_eq!(traces[0].tool, "failing_tool");
    assert!(traces[0].result_json.contains("division by zero"));
    assert_eq!(traces[1].tool, "echo_tool");
    assert!(traces[1].result_json.contains("expression"));
}
