use crate::provider::{CompletionResponse, Message, Provider, StopReason, Tool, ToolCall};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Generic OpenAI-compatible provider used for OpenCode Zen and OpenCode Go.
/// The endpoint, API key, and model are all configurable so the same code
/// serves both the Zen and Go endpoints (or any other OpenAI-compatible endpoint).
pub struct OpenCodeProvider {
    api_key: String,
    model: String,
    endpoint: String,
    client: reqwest::Client,
}

impl OpenCodeProvider {
    pub fn new(api_key: String, endpoint: String, model: String) -> Self {
        Self {
            api_key,
            model,
            endpoint,
            client: reqwest::Client::new(),
        }
    }

    pub fn with_model(mut self, model: String) -> Self {
        self.model = model;
        self
    }
}

#[async_trait]
impl Provider for OpenCodeProvider {
    async fn complete(
        &self,
        messages: Vec<Message>,
        tools: Option<Vec<Tool>>,
        max_tokens: Option<u32>,
        system_prompt: Option<String>,
    ) -> Result<CompletionResponse> {
        let mut body = json!({
            "model": self.model,
            "messages": messages,
        });

        if let Some(sys_prompt) = system_prompt {
            // Prepend system message
            if let Some(arr) = body["messages"].as_array_mut() {
                arr.insert(
                    0,
                    json!({
                        "role": "system",
                        "content": sys_prompt
                    }),
                );
            }
        }

        if let Some(max_tokens) = max_tokens {
            body["max_tokens"] = json!(max_tokens);
        }

        // Add tools if provided (OpenAI-compatible format)
        if let Some(tools) = tools {
            let tool_defs: Vec<Value> = tools
                .iter()
                .map(|t| {
                    json!({
                        "type": "function",
                        "function": {
                            "name": t.name,
                            "description": t.description,
                            "parameters": t.input_schema,
                        }
                    })
                })
                .collect();
            body["tools"] = json!(tool_defs);
        }

        let response = self
            .client
            .post(&self.endpoint)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .header("Content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await?;
            return Err(anyhow!("OpenCode API error {}: {}", status, error_text));
        }

        let api_response: OpenCodeResponse = response.json().await?;

        let choice = api_response
            .choices
            .first()
            .ok_or_else(|| anyhow!("No choices in OpenCode response"))?;

        let text = choice.message.content.clone();

        let tool_calls = if let Some(calls) = &choice.message.tool_calls {
            calls
                .iter()
                .map(|tc| {
                    let input: Value =
                        serde_json::from_str(&tc.function.arguments).unwrap_or(json!({}));
                    ToolCall {
                        id: tc.id.clone(),
                        name: tc.function.name.clone(),
                        input,
                    }
                })
                .collect::<Vec<ToolCall>>()
        } else {
            vec![]
        };

        let stop_reason = match choice.finish_reason.as_str() {
            "tool_calls" => StopReason::ToolUse,
            "stop" => StopReason::EndTurn,
            "length" => StopReason::MaxTokens,
            "content_filter" => StopReason::ContentFilter,
            _ => StopReason::Error,
        };

        Ok(CompletionResponse {
            text,
            tool_calls,
            stop_reason,
        })
    }
}

// OpenCode API response structures (OpenAI-compatible)
#[derive(Debug, Deserialize, Serialize)]
struct OpenCodeResponse {
    choices: Vec<Choice>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Choice {
    message: ResponseMessage,
    finish_reason: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct ResponseMessage {
    content: Option<String>,
    tool_calls: Option<Vec<OpenCodeToolCall>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct OpenCodeToolCall {
    id: String,
    function: FunctionCall,
}

#[derive(Debug, Deserialize, Serialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Message;

    #[test]
    fn extended_messages_serialize_to_openai_compatible_wire_shape() {
        let messages = vec![
            Message {
                role: "assistant".to_string(),
                content: String::new(),
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".to_string(),
                    name: "calculator".to_string(),
                    input: json!({"operation": "multiply", "a": 2.0, "b": 4.0}),
                }]),
                tool_call_id: None,
            },
            Message {
                role: "tool".to_string(),
                content: "8.0".to_string(),
                tool_calls: None,
                tool_call_id: Some("call_1".to_string()),
            },
        ];

        let body = json!({"model": "test-model", "messages": messages});
        let wire = &body["messages"];

        let assistant = &wire[0];
        assert_eq!(assistant["role"], "assistant");
        let call = &assistant["tool_calls"][0];
        assert_eq!(call["id"], "call_1");
        assert_eq!(call["type"], "function");
        assert_eq!(call["function"]["name"], "calculator");
        let arguments: Value =
            serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(
            arguments,
            json!({"operation": "multiply", "a": 2.0, "b": 4.0})
        );

        let tool = &wire[1];
        assert_eq!(tool["role"], "tool");
        assert_eq!(tool["tool_call_id"], "call_1");
        assert_eq!(tool["content"], "8.0");
    }
}
