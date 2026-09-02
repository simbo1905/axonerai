//! Shared Tavily REST API internals. Both the builtin WebSearch/WebFetch
//! tools and the fake Tavily MCP facade tools ([`crate::tools::tavily_mcp`])
//! call these functions, so their wire behaviour and result formatting stay
//! identical.

use anyhow::{Ok, Result, anyhow};
use serde_json::{Value, json};

pub const DEFAULT_TAVILY_BASE_URL: &str = "https://api.tavily.com";
pub const DEFAULT_MAX_RESULTS: i64 = 5;
pub const MAX_SNIPPET_CHARS: usize = 500;
pub const MAX_OUTPUT_CHARS: usize = 8000;
pub const TRUNCATION_MARKER: &str = "[… truncated]";

/// POST `<base_url>/search` with bearer auth and format the results.
pub async fn search(
    base_url: &str,
    api_key: &str,
    query: &str,
    max_results: i64,
) -> Result<String> {
    let url = format!("{}/search", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&json!({
            "query": query,
            "search_depth": "basic",
            "max_results": max_results
        }))
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        let excerpt: String = body.chars().take(200).collect();
        return Err(anyhow!(
            "Tavily search request failed with status {}: {}",
            status,
            excerpt
        ));
    }

    let data: Value = serde_json::from_str(&body)?;
    format_results(&data, query)
}

pub fn format_results(data: &Value, query: &str) -> Result<String> {
    let results = data["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing results in Tavily response"))?;

    if results.is_empty() {
        return Ok(format!("No results found for: {}", query));
    }

    let mut output = String::new();
    for (index, result) in results.iter().enumerate() {
        let title = result["title"].as_str().unwrap_or("");
        let url = result["url"].as_str().unwrap_or("");
        let content = result["content"].as_str().unwrap_or("");
        let snippet: String = content.chars().take(MAX_SNIPPET_CHARS).collect();

        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!(
            "{}. {}\n   {}\n   {}",
            index + 1,
            title,
            url,
            snippet
        ));
    }
    Ok(output)
}

/// POST `<base_url>/extract` with bearer auth and format the extracted text.
pub async fn extract(
    base_url: &str,
    api_key: &str,
    url: &str,
    query: Option<&str>,
) -> Result<String> {
    let mut body = json!({
        "urls": [url],
        "extract_depth": "basic"
    });
    if let Some(query) = query {
        body["query"] = json!(query);
    }

    let url = format!("{}/extract", base_url.trim_end_matches('/'));
    let response = reqwest::Client::new()
        .post(url)
        .bearer_auth(api_key)
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    let response_body = response.text().await?;
    if !status.is_success() {
        let excerpt: String = response_body.chars().take(200).collect();
        return Err(anyhow!(
            "Tavily extract request failed with status {}: {}",
            status,
            excerpt
        ));
    }

    let data: Value = serde_json::from_str(&response_body)?;
    format_extract(&data)
}

pub fn format_extract(data: &Value) -> Result<String> {
    let results = data["results"]
        .as_array()
        .ok_or_else(|| anyhow!("Missing results in Tavily response"))?;

    let mut output = String::new();
    for result in results {
        let url = result["url"].as_str().unwrap_or("");
        let raw_content = result["raw_content"].as_str().unwrap_or("");

        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&format!("== {} ==\n", url));
        if raw_content.is_empty() {
            output.push_str("(no content extracted)");
        } else {
            output.push_str(raw_content);
        }
    }

    if output.chars().count() > MAX_OUTPUT_CHARS {
        let keep = MAX_OUTPUT_CHARS - TRUNCATION_MARKER.chars().count();
        let truncated: String = output.chars().take(keep).collect();
        output = format!("{}{}", truncated, TRUNCATION_MARKER);
    }

    if let Some(failed) = data["failed_results"].as_array() {
        let failed_urls: Vec<&str> = failed.iter().filter_map(|f| f["url"].as_str()).collect();
        if !failed_urls.is_empty() {
            output.push_str(&format!("\n\nFailed to fetch: {}", failed_urls.join(", ")));
        }
    }

    Ok(output)
}
