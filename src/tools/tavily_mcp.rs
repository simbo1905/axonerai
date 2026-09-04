//! Fake Tavily MCP facade: built-in Rust tools dressed as a Tavily MCP
//! server's tools. There is NO MCP host process — they call the Tavily REST
//! API directly through the shared internals in [`crate::tools::tavily`],
//! reusing the exact request/response behaviour of WebSearch/WebFetch.

use crate::tool::{Tool, ToolSource};
use crate::tools::tavily::{DEFAULT_MAX_RESULTS, DEFAULT_TAVILY_BASE_URL, extract, search};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;

#[cfg(feature = "web")]
use tracing::debug;

pub struct TavilyMcpSearch {
    base_url: String,
}

impl TavilyMcpSearch {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_TAVILY_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for TavilyMcpSearch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct TavilyMcpSearchInput {
    query: String,
    max_results: Option<i64>,
}

#[async_trait]
impl Tool for TavilyMcpSearch {
    fn name(&self) -> String {
        "tavily_search".to_string()
    }

    fn description(&self) -> String {
        "Part of the Tavily MCP server. Search the web for current information. \
         Prefer this tool when the user explicitly asks for a Tavily search (e.g. \
         \"do a tavily search …\"). Returns a list of results with title, URL and \
         a content snippet."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (1-10, default 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Mcp
    }

    fn mcp_server(&self) -> Option<&'static str> {
        Some("tavily")
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let input: TavilyMcpSearchInput = serde_json::from_value(input)
            .map_err(|e| anyhow!("Invalid tavily_search input: {}", e))?;

        #[cfg(feature = "web")]
        debug!("tavily_search: query={}", input.query);

        let api_key = env::var("TAVILY_API_KEY")
            .map_err(|_| anyhow!("TAVILY_API_KEY environment variable is not set; the tavily_search tool requires a Tavily API key"))?;

        let max_results = input
            .max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, 10);

        search(&self.base_url, &api_key, &input.query, max_results).await
    }
}

pub struct TavilyMcpExtract {
    base_url: String,
}

impl TavilyMcpExtract {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_TAVILY_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for TavilyMcpExtract {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct TavilyMcpExtractInput {
    url: String,
    query: Option<String>,
}

#[async_trait]
impl Tool for TavilyMcpExtract {
    fn name(&self) -> String {
        "tavily_extract".to_string()
    }

    fn description(&self) -> String {
        "Part of the Tavily MCP server. Fetch the contents of a web page URL and \
         return the extracted text. Prefer this tool when the user explicitly asks \
         for a Tavily extract."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL of the web page to fetch"
                },
                "query": {
                    "type": "string",
                    "description": "Optional query to rerank and focus the extracted content"
                }
            },
            "required": ["url"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Mcp
    }

    fn mcp_server(&self) -> Option<&'static str> {
        Some("tavily")
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let input: TavilyMcpExtractInput = serde_json::from_value(input)
            .map_err(|e| anyhow!("Invalid tavily_extract input: {}", e))?;

        #[cfg(feature = "web")]
        debug!("tavily_extract: url={}", input.url);

        let api_key = env::var("TAVILY_API_KEY")
            .map_err(|_| anyhow!("TAVILY_API_KEY environment variable is not set; the tavily_extract tool requires a Tavily API key"))?;

