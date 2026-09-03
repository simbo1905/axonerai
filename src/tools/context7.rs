//! Shared Context7 REST API internals. Both the fake Context7 MCP facade
//! tools ([`crate::tools::context7_mcp`]) call these functions, so their wire
//! behaviour and result formatting stay in one place (same split as the
//! Tavily facade in [`crate::tools::tavily`]).
//!
//! # Observed wire shapes (probed 2026-09-04 against context7.com with a real
//! `CONTEXT7_API_KEY`)
//!
//! Auth: `Authorization: Bearer <CONTEXT7_API_KEY>` on every request. An
//! invalid key gets `401` with JSON body
//! `{"error":"invalid_api_key","message":"Invalid API key. ..."}`.
//!
//! Resolve (search): `GET <base>/api/v1/search?query=<library_name>` →
//! `200` JSON `{"results":[{ "id": "/tokio-rs/tokio", "title": "Tokio",
//! "description": "...", "branch": "master", "lastUpdateDate": "...",
//! "state": "finalized", "totalTokens": 39354, "totalSnippets": 575,
//! "stars": 28524, "trustScore": 7.5, "benchmarkScore": 77.98,
//! "versions": [], "score": 444.8, "vip": false, "verified": false }, …]}`.
//!
//! Docs: `GET <base>/api/v1/<library-id>?topic=<topic>&type=code&page=<n>`
//! → `200` with a plain-text markdown body (NOT JSON): `### heading` blocks
//! with `Source: <url>` lines and fenced code samples, separated by
//! `--------------------------------` rules. The `page` parameter pages
//! through the topic-filtered snippet list.
//!
//! Library ids contain slashes (`/tokio-rs/tokio`, `/websites/rs_tokio`), so
//! they are used as-is as the path tail after `/api/v1`.

use anyhow::{Result, anyhow};
use serde_json::Value;

pub const DEFAULT_CONTEXT7_BASE_URL: &str = "https://context7.com";
/// How many search hits `format_search_results` renders.
pub const MAX_SEARCH_RESULTS: usize = 5;
/// Cap on the docs payload returned to the model (~100 KB of characters).
pub const MAX_DOCS_OUTPUT_CHARS: usize = 100_000;
/// Clamp for the optional `page` parameter.
pub const MAX_PAGE: i64 = 100;
/// Clamp for free-text path/query inputs (library name / id / topic).
pub const MAX_INPUT_CHARS: usize = 512;
pub const TRUNCATION_MARKER: &str = "[… truncated: docs payload exceeded the 100 KB cap]";

/// GET `<base_url>/api/v1/search?query=<library_name>` with bearer auth and
/// format the hits (id, title, trust/benchmark metadata, description).
pub async fn search_library(base_url: &str, api_key: &str, library_name: &str) -> Result<String> {
    let library_name: String = library_name.chars().take(MAX_INPUT_CHARS).collect();
    if library_name.trim().is_empty() {
        return Err(anyhow!("library_name must not be empty"));
    }

    let url = format!("{}/api/v1/search", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .get(url)
        .bearer_auth(api_key)
        .query(&[("query", library_name.as_str())])
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let excerpt: String = body.chars().take(200).collect();
        return Err(anyhow!(
            "Context7 search request failed with status {}: {}",
            status,
            excerpt
        ));
    }

    let data: Value = serde_json::from_str(&body)?;
    format_search_results(&data, &library_name)
}

pub fn format_search_results(data: &Value, library_name: &str) -> Result<String> {
    let results = data["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing results in Context7 response"))?;

    if results.is_empty() {
        return Ok(format!("No Context7 libraries found for: {}", library_name));
    }

    let mut output = String::new();
    for (index, result) in results.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let id = result["id"].as_str().unwrap_or("");
        let title = result["title"].as_str().unwrap_or("");
        let description = result["description"].as_str().unwrap_or("");
        let snippet: String = description.chars().take(500).collect();
        let trust_score = result["trustScore"].as_f64().unwrap_or(0.0);
        let benchmark_score = result["benchmarkScore"].as_f64().unwrap_or(0.0);
        let total_tokens = result["totalTokens"].as_i64().unwrap_or(0);
        let verified = result["verified"].as_bool().unwrap_or(false);

        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!(
            "{}. {} ({})\n   trust {:.1}, benchmark {:.2}, tokens {}, verified: {}\n   {}",
            index + 1,
            title,
            id,
            trust_score,
            benchmark_score,
            total_tokens,
            verified,
            snippet
        ));
    }
    Ok(output)
}

/// GET `<base_url>/api/v1/<library_id>` with bearer auth and optional
/// `topic`/`page` query parameters (`type=code` is always sent, per the
/// observed default). The response body is plain-text markdown; it is
/// returned as-is, capped at [`MAX_DOCS_OUTPUT_CHARS`] characters with an
/// explicit truncation marker.
pub async fn get_library_docs(
    base_url: &str,
    api_key: &str,
    library_id: &str,
    topic: Option<&str>,
    page: Option<i64>,
) -> Result<String> {
    let library_id: String = library_id
        .trim()
        .trim_start_matches('/')
        .chars()
        .take(MAX_INPUT_CHARS)
        .collect();
    if library_id.is_empty() {
        return Err(anyhow!("context7_compatible_library_id must not be empty"));
    }

    let url = format!("{}/api/v1/{}", base_url.trim_end_matches('/'), library_id);
    let mut request = reqwest::Client::new()
        .get(url)
        .bearer_auth(api_key)
        .query(&[("type", "code")]);
    if let Some(topic) = topic {
        let topic: String = topic.chars().take(MAX_INPUT_CHARS).collect();
        request = request.query(&[("topic", topic.as_str())]);
    }
    if let Some(page) = page {
        request = request.query(&[("page", page.clamp(1, MAX_PAGE))]);
    }
    let response = request.send().await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let excerpt: String = body.chars().take(200).collect();
        return Err(anyhow!(
            "Context7 docs request failed with status {}: {}",
            status,
            excerpt
        ));
    }

    if body.chars().count() > MAX_DOCS_OUTPUT_CHARS {
        let keep = MAX_DOCS_OUTPUT_CHARS - TRUNCATION_MARKER.chars().count();
        let truncated: String = body.chars().take(keep).collect();
        return Ok(format!("{}{}", truncated, TRUNCATION_MARKER));
    }
    Ok(body)
}

/// Helper for the tool layer: clamp an optional page parameter the same way
/// the request path does, so callers can validate before sending.
pub fn clamp_page(page: i64) -> i64 {
    page.clamp(1, MAX_PAGE)
}

/// Keep JSON tests honest about the observed search hit shape.
#[cfg(test)]
pub(crate) fn sample_search_response() -> Value {
    use serde_json::json;
    json!({
        "results": [
            {
                "id": "/tokio-rs/tokio",
                "title": "Tokio",
                "description": "A runtime for writing reliable asynchronous applications with Rust.",
                "branch": "master",
                "lastUpdateDate": "2026-09-02T13:27:47.479Z",
                "state": "finalized",
                "totalTokens": 39354,
                "totalSnippets": 575,
                "stars": 28524,
                "trustScore": 7.5,
                "benchmarkScore": 77.98,
                "versions": [],
                "score": 444.8,
                "vip": false,
                "verified": false
            }
        ]
    })
}
