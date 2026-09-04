use crate::provider::ToolCall;
use crate::tool::ToolRegistry;
use anyhow::{Result, anyhow};

/// Default per-run tool-round budget (item54): how many model responses that
/// request tool use may be executed before the run stops with an honest
/// final message. The loop's `max_iterations` caps TOTAL provider calls;
/// this budget bounds over-FETCHING tool rounds specifically.
pub const DEFAULT_TOOL_ROUND_BUDGET: usize = 25;

/// Executes tool calls and returns results
pub struct ToolExecutor<'a> {
    registry: &'a ToolRegistry,
    /// When set, the per-call stdout chatter is suppressed (the agent's
    /// quiet mode; see [`crate::agent::Agent::set_quiet`]).
    quiet: bool,
    /// item54: max tool-use rounds per run (see
    /// [`DEFAULT_TOOL_ROUND_BUDGET`]).
    max_rounds: usize,
    /// Tool-use rounds executed so far in this run.
    rounds: usize,
}

impl<'a> ToolExecutor<'a> {
    pub fn new(registry: &'a ToolRegistry) -> Self {
        Self {
            registry,
            quiet: false,
            max_rounds: DEFAULT_TOOL_ROUND_BUDGET,
            rounds: 0,
        }
    }

    /// Suppress the per-call stdout chatter (see [`ToolExecutor::quiet`]).
    pub fn set_quiet(&mut self, quiet: bool) {
        self.quiet = quiet;
    }

    /// Set the per-run tool-round budget (item54).
    pub fn set_max_rounds(&mut self, max_rounds: usize) {
        self.max_rounds = max_rounds.max(1);
    }

    /// The configured tool-round budget.
    pub fn max_rounds(&self) -> usize {
        self.max_rounds
    }

    /// Tool-use rounds executed so far in this run.
    pub fn rounds(&self) -> usize {
        self.rounds
    }

    /// Whether another tool round may execute under the budget.
    pub fn round_available(&self) -> bool {
        self.rounds < self.max_rounds
    }

    /// Count one tool-use round (the agent calls this once per provider
    /// response that requests tool execution, BEFORE executing the batch).
    pub fn begin_round(&mut self) {
        self.rounds += 1;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ToolCall;
    use serde_json::{Value, json};

    struct DummyTool;

    #[async_trait::async_trait]
    impl crate::tool::Tool for DummyTool {
        fn name(&self) -> String {
            "dummy".to_string()
        }

        fn description(&self) -> String {
            "dummy".to_string()
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, _input: Value) -> Result<String> {
            Ok("ok".to_string())
        }
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "call_1".to_string(),
            name: "dummy".to_string(),
            input: json!({}),
        }
    }

    #[test]
    fn default_budget_is_item54_default() {
        let registry = ToolRegistry::new();
        let executor = ToolExecutor::new(&registry);
        assert_eq!(executor.max_rounds(), 25);
        assert_eq!(executor.rounds(), 0);
        assert!(executor.round_available(), "a fresh run has budget left");
    }

    #[test]
    fn begin_round_consumes_budget_until_exhausted() {
        let registry = ToolRegistry::new();
        let mut executor = ToolExecutor::new(&registry);
        executor.set_max_rounds(3);
        assert_eq!(executor.max_rounds(), 3, "the configured budget wins");
        for _ in 0..3 {
            assert!(executor.round_available());
            executor.begin_round();
        }
        assert_eq!(executor.rounds(), 3);
        assert!(
            !executor.round_available(),
            "the budget is exhausted after N rounds"
        );
    }

    #[test]
    fn set_max_rounds_never_yields_zero() {
        let registry = ToolRegistry::new();
        let mut executor = ToolExecutor::new(&registry);
        executor.set_max_rounds(0);
        assert_eq!(executor.max_rounds(), 1, "a budget of 0 clamps to 1");
        executor.begin_round();
        assert!(!executor.round_available());
    }

    #[tokio::test]
    async fn execute_still_works_across_rounds() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DummyTool));
        let mut executor = ToolExecutor::new(&registry);
        executor.begin_round();
        let result = executor.execute_captured(&call()).await;
        assert_eq!(result.tool_name, "dummy");
        assert!(!result.is_error);
    }
}
