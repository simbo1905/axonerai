//! Fake Context7 MCP facade: built-in Rust tools dressed as a Context7 MCP
//! server's tools. There is NO MCP host process — they call the Context7
//! REST API directly through the shared internals in
//! [`crate::tools::context7`], mirroring the Tavily facade in
//! [`crate::tools::tavily_mcp`].

use crate::tool::{Tool, ToolSource};
use crate::tools::context7::{
    DEFAULT_CONTEXT7_BASE_URL, clamp_page, get_library_docs, search_library,
};
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::env;

#[cfg(feature = "web")]
use tracing::debug;

/// Whether the Context7 facade tools should be registered at all: same
/// gating convention as the Tavily-backed web tools (key present → present).
pub fn is_configured() -> bool {
    env::var("CONTEXT7_API_KEY").is_ok()
}

pub struct Context7McpResolveLibraryId {
    base_url: String,
}

impl Context7McpResolveLibraryId {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_CONTEXT7_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for Context7McpResolveLibraryId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Context7ResolveInput {
    library_name: String,
}

#[async_trait]
impl Tool for Context7McpResolveLibraryId {
    fn name(&self) -> String {
        "context7_resolve_library_id".to_string()
    }

    fn description(&self) -> String {
        "Part of the Context7 MCP server. Built-in Context7 work-alike; resolves \
         library ids and fetches up-to-date docs. This tool turns a library name \
         (e.g. \"tokio\") into Context7-compatible library ids with trust and \
         benchmark metadata. Call it first when the user asks for Context7 docs; \
         feed the chosen id into context7_get_library_docs."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "library_name": {
                    "type": "string",
                    "description": "The name of the library to search for (e.g. \"tokio\")"
                }
            },
            "required": ["library_name"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Mcp
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let input: Context7ResolveInput = serde_json::from_value(input)
            .map_err(|e| anyhow!("Invalid context7_resolve_library_id input: {}", e))?;

        #[cfg(feature = "web")]
        debug!(
            "context7_resolve_library_id: library_name={}",
            input.library_name
        );

        let api_key = env::var("CONTEXT7_API_KEY").map_err(|_| {
            anyhow!(
                "CONTEXT7_API_KEY environment variable is not set; the \
                 context7_resolve_library_id tool requires a Context7 API key"
            )
        })?;

        search_library(&self.base_url, &api_key, &input.library_name).await
    }
}

pub struct Context7McpGetLibraryDocs {
    base_url: String,
}

impl Context7McpGetLibraryDocs {
    pub fn new() -> Self {
        Self {
            base_url: DEFAULT_CONTEXT7_BASE_URL.to_string(),
        }
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = base_url.into();
        self
    }
}

impl Default for Context7McpGetLibraryDocs {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct Context7DocsInput {
    context7_compatible_library_id: String,
    topic: Option<String>,
    page: Option<i64>,
}

#[async_trait]
impl Tool for Context7McpGetLibraryDocs {
    fn name(&self) -> String {
        "context7_get_library_docs".to_string()
    }

