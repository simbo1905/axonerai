//! TDD bar for item35: the agent loop must persist and replay NATIVE
//! tool-call protocol turns (assistant `tool_calls` + `role:"tool"` results),
//! not the old fake narration text.

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
