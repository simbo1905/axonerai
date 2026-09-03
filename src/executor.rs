use crate::provider::ToolCall;
use crate::tool::ToolRegistry;
use anyhow::{Result, anyhow};

/// Executes tool calls and returns results
pub struct ToolExecutor<'a> {
    registry: &'a ToolRegistry,
    /// When set, the per-call stdout chatter is suppressed (the agent's
    /// quiet mode; see [`crate::agent::Agent::set_quiet`]).
    quiet: bool,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(registry: &'a ToolRegistry) -> Self {
        Self {
            registry,
            quiet: false,
        }
    }

    /// Suppress the per-call stdout chatter (see [`ToolExecutor::quiet`]).
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Execute a single tool call
    pub async fn execute(&self, tool_call: &ToolCall) -> Result<ToolResult> {
        if !self.quiet {
            println!("  🔧 Executing tool: {}", tool_call.name);
        }

        let tool = self
            .registry
            .get(&tool_call.name)
            .ok_or_else(|| anyhow!("Tool not found: {}", tool_call.name))?;

        if self.registry.is_suppressed(&tool_call.name) {
            return Err(anyhow!("tool '{}' is disabled", tool_call.name));
        }

        let result = tool.execute(tool_call.input.clone()).await?;

        Ok(ToolResult {
            tool_call_id: tool_call.id.clone(),
            tool_name: tool_call.name.clone(),
            result,
            is_error: false,
        })
    }

    /// Execute a single tool call, capturing failures as an error
    /// [`ToolResult`] instead of propagating the Err. Agent-loop semantics:
    /// tool executions ALWAYS produce a tool result — errors go back to the
    /// model as the result content so it can retry or explain.
    pub async fn execute_captured(&self, tool_call: &ToolCall) -> ToolResult {
        match self.execute(tool_call).await {
            Ok(result) => result,
            Err(e) => ToolResult {
                tool_call_id: tool_call.id.clone(),
                tool_name: tool_call.name.clone(),
                result: format!("error: {e}"),
                is_error: true,
            },
        }
    }

    /// Execute multiple tool calls, capturing per-call failures as error
    /// results (one bad call must not abort the batch).
    pub async fn execute_all(&self, tool_calls: &[ToolCall]) -> Vec<ToolResult> {
        let mut results = Vec::new();
        for call in tool_calls {
            results.push(self.execute_captured(call).await);
        }
        results
    }
}

/// Result from executing a tool. `is_error` marks a failed execution whose
/// `result` carries the error text; it is agent-internal state and is NOT
/// sent to the LLM (the tool message content already carries the error).
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub result: String,
    pub is_error: bool,
}