    fn description(&self) -> String {
        "Part of the Context7 MCP server. Built-in Context7 work-alike; resolves \
         library ids and fetches up-to-date docs. Fetches code documentation for \
         a Context7-compatible library id (as returned by \
         context7_resolve_library_id), optionally narrowed to a topic (e.g. \
         \"tokio::select\")."
            .to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "context7_compatible_library_id": {
                    "type": "string",
                    "description": "A Context7-compatible library id such as \"/tokio-rs/tokio\" (from context7_resolve_library_id)"
                },
                "topic": {
                    "type": "string",
                    "description": "Optional topic to focus the docs on (e.g. \"tokio::select\")"
                },
                "page": {
                    "type": "integer",
                    "description": "Optional docs page number (1-100, clamped; default first page)"
                }
            },
            "required": ["context7_compatible_library_id"]
        })
    }

    fn source(&self) -> ToolSource {
        ToolSource::Mcp
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let input: Context7DocsInput = serde_json::from_value(input)
            .map_err(|e| anyhow!("Invalid context7_get_library_docs input: {}", e))?;

        #[cfg(feature = "web")]
        debug!(
            "context7_get_library_docs: id={} topic={:?} page={:?}",
            input.context7_compatible_library_id, input.topic, input.page
        );

        let api_key = env::var("CONTEXT7_API_KEY").map_err(|_| {
            anyhow!(
                "CONTEXT7_API_KEY environment variable is not set; the \
                 context7_get_library_docs tool requires a Context7 API key"
            )
        })?;

        get_library_docs(
            &self.base_url,
            &api_key,
            &input.context7_compatible_library_id,
            input.topic.as_deref(),
            input.page.map(clamp_page),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::context7::{MAX_DOCS_OUTPUT_CHARS, MAX_PAGE, sample_search_response};
    use crate::tools::testing::{env_guard, set_context7_key, spawn_stub, unset_context7_key};
    use serde_json::json;

    #[tokio::test]
    async fn resolve_round_trips_through_stub_server() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(
                req.path, "/api/v1/search?query=tokio",
                "must hit the observed search endpoint with the query param"
            );
            assert_eq!(
                req.authorization.as_deref(),
                Some("Bearer test-key"),
                "must send Authorization: Bearer header"
            );
            assert!(req.body.is_empty(), "search is a GET with no body");
            (200, sample_search_response().to_string())
        })
        .await;

        let tool = Context7McpResolveLibraryId::new().with_base_url(server.base_url());
        assert_eq!(tool.name(), "context7_resolve_library_id");
        assert_eq!(tool.source(), ToolSource::Mcp);
        assert!(tool.is_read_only());
        assert!(
            tool.description()
                .contains("Part of the Context7 MCP server"),
            "description must mention the Context7 MCP server"
        );
        assert!(
            tool.description().contains("Built-in Context7 work-alike"),
            "description must stay honest about being built-in"
        );

        let output = tool
            .execute(json!({"library_name": "tokio"}))
            .await
            .expect("facade resolve round-trip should succeed");
        assert!(output.contains("1. Tokio (/tokio-rs/tokio)"));
        assert!(output.contains("trust 7.5, benchmark 77.98"));
        assert!(output.contains("verified: false"));
        unset_context7_key();
    }

    #[tokio::test]
    async fn resolve_reports_no_results_for_empty_hit_list() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|_req| (200, r#"{"results": []}"#.to_string())).await;

        let tool = Context7McpResolveLibraryId::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"library_name": "does-not-exist-xyz"}))
            .await
            .expect("empty hit list is not an error");
        assert!(output.contains("No Context7 libraries found"));
        unset_context7_key();
    }

    #[tokio::test]
    async fn docs_round_trips_through_stub_server() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(
                req.path,
                "/api/v1/tokio-rs/tokio?type=code&topic=tokio%3A%3Aselect&page=2",
                "must hit the observed docs endpoint with topic/type/page params"
            );
            assert_eq!(
                req.authorization.as_deref(),
                Some("Bearer test-key"),
                "must send Authorization: Bearer header"
            );
            (
                200,
                "### Graceful shutdown\n\nUses tokio::select! to kill a child process.\n\n```rust\ntokio::select! { ... }\n```".to_string(),
            )
        })
        .await;

        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        assert_eq!(tool.name(), "context7_get_library_docs");
        assert_eq!(tool.source(), ToolSource::Mcp);
        assert!(tool.is_read_only());
        assert!(
            tool.description()
                .contains("Part of the Context7 MCP server"),
            "description must mention the Context7 MCP server"
        );

        let output = tool
            .execute(json!({
                "context7_compatible_library_id": "/tokio-rs/tokio",
                "topic": "tokio::select",
                "page": 2
            }))
            .await
            .expect("facade docs round-trip should succeed");
        assert!(output.contains("### Graceful shutdown"));
        assert!(output.contains("tokio::select!"));
        unset_context7_key();
    }

    #[tokio::test]
    async fn docs_omits_optional_params_when_absent() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(
                req.path, "/api/v1/tokio-rs/tokio?type=code",
                "only the default type=code param is sent"
            );
            (200, "docs body".to_string())
        })
        .await;

        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"context7_compatible_library_id": "tokio-rs/tokio"}))
            .await
            .expect("docs without optional params should succeed");
        assert_eq!(output, "docs body");
        unset_context7_key();
    }

    #[tokio::test]
    async fn error_status_maps_to_err() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|req| {
            assert!(req.path.starts_with("/api/v1/search"));
            (
                401,
                r#"{"error":"invalid_api_key","message":"Invalid API key. Please check your API key."}"#
                    .to_string(),
            )
        })
        .await;

        let tool = Context7McpResolveLibraryId::new().with_base_url(server.base_url());
        let err = tool
            .execute(json!({"library_name": "tokio"}))
            .await
            .expect_err("401 must surface as Err");
        assert!(
            err.to_string()
                .contains("Context7 search request failed with status 401"),
            "unexpected error: {err}"
        );
        unset_context7_key();

        set_context7_key("test-key");
        let server = spawn_stub(|_req| (500, "boom".to_string())).await;
        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        let err = tool
            .execute(json!({"context7_compatible_library_id": "/x/y"}))
            .await
            .expect_err("500 must surface as Err");
        assert!(
            err.to_string()
                .contains("Context7 docs request failed with status 500"),
            "unexpected error: {err}"
        );
        unset_context7_key();
    }

    #[tokio::test]
    async fn docs_payload_is_capped_with_truncation_marker() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let huge = "x".repeat(MAX_DOCS_OUTPUT_CHARS + 5_000);
        let server = spawn_stub(move |_req| (200, huge.clone())).await;

        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({"context7_compatible_library_id": "/tokio-rs/tokio"}))
            .await
            .expect("oversized payload still succeeds (truncated)");
        assert!(
            output.ends_with(crate::tools::context7::TRUNCATION_MARKER),
            "payload must end with the explicit truncation marker"
        );
        assert!(
            output.chars().count() <= MAX_DOCS_OUTPUT_CHARS,
            "payload must be capped at the limit"
        );
        unset_context7_key();
    }

    #[tokio::test]
    async fn page_and_topic_params_are_clamped() {
        let _guard = env_guard();
        set_context7_key("test-key");
        let server = spawn_stub(|req| {
            assert_eq!(
                req.path, "/api/v1/tokio-rs/tokio?type=code&page=100",
                "page must clamp into 1..=100"
            );
            (200, "docs".to_string())
        })
        .await;

        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({
                "context7_compatible_library_id": "/tokio-rs/tokio",
                "page": 999
            }))
            .await
            .expect("clamped page should succeed");
        assert_eq!(output, "docs");

        let server = spawn_stub(|req| {
            assert_eq!(
                req.path, "/api/v1/tokio-rs/tokio?type=code&page=1",
                "page must clamp up to 1"
            );
            (200, "docs".to_string())
        })
        .await;
        let tool = Context7McpGetLibraryDocs::new().with_base_url(server.base_url());
        let output = tool
            .execute(json!({
                "context7_compatible_library_id": "/tokio-rs/tokio",
                "page": 0
            }))
            .await
            .expect("clamped page should succeed");
        assert_eq!(output, "docs");
        unset_context7_key();

        // Pure clamp helper covers the remaining edges.
        assert_eq!(clamp_page(-5), 1);
        assert_eq!(clamp_page(1), 1);
        assert_eq!(clamp_page(50), 50);
        assert_eq!(clamp_page(MAX_PAGE), MAX_PAGE);
        assert_eq!(clamp_page(MAX_PAGE + 1), MAX_PAGE);
    }

    #[tokio::test]
    async fn missing_api_key_errors_clearly() {
        let _guard = env_guard();
        unset_context7_key();

        let tool = Context7McpResolveLibraryId::new();
        let err = tool
            .execute(json!({"library_name": "tokio"}))
            .await
            .expect_err("missing key must error");
        assert!(
            err.to_string().contains("CONTEXT7_API_KEY"),
            "unexpected error: {err}"
        );

        let tool = Context7McpGetLibraryDocs::new();
        let err = tool
            .execute(json!({"context7_compatible_library_id": "/tokio-rs/tokio"}))
            .await
            .expect_err("missing key must error");
        assert!(
            err.to_string().contains("CONTEXT7_API_KEY"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn registration_gating_follows_the_api_key() {
        let _guard = env_guard();

        unset_context7_key();
        assert!(
            !is_configured(),
            "with the key absent the facade must report unconfigured (not registered)"
        );

        set_context7_key("test-key");
        assert!(
            is_configured(),
            "with the key present the facade must report configured (registered)"
        );
        unset_context7_key();
    }

    #[test]
    fn tools_survive_the_read_only_filter() {
        let mut registry = crate::tool::ToolRegistry::new();
        registry.register(Box::new(Context7McpResolveLibraryId::new()));
        registry.register(Box::new(Context7McpGetLibraryDocs::new()));
        registry.register(Box::new(crate::tools::WriteFile::default()));

        let read_only = registry.into_read_only();
        let names = read_only.list_tools();
        assert!(names.contains(&"context7_resolve_library_id".to_string()));
        assert!(names.contains(&"context7_get_library_docs".to_string()));
        assert!(!names.contains(&"write_file".to_string()));
    }
}
