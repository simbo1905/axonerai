use crate::tool::Tool;
use crate::tools::tavily::{self, DEFAULT_MAX_RESULTS, DEFAULT_TAVILY_BASE_URL};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;

#[cfg(feature = "web")]
use tracing::debug;

pub struct WebSearch {
    base_url: String,
}

impl WebSearch {
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

impl Default for WebSearch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct WebSearchInput {
    query: String,
    max_results: Option<i64>,
}

#[async_trait]
impl Tool for WebSearch {
    fn name(&self) -> String {
        "WebSearch".to_string()
    }

    fn description(&self) -> String {
        "Search the web for current information. Returns a list of results with title, URL and a content snippet."
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

    async fn execute(&self, input: Value) -> Result<String> {
        let input: WebSearchInput =
            serde_json::from_value(input).map_err(|e| anyhow!("Invalid WebSearch input: {}", e))?;

        #[cfg(feature = "web")]
        debug!("WebSearch: query={}", input.query);

        let api_key = env::var("TAVILY_API_KEY")
            .map_err(|_| anyhow!("TAVILY_API_KEY environment variable is not set; the WebSearch tool requires a Tavily API key"))?;

        let max_results = input
            .max_results
            .unwrap_or(DEFAULT_MAX_RESULTS)
            .clamp(1, 10);

        tavily::search(&self.base_url, &api_key, &input.query, max_results).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::testing::{env_guard, set_key, spawn_stub, unset_key};
    use serde_json::json;

    const CANNED_SEARCH_RESPONSE: &str = r#"{
        "results": [
            {"title": "Rust programming language", "url": "https://www.rust-lang.org", "content": "A language empowering everyone to build reliable and efficient software."},
            {"title": "Tavily API docs", "url": "https://docs.tavily.com", "content": "Tavily is a search API built for AI agents."}
        ]
    }"#;

    #[tokio::test]
    async fn happy_path_posts_bearer_auth_and_formats_results() {
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
            assert_eq!(body["max_results"], 5);
            (200, CANNED_SEARCH_RESPONSE.to_string())
        })
        .await;

        let tool = WebSearch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"query": "rust vs go"}))
            .await
            .expect("happy path should succeed");

        assert!(output.contains("1. Rust programming language"));
        assert!(output.contains("https://www.rust-lang.org"));
        assert!(output.contains("A language empowering everyone"));
        assert!(output.contains("2. Tavily API docs"));
        assert!(output.contains("https://docs.tavily.com"));
        unset_key();
    }

    #[tokio::test]
    async fn max_results_is_clamped_to_valid_range() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|req| {
            let body: Value = serde_json::from_str(&req.body).expect("JSON body");
            assert_eq!(body["max_results"], 10, "0 should clamp to 1, 99 to 10");
            (200, CANNED_SEARCH_RESPONSE.to_string())
        })
        .await;

        let tool = WebSearch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"query": "test", "max_results": 99}))
            .await
            .expect("clamped request should succeed");
        assert!(output.contains("Rust programming language"));
        unset_key();
    }

    #[tokio::test]
    async fn missing_api_key_returns_clear_error() {
        let _guard = env_guard();
        unset_key();

        let tool = WebSearch::new();
        let err = tool
            .execute(json!({"query": "anything"}))
            .await
            .expect_err("missing key must error");
        assert!(
            err.to_string().contains("TAVILY_API_KEY"),
            "error should mention TAVILY_API_KEY, got: {err}"
        );
    }

    #[tokio::test]
    async fn non_200_response_returns_err_with_status() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|_req| (401, r#"{"detail": "Unauthorized"}"#.to_string())).await;

        let tool = WebSearch::new().with_base_url(server.base_url());
        let err = tool
            .execute(json!({"query": "test"}))
            .await
            .expect_err("401 must be an error");
        assert!(err.to_string().contains("401"), "got: {err}");
        unset_key();
    }

    #[tokio::test]
    async fn empty_results_reports_no_results() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|_req| (200, r#"{"results": []}"#.to_string())).await;

        let tool = WebSearch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"query": "obscure query"}))
            .await
            .expect("empty results is not an error");
        assert_eq!(output, "No results found for: obscure query");
        unset_key();
    }
}
