//! Pluggable session storage trait and implementations.
//!
//! This module provides a trait for session storage backends and various
//! implementations including file-based storage with null-byte delimited format.

use crate::session::Session;
use crate::provider::Message;
use anyhow::Result;
use async_trait::async_trait;
use std::path::PathBuf;

/// Trait for session storage backends.
/// 
/// Implementations can store sessions in various ways: in-memory, file-based,
/// database-backed, etc.
#[async_trait]
pub trait SessionStorage: Send + Sync {
    /// Save a session to storage
    async fn save(&self, session: &Session) -> Result<()>;
    
    /// Load a session from storage
    async fn load(&self, session_id: &str) -> Result<Option<Session>>;
    
    /// Check if a session exists
    async fn exists(&self, session_id: &str) -> Result<bool>;
    
    /// Delete a session
    async fn delete(&self, session_id: &str) -> Result<()>;
    
    /// List all session IDs
    async fn list_sessions(&self) -> Result<Vec<String>>;
    
    /// Append a message to a session (streaming-friendly)
    async fn append_message(&self, session_id: &str, message: &Message) -> Result<()>;
}

/// In-memory session storage (stateless between restarts)
pub struct MemorySessionStorage {
    sessions: tokio::sync::RwLock<std::collections::HashMap<String, Session>>,
}

impl MemorySessionStorage {
    pub fn new() -> Self {
        Self {
            sessions: tokio::sync::RwLock::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for MemorySessionStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStorage for MemorySessionStorage {
    async fn save(&self, session: &Session) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.get_id().to_string(), session.clone());
        Ok(())
    }
    
    async fn load(&self, session_id: &str) -> Result<Option<Session>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(session_id).cloned())
    }
    
    async fn exists(&self, session_id: &str) -> Result<bool> {
        let sessions = self.sessions.read().await;
        Ok(sessions.contains_key(session_id))
    }
    
    async fn delete(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(session_id);
        Ok(())
    }
    
    async fn list_sessions(&self) -> Result<Vec<String>> {
        let sessions = self.sessions.read().await;
        Ok(sessions.keys().cloned().collect())
    }
    
    async fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(session_id) {
            session.add_message(message.clone());
        }
        Ok(())
    }
}

/// Null-byte delimited file session storage.
/// 
/// Format:
/// ```text
/// <record_type>\n<data>\0
/// ```
/// 
/// Record types:
/// - `SESSION_META` - Session metadata (id, timestamp)
/// - `USER_MSG` - User message
/// - `ASSISTANT_MSG` - Assistant message
/// - `TOOL_CALL` - Tool call record
/// - `TOOL_RESULT` - Tool result record
/// - `BINARY` - Binary data (base64 encoded)
pub struct NullDelimitedSessionStorage {
    base_dir: PathBuf,
}

impl NullDelimitedSessionStorage {
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&base_dir)?;
        Ok(Self { base_dir })
    }
    
    fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(format!("{}.session", session_id))
    }
    
    /// Write a record to the session file
    fn write_record(file: &mut std::fs::File, record_type: &str, data: &str) -> Result<()> {
        use std::io::Write;
        write!(file, "{}\n{}\0", record_type, data)?;
        Ok(())
    }
    
    /// Parse records from raw bytes
    fn parse_records(content: &[u8]) -> Vec<(String, String)> {
        let mut records = Vec::new();
        let content_str = String::from_utf8_lossy(content);
        
        for record in content_str.split('\0') {
            if record.is_empty() {
                continue;
            }
            if let Some(newline_pos) = record.find('\n') {
                let record_type = record[..newline_pos].to_string();
                let data = record[newline_pos + 1..].to_string();
                records.push((record_type, data));
            }
        }
        
        records
    }
    
    /// Build a session from parsed records
    fn build_session(records: Vec<(String, String)>) -> Result<Session> {
        let mut session_id = String::new();
        let mut timestamp = String::new();
        let mut messages = Vec::new();
        
        for (record_type, data) in records {
            match record_type.as_str() {
                "SESSION_META" => {
                    // Format: session_id|timestamp
                    let parts: Vec<&str> = data.splitn(2, '|').collect();
                    if parts.len() >= 2 {
                        session_id = parts[0].to_string();
                        timestamp = parts[1].to_string();
                    }
                }
                "USER_MSG" => {
                    messages.push(Message {
                        role: "user".to_string(),
                        content: data,
                    });
                }
                "ASSISTANT_MSG" => {
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: data,
                    });
                }
                "TOOL_CALL" | "TOOL_RESULT" => {
                    // Tool calls are stored as assistant messages for context
                    messages.push(Message {
                        role: "assistant".to_string(),
                        content: format!("[{}] {}", record_type, data),
                    });
                }
                _ => {
                    // Unknown record type, skip
                }
            }
        }
        
        let mut session = Session::new_with_timestamp(session_id, timestamp);
        for msg in messages {
            session.add_message(msg);
        }
        
        Ok(session)
    }
}

