//! System prompt loading.
//!
//! At startup the server uses `prompts/generated/<provider>--<model>.txt` if it
//! exists, else the embedded default. Generated prompts are composed at build
//! time from `prompts/base.txt` plus per-model patches via `make prompts`
//! (`node prompts/build.mjs`); `make prompts` must run before `cargo build`
//! because the default prompt is embedded at compile time via `include_str!`.
//!
//! item50: on top of the model patch, every built-in skill that declares a
//! frontmatter prompt patch (`system-prompt-prepend` / `-append` / `-replace`,
//! see src/skills.rs) is applied at agent-build time, in deterministic
//! name-sorted order. Built-in skills are always active (they ship with the
//! binary); folder skills (local/user) never contribute patches.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::builtin_skills::prompt_patches;
use crate::skills::PromptPatch;

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

/// Compose a base (model-patched) prompt with skill prompt patches applied
/// in the given order — for item50 that order is the built-in skills,
/// name-sorted. Prepend puts text above, append below (joined by a blank
/// line); replace swaps the whole prompt. No patches → the base unchanged.
pub fn compose_system_prompt(base: &str, patches: &[(String, PromptPatch)]) -> String {
    let mut prompt = base.to_string();
    for (_, patch) in patches {
        prompt = patch.apply_to(&prompt);
    }
    prompt
}

/// Load the system prompt for a provider+model pair: the generated
/// `prompts/generated/<provider>--<model>.txt` when present, else the embedded
/// default prompt — then every built-in skill's prompt patch composed on top,
/// name-sorted (item50).
pub fn load_system_prompt(provider: &str, model: &str) -> String {
    let base = match std::fs::read_to_string(prompt_path(provider, model)) {
        Ok(contents) => contents,
        Err(_) => DEFAULT_SYSTEM_PROMPT.to_string(),
    };
    compose_system_prompt(&base, &prompt_patches())
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
        // The on-disk file is the MODEL-patched BASE; built-in skill patches
        // still compose on top (item50), so assert containment, not equality.
        let loaded = load_system_prompt("test-item19-provider", "test_model");
        assert!(loaded.starts_with(contents), "base wins: {loaded}");
        assert!(
            loaded.ends_with("prefer saved evidence over memory."),
            "skill patch composed after the base: {loaded}"
        );
        if EXISTED.load(Ordering::SeqCst) {
            EXISTED.store(false, Ordering::SeqCst);
        }
    }

    // ------------------------------------------------------ item50: patches

    use crate::skills::PromptPatch;

    fn patch(
        prepend: Option<&str>,
        append: Option<&str>,
        replace: Option<&str>,
    ) -> (String, PromptPatch) {
        (
            String::new(),
            PromptPatch {
                prepend: prepend.map(String::from),
                append: append.map(String::from),
                replace: replace.map(String::from),
            },
        )
    }

    #[test]
    fn compose_applies_patches_in_given_order() {
        let composed = compose_system_prompt(
            "BASE",
            &[
                patch(Some("FIRST PREPEND"), None, None),
                patch(None, Some("FIRST APPEND"), None),
                patch(Some("SECOND PREPEND"), Some("SECOND APPEND"), None),
            ],
        );
        // Sequential, name-sorted composition: later prepends stack above,
        // later appends stack below.
        assert_eq!(
            composed,
            "SECOND PREPEND\n\nFIRST PREPEND\n\nBASE\n\nFIRST APPEND\n\nSECOND APPEND"
        );
    }

    #[test]
    fn compose_with_no_patches_is_inert() {
        assert_eq!(compose_system_prompt("BASE", &[]), "BASE");
        // A no-patch skill contributes an empty entry that changes nothing.
        let inert = vec![("no-patch".to_string(), PromptPatch::default())];
        assert_eq!(compose_system_prompt("BASE", &inert), "BASE");
    }

    #[test]
    fn compose_replace_wins_over_everything_applied_so_far() {
        let composed = compose_system_prompt(
            "BASE",
            &[
                patch(Some("PRE"), Some("APP"), None),
                patch(None, None, Some("REPLACED")),
            ],
        );
        assert_eq!(composed, "REPLACED");
    }

    #[test]
    fn loaded_prompt_includes_the_builtin_skill_append() {
        // Precedence: the model-patched base (here the embedded default)
        // stays intact, and the built-in deepresearch append composes below.
        let prompt = load_system_prompt("no-such-provider", "no-such-model");
        assert!(prompt.contains("helpful assistant"), "base intact");
        assert!(
            prompt.contains("deep research mode"),
            "builtin skill patch composed: {prompt}"
        );
        assert!(
            prompt.ends_with("prefer saved evidence over memory."),
            "the append is last: ...{prompt}"
        );
    }
}
