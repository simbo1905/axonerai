use anyhow::{Result};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::PathBuf;
use crate::session::{Session, SessionManager};
use crate::provider::Message;

#[derive(Debug, Clone)]
pub struct Record {
    pub header: String,
    pub data: Vec<u8>,
}

pub struct FlatFileSessionManager {
    session_id: String,
    base_dir: PathBuf,
}

impl FlatFileSessionManager {
    pub fn new(session_id: String, base_dir: PathBuf) -> Self {
        Self { session_id, base_dir }
    }

    pub fn session_path(&self, session_id: &str) -> PathBuf {
        self.base_dir.join(session_id).join("session.dat")
    }

    pub fn create_session(&self, session_id: &str) -> Result<()> {
        let dir = self.base_dir.join(session_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("session.dat");
        if !path.exists() {
            fs::File::create(path)?;
        }
        Ok(())
    }

    pub fn append_record(&self, session_id: &str, header: &str, data: &[u8]) -> Result<()> {
        let path = self.session_path(session_id);
        if !path.exists() {
             self.create_session(session_id)?;
        }
        
        let mut file = OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)?;
            
        // Format: Header\nData\0
        file.write_all(header.as_bytes())?;
        file.write_all(b"\n")?;
        file.write_all(data)?;
        file.write_all(b"\0")?;
        
        Ok(())
    }

    pub fn read_records(&self, session_id: &str) -> Result<Vec<Record>> {
        let path = self.session_path(session_id);
        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut file = fs::File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        let mut records = Vec::new();
        let mut i = 0;
        let len = buffer.len();

        while i < len {
            // Find newline for header
            let mut newline_pos = None;
            for j in i..len {
                if buffer[j] == b'\n' {
                    newline_pos = Some(j);
                    break;
                }
            }

            if let Some(nl) = newline_pos {
                let header_bytes = &buffer[i..nl];
                let header = String::from_utf8_lossy(header_bytes).to_string();
                
                // Find null byte for data end
                let start_data = nl + 1;
                let mut null_pos = None;
                
                for j in start_data..len {
                    if buffer[j] == 0 {
                        null_pos = Some(j);
                        break;
                    }
                }

                if let Some(null) = null_pos {
                    let data = buffer[start_data..null].to_vec();
                    records.push(Record { header, data });
                    i = null + 1;
                } else {
                    // Malformed or incomplete last record
                    break; 
                }
            } else {
                break;
            }
        }
        
        Ok(records)
    }
}

impl SessionManager for FlatFileSessionManager {
    fn load(&self) -> Result<Session> {
        let records = self.read_records(&self.session_id)?;
        let mut messages = Vec::new();
        for r in records {
            if r.header == "UserMessage" {
                messages.push(Message {
                    role: "user".to_string(),
                    content: String::from_utf8_lossy(&r.data).to_string(),
                });
            } else if r.header == "AssistantMessage" {
                messages.push(Message {
                    role: "assistant".to_string(),
                    content: String::from_utf8_lossy(&r.data).to_string(),
                });
            }
        }
        Ok(Session {
            session_id: self.session_id.clone(),
            messages,
            time_stamp: "now".to_string(),
        })
    }

    fn save(&self, session: &Session) -> Result<()> {
        let dir = self.base_dir.join(&self.session_id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("session.dat");
        
        // Truncate file and rewrite all
        let mut file = fs::File::create(&path)?;
        
        for msg in &session.messages {
            let header = match msg.role.as_str() {
                "user" => "UserMessage",
                "assistant" => "AssistantMessage",
                _ => "UnknownMessage",
            };
            file.write_all(header.as_bytes())?;
            file.write_all(b"\n")?;
            file.write_all(msg.content.as_bytes())?;
            file.write_all(b"\0")?;
        }
        Ok(())
    }

    fn exists(&self) -> bool {
        self.session_path(&self.session_id).exists()
    }

    fn get_session_id(&self) -> &str {
        &self.session_id
    }
}
