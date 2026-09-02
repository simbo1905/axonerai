use crate::tool::Tool;
use crate::tools::tavily::{self, DEFAULT_TAVILY_BASE_URL};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;

#[cfg(feature = "web")]
use tracing::debug;

pub struct WebFetch {
    base_url: String,
}

impl WebFetch {
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

impl Default for WebFetch {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct WebFetchInput {
    url: String,
    query: Option<String>,
}

#[async_trait]
impl Tool for WebFetch {
    fn name(&self) -> String {
        "WebFetch".to_string()
    }

    fn description(&self) -> String {
        "Fetch the contents of a web page URL and return the extracted text.".to_string()
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

    async fn execute(&self, input: Value) -> Result<String> {
        let input: WebFetchInput =
            serde_json::from_value(input).map_err(|e| anyhow!("Invalid WebFetch input: {}", e))?;

        #[cfg(feature = "web")]
        debug!("WebFetch: url={}", input.url);

        let api_key = env::var("TAVILY_API_KEY")
            .map_err(|_| anyhow!("TAVILY_API_KEY environment variable is not set; the WebFetch tool requires a Tavily API key"))?;

        tavily::extract(&self.base_url, &api_key, &input.url, input.query.as_deref()).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tavily::{MAX_OUTPUT_CHARS, TRUNCATION_MARKER};
    use crate::tools::testing::{env_guard, set_key, spawn_stub, unset_key};
    use serde_json::json;

    #[tokio::test]
    async fn happy_path_posts_bearer_auth_and_concatenates_content() {
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
            assert_eq!(body["query"], "rust ownership");
            (
                200,
                r#"{
                    "results": [
                        {"url": "https://example.com/page", "raw_content": "Rust ownership rules explained."}
                    ],
                    "failed_results": []
                }"#
                .to_string(),
            )
        })
        .await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"url": "https://example.com/page", "query": "rust ownership"}))
            .await
            .expect("happy path should succeed");

        assert!(output.contains("== https://example.com/page =="));
        assert!(output.contains("Rust ownership rules explained."));
        assert!(!output.contains("Failed to fetch"));
        unset_key();
    }

    #[tokio::test]
    async fn omits_query_field_when_not_provided() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|req| {
            let body: Value = serde_json::from_str(&req.body).expect("JSON body");
            assert!(body.get("query").is_none(), "query must be omitted");
            (
                200,
                r#"{"results": [{"url": "https://example.com", "raw_content": "hello"}]}"#
                    .to_string(),
            )
        })
        .await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"url": "https://example.com"}))
            .await
            .expect("happy path should succeed");
        assert!(output.contains("hello"));
        unset_key();
    }

    #[tokio::test]
    async fn long_content_is_truncated_at_8000_chars() {
        let _guard = env_guard();
        set_key("test-key");
        let long_content = "x".repeat(20000);
        let canned = json!({
            "results": [{"url": "https://example.com/long", "raw_content": long_content}]
        })
        .to_string();
        let server = spawn_stub(move |_req| (200, canned.clone())).await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"url": "https://example.com/long"}))
            .await
            .expect("truncation path should succeed");

        assert!(
            output.contains(TRUNCATION_MARKER),
            "output must end with truncation marker"
        );
        assert!(
            output.chars().count() <= MAX_OUTPUT_CHARS,
            "output must be capped at 8000 chars, got {}",
            output.chars().count()
        );
        unset_key();
    }

    #[tokio::test]
    async fn failed_results_urls_are_reported() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|_req| {
            (
                200,
                r#"{
                    "results": [
                        {"url": "https://example.com/ok", "raw_content": "fine"}
                    ],
                    "failed_results": [
                        {"url": "https://broken.example.com", "error": "timeout"}
                    ]
                }"#
                .to_string(),
            )
        })
        .await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"url": "https://broken.example.com"}))
            .await
            .expect("failed_results are reported, not an error");

        assert!(output.contains("== https://example.com/ok =="));
        assert!(output.contains("Failed to fetch: https://broken.example.com"));
        unset_key();
    }

    #[tokio::test]
    async fn empty_raw_content_is_noted() {
        let _guard = env_guard();
        set_key("test-key");
        let server = spawn_stub(|_req| {
            (
                200,
                r#"{"results": [{"url": "https://example.com/empty", "raw_content": ""}]}"#
                    .to_string(),
            )
        })
        .await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"url": "https://example.com/empty"}))
            .await
            .expect("empty content is not an error");
        assert!(output.contains("(no content extracted)"));
        unset_key();
    }

    #[tokio::test]
    async fn missing_api_key_returns_clear_error() {
        let _guard = env_guard();
        unset_key();

        let tool = WebFetch::new();
        let err = tool
            .execute(json!({"url": "https://example.com"}))
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
        let server = spawn_stub(|_req| (500, "internal error".to_string())).await;

        let tool = WebFetch::new().with_base_url(server.base_url());
        let err = tool
            .execute(json!({"url": "https://example.com"}))
            .await
            .expect_err("500 must be an error");
        assert!(err.to_string().contains("500"), "got: {err}");
        unset_key();
    }
}
