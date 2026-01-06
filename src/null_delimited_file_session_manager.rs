use crate::provider::Message;
use crate::session::Session;
use crate::session_manager::SessionManager;
use anyhow::{anyhow, Result};
use base64::Engine;
use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Flat-file session storage:
/// `header\n<data>\0` repeated, where header is ASCII key=value pairs.
///
/// This is designed to be stream-friendly: you can read from a growing file
/// and split records by the NUL delimiter.
///
/// Example record header:
/// `type=message role=user encoding=utf8`
pub struct NullDelimitedFileSessionManager {
    session_id: String,
    directory: PathBuf,
}

impl NullDelimitedFileSessionManager {
    pub fn new(session_id: String, base_dir: PathBuf) -> Result<Self> {
        fs::create_dir_all(base_dir.join(&session_id))?;
        Ok(Self {
            session_id,
            directory: base_dir,
        })
    }

    fn session_dir(&self) -> PathBuf {
        self.directory.join(&self.session_id)
    }

    fn log_path(&self) -> PathBuf {
        self.session_dir().join("session.ndl") // NUL-delimited log
    }

    fn append_record(&self, header: &str, data: &[u8]) -> Result<()> {
        if header.contains('\n') || header.contains('\0') {
            return Err(anyhow!("record header must not contain newline or NUL"));
        }
        if data.iter().any(|b| *b == 0) {
            // Allow binary via base64 (no NUL bytes).
            return Err(anyhow!("record data must not contain NUL bytes; use base64 encoding"));
        }

        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path())?;

        f.write_all(header.as_bytes())?;
        f.write_all(b"\n")?;
        f.write_all(data)?;
        f.write_all(b"\0")?;
        f.flush()?;
        Ok(())
    }

    fn parse_header_line(line: &[u8]) -> Result<HashMap<String, String>> {
        let s = std::str::from_utf8(line)?.trim();
        let mut out = HashMap::new();
        for part in s.split_whitespace() {
            let (k, v) = part
                .split_once('=')
                .ok_or_else(|| anyhow!("invalid header segment: {part}"))?;
            out.insert(k.to_string(), v.to_string());
        }
        Ok(out)
    }

    fn parse_record_bytes(record: &[u8]) -> Result<Option<Message>> {
        if record.is_empty() {
            return Ok(None);
        }
        let Some(split_at) = record.iter().position(|b| *b == b'\n') else {
            return Ok(None);
        };
        let header = Self::parse_header_line(&record[..split_at])?;
        let data = &record[split_at + 1..];

        let r#type = header.get("type").map(|s| s.as_str()).unwrap_or("");
        if r#type != "message" {
            return Ok(None);
        }

        let role = header
            .get("role")
            .ok_or_else(|| anyhow!("message record missing role"))?
            .to_string();
        let encoding = header.get("encoding").map(|s| s.as_str()).unwrap_or("utf8");

        let content_bytes = match encoding {
            "utf8" => data.to_vec(),
            "base64" => base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| anyhow!("base64 decode failed: {e}"))?,
            other => return Err(anyhow!("unknown encoding: {other}")),
        };

        let content = String::from_utf8(content_bytes)?;
        Ok(Some(Message { role, content }))
    }

    fn load_messages_from_path(path: &Path) -> Result<Vec<Message>> {
        if !path.exists() {
            return Ok(vec![]);
        }
        let mut f = fs::File::open(path)?;
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;

        let mut out = Vec::new();
        for record in bytes.split(|b| *b == 0) {
            if let Some(msg) = Self::parse_record_bytes(record)? {
                out.push(msg);
            }
        }
        Ok(out)
    }

    pub fn append_message(&self, role: &str, content: &str) -> Result<()> {
        if content.as_bytes().iter().any(|b| *b == 0) {
            let header = format!("type=message role={} encoding=base64", role);
            let b64 = base64::engine::general_purpose::STANDARD.encode(content.as_bytes());
            return self.append_record(&header, b64.as_bytes());
        }

        let header = format!("type=message role={} encoding=utf8", role);
        self.append_record(&header, content.as_bytes())
    }
}

impl SessionManager for NullDelimitedFileSessionManager {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn exists(&self) -> bool {
        self.log_path().exists()
    }

    fn load(&self) -> Result<Session> {
        let mut s = Session::new(self.session_id.clone());
        for msg in Self::load_messages_from_path(&self.log_path())? {
            s.add_message(msg);
        }
        Ok(s)
    }

    fn save(&self, session: &Session) -> Result<()> {
        // Append-only, but we don’t currently track the delta in-memory.
        // We compute the on-disk message count and append only new messages.
        fs::create_dir_all(self.session_dir())?;

        let existing = Self::load_messages_from_path(&self.log_path())?;
        let existing_len = existing.len();
        let current = session.get_messages();

        if existing_len > current.len() {
            // Session was truncated/rewound; rewrite.
            fs::write(self.log_path(), &[])?;
            for m in current {
                self.append_message(&m.role, &m.content)?;
            }
            return Ok(());
        }

        for m in &current[existing_len..] {
            self.append_message(&m.role, &m.content)?;
        }
        Ok(())
    }

    fn raw_session_path(&self) -> Option<PathBuf> {
        Some(self.log_path())
    }
}

