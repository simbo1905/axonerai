//! Local settings persisted to `.axonerai/settings.jsonc` (JSONC: `//` and
//! `/* */` comments are allowed and stripped on load; plain JSON written by
//! `save()` is valid JSONC).

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const DEFAULT_SETTINGS_PATH: &str = ".axonerai/settings.jsonc";

/// User-level tool settings. Currently only per-tool suppression, but the
/// shape is deliberately flat and forward-compatible.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct Settings {
    #[serde(default)]
    pub suppressed_tools: Vec<String>,
}

impl Settings {
    /// Load settings from `.axonerai/settings.jsonc`. A missing file yields
    /// defaults; a corrupt file yields defaults with a warning.
    pub fn load() -> Self {
        Self::load_from(Path::new(DEFAULT_SETTINGS_PATH))
    }

    /// Load settings from an explicit path (used by tests and tooling).
    pub fn load_from(path: &Path) -> Self {
        let raw = match std::fs::read_to_string(path) {
            Ok(raw) => raw,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!(
                    "[settings] failed to read {}: {e}; using defaults",
                    path.display()
                );
                return Self::default();
            }
        };

        let json = crate::config::strip_jsonc_comments(&raw);
        match serde_json::from_str(&json) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!(
                    "[settings] corrupt settings file {}: {e}; using defaults",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Save settings to `.axonerai/settings.jsonc` as pretty JSON (plain JSON
    /// is valid JSONC).
    pub fn save(&self) -> Result<()> {
        self.save_to(Path::new(DEFAULT_SETTINGS_PATH))
    }

    /// Save settings to an explicit path (used by tests and tooling).
    pub fn save_to(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(path, json)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path(tag: &str) -> PathBuf {
        let n = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "axonerai-settings-{}-{}-{tag}.jsonc",
            std::process::id(),
            n
        ))
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let settings = Settings {
            suppressed_tools: vec!["WebSearch".to_string(), "tavily_search".to_string()],
        };
        settings.save_to(&path).expect("save should succeed");

        let loaded = Settings::load_from(&path);
        assert_eq!(
            loaded.suppressed_tools,
            vec!["WebSearch".to_string(), "tavily_search".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn missing_file_yields_defaults() {
        let path = temp_path("missing");
        let loaded = Settings::load_from(&path);
        assert!(loaded.suppressed_tools.is_empty());
    }

    #[test]
    fn jsonc_comments_are_stripped_on_load() {
        let path = temp_path("comments");
        std::fs::write(
            &path,
            r#"{
                // Tool suppression list.
                "suppressed_tools": [
                    "WebSearch", // disabled by the UI panel
                    /* block comment */
                    "tavily_search"
                ]
            }"#,
        )
        .expect("write test file");

        let loaded = Settings::load_from(&path);
        assert_eq!(
            loaded.suppressed_tools,
            vec!["WebSearch".to_string(), "tavily_search".to_string()]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_file_yields_defaults() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{ not valid json !!!").expect("write test file");

        let loaded = Settings::load_from(&path);
        assert!(loaded.suppressed_tools.is_empty(), "corrupt → defaults");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_creates_missing_parent_dirs() {
        let base = temp_path("nested");
        let nested = base.join("sub").join("settings.jsonc");
        let settings = Settings {
            suppressed_tools: vec!["WebFetch".to_string()],
        };
        settings.save_to(&nested).expect("save should create dirs");
        assert!(nested.exists());
        let loaded = Settings::load_from(&nested);
        assert_eq!(loaded.suppressed_tools, vec!["WebFetch".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }
}
