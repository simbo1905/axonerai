use crate::session::Session;
use anyhow::Result;
use std::path::PathBuf;

/// Pluggable session persistence strategy.
///
/// This crate’s `Agent` only needs `Session` load/save/existence checks.
/// Implementations may additionally expose a raw on-disk representation via `raw_session_path()`.
pub trait SessionManager: Send + Sync {
    fn session_id(&self) -> &str;
    fn exists(&self) -> bool;
    fn load(&self) -> Result<Session>;
    fn save(&self, session: &Session) -> Result<()>;

    /// Optional path to the raw session log (if any).
    fn raw_session_path(&self) -> Option<PathBuf> {
        None
    }
}