        extract(&self.base_url, &api_key, &input.url, input.query.as_deref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::executor::ToolExecutor;
    use crate::provider::ToolCall;
    use crate::tool::ToolRegistry;
    use crate::tools::testing::{env_guard, set_key, spawn_stub, unset_key};
    use serde_json::json;

    /// item48: both facade tools report the `tavily` server so the panel's
    /// per-server toggle (POST /api/mcp) can suppress them together.
    #[test]
    fn facade_tools_report_their_server() {
        assert_eq!(TavilyMcpSearch::new().mcp_server(), Some("tavily"));
        assert_eq!(TavilyMcpExtract::new().mcp_server(), Some("tavily"));
    }

    #[tokio::test]
    async fn search_round_trips_through_stub_server() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(req.path, "/search");
            assert_eq!(
                req.authorization.as_deref(),
                Some("Bearer test-key"),
                "must send Authorization: Bearer header"
            );
            let body: Value = serde_json::from_str(&req.body).expect("JSON body");
            assert_eq!(body["query"], "rust vs go");
            assert_eq!(body["search_depth"], "basic");
            assert_eq!(body["max_results"], 3);
            (
                200,
                r#"{"results": [{"title": "Rust", "url": "https://www.rust-lang.org", "content": "A language empowering everyone."}]}"#
                    .to_string(),
            )
        })
        .await;

        let tool = TavilyMcpSearch::new().with_base_url(server.base_url());
        assert_eq!(tool.name(), "tavily_search");
        assert_eq!(tool.source(), ToolSource::Mcp);
        assert!(
            tool.description().contains("Part of the Tavily MCP server"),
            "description must mention the Tavily MCP server"
        );

        let output = tool
            .execute(json!({"query": "rust vs go", "max_results": 3}))
            .await
            .expect("facade search round-trip should succeed");
        assert!(output.contains("1. Rust"));
        assert!(output.contains("https://www.rust-lang.org"));
        unset_key();
    }

    #[tokio::test]
    async fn extract_round_trips_through_stub_server() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(req.path, "/extract");
            assert_eq!(
                req.authorization.as_deref(),
                Some("Bearer test-key"),
                "must send Authorization: Bearer header"
            );
            let body: Value = serde_json::from_str(&req.body).expect("JSON body");
            assert_eq!(body["urls"], json!(["https://example.com/page"]));
            assert_eq!(body["extract_depth"], "basic");
            (
                200,
                r#"{"results": [{"url": "https://example.com/page", "raw_content": "Rust ownership rules explained."}]}"#
                    .to_string(),
            )
        })
        .await;

        let tool = TavilyMcpExtract::new().with_base_url(server.base_url());
        assert_eq!(tool.name(), "tavily_extract");
        assert_eq!(tool.source(), ToolSource::Mcp);
        assert!(
            tool.description().contains("Part of the Tavily MCP server"),
            "description must mention the Tavily MCP server"
        );

        let output = tool
            .execute(json!({"url": "https://example.com/page"}))
            .await
            .expect("facade extract round-trip should succeed");
        assert!(output.contains("== https://example.com/page =="));
        assert!(output.contains("Rust ownership rules explained."));
        unset_key();
    }

    #[tokio::test]
    async fn suppressed_tool_is_hidden_from_llm_and_errors_on_execute() {
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(TavilyMcpSearch::new()));
        registry.register(Box::new(TavilyMcpExtract::new()));

        // Visible before suppression.
        assert_eq!(registry.get_all_for_llm().len(), 2);

        registry.set_suppressed("tavily_search", false);
        assert!(registry.is_suppressed("tavily_search"));
        assert!(!registry.is_suppressed("tavily_extract"));

        // The model never sees suppressed tools.
        let for_llm = registry.get_all_for_llm();
        assert_eq!(for_llm.len(), 1, "suppressed tool must be skipped");
        assert_eq!(for_llm[0].name, "tavily_extract");

        // Executing a suppressed tool errors clearly.
        let executor = ToolExecutor::new(&registry);
        let call = ToolCall {
            id: "call-1".to_string(),
            name: "tavily_search".to_string(),
            input: json!({"query": "test"}),
        };
        let err = executor
            .execute(&call)
            .await
            .expect_err("suppressed tool must error");
        assert!(
            err.to_string().contains("tool 'tavily_search' is disabled"),
            "unexpected error: {err}"
        );

        // Re-enabling restores visibility.
        registry.set_suppressed("tavily_search", true);
        assert!(!registry.is_suppressed("tavily_search"));
        assert_eq!(registry.get_all_for_llm().len(), 2);
    }
}
