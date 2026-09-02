//! System prompt loading.
//!
//! At startup the server uses `prompts/generated/<provider>--<model>.txt` if it
//! exists, else the embedded default. Generated prompts are composed at build
//! time from `prompts/base.txt` plus per-model patches via `make prompts`
//! (`node prompts/build.mjs`); `make prompts` must run before `cargo build`
//! because the default prompt is embedded at compile time via `include_str!`.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const DEFAULT_SYSTEM_PROMPT: &str = include_str!("../prompts/generated/default.txt");

/// Filesystem path of the generated prompt for a provider+model pair.
/// Any `/` in the model id is replaced with `_`, matching the filename
/// convention used by the prompt composer (`prompts/build.mjs`).
fn prompt_path(provider: &str, model: &str) -> String {
    let model = model.replace('/', "_");
    Path::new("prompts")
        .join("generated")
        .join(format!("{provider}--{model}.txt"))
        .to_string_lossy()
        .into_owned()
}

/// Load the system prompt for a provider+model pair: the generated
/// `prompts/generated/<provider>--<model>.txt` when present, else the embedded
/// default prompt.
pub fn load_system_prompt(provider: &str, model: &str) -> String {
    let path = prompt_path(provider, model);
    if let Ok(contents) = std::fs::read_to_string(&path) {
        return contents;
    }
    DEFAULT_SYSTEM_PROMPT.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Removes the probe file when dropped, even if an assertion panics.
    struct Cleanup(String);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    #[test]
    fn default_prompt_is_embedded() {
        let prompt = load_system_prompt("no-such-provider", "no-such-model");
        assert!(prompt.contains("helpful assistant"));
    }

    #[test]
    fn model_slashes_map_to_underscores() {
        assert_eq!(
            prompt_path("groq", "openai/gpt-oss-20b"),
            "prompts/generated/groq--openai_gpt-oss-20b.txt"
        );
        assert_eq!(
            prompt_path("mistral", "zai-glm-5-2"),
            "prompts/generated/mistral--zai-glm-5-2.txt"
        );
    }

    #[test]
    fn on_disk_override_wins_over_default() {
        // Unique name so parallel test runs never collide; removed on drop.
        let path = "prompts/generated/test-item19-provider--test_model.txt";
        let contents = "UNIQUE PROBE PROMPT FOR ITEM19 TESTS";
        // Set only if not already present; the atomic guards the cleanup flag
        // so a pre-existing file is never deleted by this test.
        static EXISTED: AtomicBool = AtomicBool::new(false);
        if Path::new(path).exists() {
            EXISTED.store(true, Ordering::SeqCst);
        }
        let _cleanup = Cleanup(path.to_string());
        fs::write(path, contents).expect("write probe prompt");
        assert_eq!(
            load_system_prompt("test-item19-provider", "test_model"),
            contents
        );
        if EXISTED.load(Ordering::SeqCst) {
            EXISTED.store(false, Ordering::SeqCst);
        }
    }
}
