use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::UnboundedSender;

use crate::executor::ToolExecutor;
use crate::file_session_manager::FileSessionManager;
use crate::provider::{Message, Provider, StopReason};
use crate::session::Session;
use crate::tool::ToolRegistry;
use anyhow::Result;

/// Full-fidelity trace of a single tool execution, emitted on the
/// `run_with_traces` channel. The consumer is responsible for abridging
/// before egress — these fields are always untruncated.
pub struct ToolTrace {
    pub tool: String,
    pub args_json: String,
    pub result_json: String,
    pub bytes_up: usize,
    pub bytes_down: usize,
    pub duration_ms: u64,
    pub ts: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub struct Agent {
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    max_iterations: usize,
    system_prompt: Option<String>,
    file_session_manager: Option<FileSessionManager>,
}

impl Agent {
    pub fn new(
        provider: Box<dyn Provider>,
        registry: ToolRegistry,
        system_prompt: Option<String>,
        file_session_manager: Option<FileSessionManager>,
    ) -> Self {
        Self {
            provider,
            registry,
            max_iterations: 10, // Prevent infinite loops
            system_prompt,
            file_session_manager,
        }
    }

    /// Run the agent with a user prompt (no tool-trace reporting).
    pub async fn run(&self, user_prompt: &str) -> Result<String> {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        self.run_with_traces(user_prompt, tx).await
    }

    /// Run the agent with a user prompt, streaming one [`ToolTrace`] per tool
    /// execution on `tx`. Send failures (e.g. a dropped receiver) are ignored.
    pub async fn run_with_traces(
        &self,
        user_prompt: &str,
        tx: UnboundedSender<ToolTrace>,
    ) -> Result<String> {
        let mut session = if let Some(ref sm) = self.file_session_manager {
            if sm.exists() {
                sm.load()?
            } else {
                Session::new(sm.get_session().to_string())
            }
        } else {
            Session::new("stateless".to_string())
        };

        println!();

        session.add_message(Message {
            role: "user".to_string(),
            content: user_prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let executor = ToolExecutor::new(&self.registry);
        let tools = self.registry.get_all_for_llm();

        for _iteration in 1..=self.max_iterations {
            let response = self
                .provider
                .complete(
                    session.get_messages().clone(),
                    Some(tools.clone()),
                    None,
                    self.system_prompt.clone(),
                )
                .await?;

            match response.stop_reason {
                StopReason::EndTurn => {
                    if let Some(text) = response.text {
                        session.add_message(Message {
                            role: "assistant".to_string(),
                            content: text.clone(),
                            tool_calls: None,
                            tool_call_id: None,
                        });

                        if let Some(ref sm) = self.file_session_manager {
                            sm.save(&session)?;
                        }

                        println!("Response from Agent:");
                        return Ok(text);
                    } else {
                        return Ok("(No response from agent)".to_string());
                    }
                }

                StopReason::ToolUse => {
                    if let Some(text) = &response.text {
                        println!("💭 Agent thinking: {}", text);
                    }

                    if response.tool_calls.is_empty() {
                        return Ok("Agent wanted to use tools but didn't specify any".to_string());
                    }

                    // Execute the tools one call at a time, emitting a
                    // full-fidelity trace per call (per-call timing required;
                    // batch timing is not acceptable). Per-call failures are
                    // captured as error results fed back to the model — a bad
                    // tool call must not abort the run.
                    let mut tool_results = Vec::with_capacity(response.tool_calls.len());
                    for call in &response.tool_calls {
                        let args_pretty =
                            serde_json::to_string_pretty(&call.input).unwrap_or_default();
                        let started = Instant::now();
                        let result = executor.execute_captured(call).await;
                        let duration_ms = started.elapsed().as_millis() as u64;
                        let result_pretty =
                            serde_json::to_string_pretty(&result.result).unwrap_or_default();
                        let _ = tx.send(ToolTrace {
                            tool: call.name.clone(),
                            args_json: args_pretty.clone(),
                            result_json: result_pretty.clone(),
                            bytes_up: args_pretty.len(),
                            bytes_down: result_pretty.len(),
                            duration_ms,
                            ts: now_ms(),
                        });
                        tool_results.push(result);
                    }

                    session.add_message(Message {
                        role: "assistant".to_string(),
                        content: response.text.unwrap_or_default(),
                        tool_calls: Some(response.tool_calls),
                        tool_call_id: None,
                    });

                    for result in &tool_results {
                        session.add_message(Message {
                            role: "tool".to_string(),
                            content: result.result.clone(),
                            tool_calls: None,
                            tool_call_id: Some(result.tool_call_id.clone()),
                        });
                    }

                    println!();
                    // Continue the loop
                }

                StopReason::MaxTokens => {
                    return Ok("Agent hit max tokens limit".to_string());
                }

                _ => {
                    return Ok(format!(
                        "Agent stopped with reason: {:?}",
                        response.stop_reason
                    ));
                }
            }
        }

        if let Some(ref sm) = self.file_session_manager {
            sm.save(&session)?;
        }
        Ok(format!(
            "Agent reached max iterations ({})",
            self.max_iterations
        ))
    }
}
