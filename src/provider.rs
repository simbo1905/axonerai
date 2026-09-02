use anyhow::Result;
use async_trait::async_trait;
use serde::de::Deserializer;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Core trait that all LLM providers must implement
#[async_trait]
pub trait Provider: Send + Sync {
    /// Send a completion request to the LLM
    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        max_tokens: Option<u32>,
        system_prompt: Option<String>,
    ) -> Result<CompletionResponse>;
}

/// A message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String, // "user", "assistant", or "tool"
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

/// Unified response from any LLM provider
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
}

/// When the model wants to call a tool
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

impl Serialize for ToolCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        struct FunctionWire<'a> {
            name: &'a str,
            arguments: &'a str,
        }

        impl Serialize for FunctionWire<'_> {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                let mut state = serializer.serialize_struct("FunctionWire", 2)?;
                state.serialize_field("name", self.name)?;
                state.serialize_field("arguments", self.arguments)?;
                state.end()
            }
        }

        let arguments = serde_json::to_string(&self.input).unwrap_or_else(|_| "{}".to_string());
        let mut state = serializer.serialize_struct("ToolCall", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("type", "function")?;
        state.serialize_field(
            "function",
            &FunctionWire {
                name: &self.name,
                arguments: &arguments,
            },
        )?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for ToolCall {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawFunction {
            name: String,
            arguments: String,
        }

        #[derive(Deserialize)]
        struct RawToolCall {
            id: String,
            #[serde(default)]
            function: Option<RawFunction>,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            input: Option<Value>,
        }

        let raw = RawToolCall::deserialize(deserializer)?;
        if let Some(function) = raw.function {
            let input: Value = serde_json::from_str(&function.arguments).unwrap_or(Value::Null);
            return Ok(ToolCall {
                id: raw.id,
                name: function.name,
                input,
            });
        }
        Ok(ToolCall {
            id: raw.id,
            name: raw.name.unwrap_or_default(),
            input: raw.input.unwrap_or(Value::Null),
        })
    }
}

/// Why the completion stopped
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,       // Natural completion
    ToolUse,       // Wants to call tools
    MaxTokens,     // Hit token limit
    ContentFilter, // Filtered by provider
    Error,         // Something went wrong
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn assistant_message_serializes_native_tool_calls_wire_shape() {
        let msg = Message {
            role: "assistant".to_string(),
            content: String::new(),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".to_string(),
                name: "calculator".to_string(),
                input: json!({"a": 2.0, "b": 4.0, "operation": "multiply"}),
            }]),
            tool_call_id: None,
        };

        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["tool_call_id"], Value::Null);
        let call = &v["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "calculator");
        let arguments = call["function"]["arguments"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(arguments).unwrap();
        assert_eq!(parsed, json!({"a": 2.0, "b": 4.0, "operation": "multiply"}));
        assert!(call.get("name").is_none(), "internal shape must not leak");
        assert!(call.get("input").is_none(), "internal shape must not leak");
    }

    #[test]
    fn tool_result_message_serializes_tool_role_shape() {
        let msg = Message {
            role: "tool".to_string(),
            content: "8.0".to_string(),
            tool_calls: None,
            tool_call_id: Some("call_1".to_string()),
        };

        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(
            v,
            json!({"role": "tool", "content": "8.0", "tool_call_id": "call_1"})
        );
    }

    #[test]
    fn plain_message_serializes_exactly_as_before() {
        let msg = Message {
            role: "user".to_string(),
            content: "hi".to_string(),
            tool_calls: None,
            tool_call_id: None,
        };

        let v = serde_json::to_value(&msg).unwrap();
        assert_eq!(v, json!({"role": "user", "content": "hi"}));
    }

    #[test]
    fn old_plain_message_deserializes_with_tool_fields_none() {
        let msg: Message =
            serde_json::from_str(r#"{"role":"assistant","content":"hello"}"#).unwrap();
        assert_eq!(msg.role, "assistant");
        assert_eq!(msg.content, "hello");
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
    }

    #[test]
    fn wire_shaped_tool_calls_deserialize_back_to_internal_form() {
        let raw = r#"{
            "role": "assistant",
            "content": "",
            "tool_calls": [{
                "id": "call_9",
                "type": "function",
                "function": {"name": "calculator", "arguments": "{\"a\":2}"}
            }]
        }"#;
        let msg: Message = serde_json::from_str(raw).unwrap();
        let calls = msg.tool_calls.unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_9");
        assert_eq!(calls[0].name, "calculator");
        assert_eq!(calls[0].input, json!({"a": 2}));
    }

    #[test]
    fn message_round_trips_through_json_without_loss() {
        let msg = Message {
            role: "tool".to_string(),
            content: "result".to_string(),
            tool_calls: None,
            tool_call_id: Some("call_2".to_string()),
        };
        let bytes = serde_json::to_string(&msg).unwrap();
        let back: Message = serde_json::from_str(&bytes).unwrap();
        assert_eq!(back.role, "tool");
        assert_eq!(back.content, "result");
        assert_eq!(back.tool_call_id.as_deref(), Some("call_2"));
    }
}
