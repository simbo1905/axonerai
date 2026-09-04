use std::time::{Instant, SystemTime, UNIX_EPOCH};

use tokio::sync::mpsc::UnboundedSender;

use crate::executor::{DEFAULT_TOOL_ROUND_BUDGET, ToolExecutor};
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
    /// When set, the loop's `println!` chatter ("💭 Agent thinking:",
    /// "Response from Agent:", blank lines) is suppressed so programmatic
    /// callers (e.g. the `--oneshot` CLI mode) keep stdout for the final
    /// text alone. Default `false` — interactive runs chatter as before.
    quiet: bool,
    /// item54: per-run tool-round budget (see
    /// [`DEFAULT_TOOL_ROUND_BUDGET`]). When exceeded the loop ends with an
    /// honest final message instead of silently hitting `max_iterations`.
    tool_round_budget: usize,
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
            quiet: false,
            tool_round_budget: DEFAULT_TOOL_ROUND_BUDGET,
        }
    }

    /// Suppress the loop's stdout chatter (see [`Agent::quiet`]).
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Set the per-run tool-round budget (item54). Minimum 1.
    pub fn set_tool_round_budget(&mut self, budget: usize) {
        self.tool_round_budget = budget.max(1);
    }

    /// The session id this agent persists against (the
    /// `FileSessionManager`'s id), or `None` when stateless.
    pub fn session_id(&self) -> Option<&str> {
        self.file_session_manager
            .as_ref()
            .map(|sm| sm.get_session())
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

        if !self.quiet {
            println!();
        }

        session.add_message(Message {
            role: "user".to_string(),
            content: user_prompt.to_string(),
            tool_calls: None,
            tool_call_id: None,
        });

        let mut executor = ToolExecutor::new(&self.registry);
        executor.set_quiet(self.quiet);
        executor.set_max_rounds(self.tool_round_budget);
        let tools = self.registry.get_all_for_llm();

        for _iteration in 1..=self.max_iterations {
            // item54: an honest budget stop BEFORE burning another provider
            // call — the previous tool round exhausted the per-run tool-round
            // budget (deepresearch showed over-FETCHING capping the loop at
            // `max_iterations` instead). The session (with every executed
            // round so far) is persisted first.
            if !executor.round_available() {
                if let Some(ref sm) = self.file_session_manager {
                    sm.save(&session)?;
                }
                return Ok(format!(
                    "stopped: tool round budget {} reached",
                    executor.max_rounds()
                ));
            }

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

                        if !self.quiet {
                            println!("Response from Agent:");
                        }
                        return Ok(text);
                    } else {
                        return Ok("(No response from agent)".to_string());
                    }
                }

                StopReason::ToolUse => {
                    // Count this tool-use round against the item54 budget
                    // before executing the batch.
                    executor.begin_round();

                    if let Some(text) = &response.text {
                        if !self.quiet {
                            println!("💭 Agent thinking: {}", text);
                        }
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

                    if !self.quiet {
                        println!();
                    }
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
