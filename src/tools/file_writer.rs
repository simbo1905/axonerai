use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::Path;

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> String {
        "write_file".to_string()
    }

    fn description(&self) -> String {
        "Write text content to a file at the specified path. Usage: {\"path\": \"example.txt\", \"content\": \"Hello world\"}".to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "The file path to write to"
                },
                "content": {
                    "type": "string",
                    "description": "The text content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path_str = input["path"].as_str().ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let content = input["content"].as_str().ok_or_else(|| anyhow::anyhow!("Missing content"))?;

        eprintln!("[WriteFile] path={}, content_len={}", path_str, content.len());

        if path_str.contains("..") || path_str.starts_with('/') {
             return Ok("Error: For security, absolute paths and parent directory traversal (..) are not allowed.".to_string());
        }

        let path = Path::new(path_str);
        eprintln!("[WriteFile] resolved path={:?}, cwd={:?}", path, std::env::current_dir()?);
        
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::write(path, content)?;
        Ok(format!("Successfully wrote {} bytes to '{}'", content.len(), path_str))
    }
}