#[async_trait]
impl SessionStorage for NullDelimitedSessionStorage {
    async fn save(&self, session: &Session) -> Result<()> {
        use std::io::Write;
        
        let path = self.session_path(session.get_id());
        let mut file = std::fs::File::create(&path)?;
        
        // Write session metadata
        let meta = format!("{}|{}", session.get_id(), session.get_timestamp());
        Self::write_record(&mut file, "SESSION_META", &meta)?;
        
        // Write all messages
        for message in session.get_messages() {
            let record_type = match message.role.as_str() {
                "user" => "USER_MSG",
                "assistant" => "ASSISTANT_MSG",
                _ => "USER_MSG",
            };
            Self::write_record(&mut file, record_type, &message.content)?;
        }
        
        file.flush()?;
        Ok(())
    }
    
    async fn load(&self, session_id: &str) -> Result<Option<Session>> {
        let path = self.session_path(session_id);
        
        if !path.exists() {
            return Ok(None);
        }
        
        let content = std::fs::read(&path)?;
        let records = Self::parse_records(&content);
        
        if records.is_empty() {
            return Ok(None);
        }
        
        let session = Self::build_session(records)?;
        Ok(Some(session))
    }
    
    async fn exists(&self, session_id: &str) -> Result<bool> {
        Ok(self.session_path(session_id).exists())
    }
    
    async fn delete(&self, session_id: &str) -> Result<()> {
        let path = self.session_path(session_id);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(())
    }
    
    async fn list_sessions(&self) -> Result<Vec<String>> {
        let mut sessions = Vec::new();
        
        if let Ok(entries) = std::fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if name.ends_with(".session") {
                        sessions.push(name.trim_end_matches(".session").to_string());
                    }
                }
            }
        }
        
        Ok(sessions)
    }
    
    async fn append_message(&self, session_id: &str, message: &Message) -> Result<()> {
        use std::io::Write;
        
        let path = self.session_path(session_id);
        
        // Create file with metadata if it doesn't exist
        if !path.exists() {
            let mut file = std::fs::File::create(&path)?;
            let timestamp = chrono::Utc::now().to_rfc3339();
            let meta = format!("{}|{}", session_id, timestamp);
            Self::write_record(&mut file, "SESSION_META", &meta)?;
        }
        
        // Append the message
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)?;
        
        let record_type = match message.role.as_str() {
            "user" => "USER_MSG",
            "assistant" => "ASSISTANT_MSG",
            _ => "USER_MSG",
        };
        
        Self::write_record(&mut file, record_type, &message.content)?;
        file.flush()?;
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[tokio::test]
    async fn test_memory_storage() {
        let storage = MemorySessionStorage::new();
        let session = Session::new("test-session".to_string());
        
        storage.save(&session).await.unwrap();
        assert!(storage.exists("test-session").await.unwrap());
        
        let loaded = storage.load("test-session").await.unwrap();
        assert!(loaded.is_some());
    }
    
    #[tokio::test]
    async fn test_null_delimited_storage() {
        let tmp_dir = TempDir::new().unwrap();
        let storage = NullDelimitedSessionStorage::new(tmp_dir.path().to_path_buf()).unwrap();
        
        let mut session = Session::new("test-session".to_string());
        session.add_message(Message {
            role: "user".to_string(),
            content: "Hello".to_string(),
        });
        session.add_message(Message {
            role: "assistant".to_string(),
            content: "Hi there!".to_string(),
        });
        
        storage.save(&session).await.unwrap();
        assert!(storage.exists("test-session").await.unwrap());
        
        let loaded = storage.load("test-session").await.unwrap();
        assert!(loaded.is_some());
        
        let loaded_session = loaded.unwrap();
        assert_eq!(loaded_session.get_messages().len(), 2);
    }
}
