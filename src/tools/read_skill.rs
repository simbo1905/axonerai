//! item49 + item50 — the read-only `ReadSkill` builtin: how a chat agent
//! loads a skill body.
//!
//! Resolution matches the /api/skills listing (src/skills.rs): a skill is
//! `<name>/SKILL.md` under `.axonerai/skills` (repo-local) or
//! `~/.axonerai/skills` (user fallback), LOCAL MASKS USER MASKS BUILTIN —
//! built-in skills (item50, src/builtin_skills.rs) ship in the binary and
//! are resolved last. Discovery is deliberately composable: the agent can
//! `ListDir` on `.axonerai/skills` to learn the folder-skill names, then
//! `ReadSkill` one of them (built-ins are not folders — /api/skills lists
//! them too). A missing or broken skill is an `Ok("Error: ...")` result
//! (fed back to the model), never a run abort. Read-only by default, so it
//! survives the `--tools-readonly` registry filter.

use crate::skills::{self, local_dir, user_dir};
use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::path::PathBuf;

/// Read-only skill-body reader over the two skills dirs.
pub struct ReadSkill {
    local: PathBuf,
    user: PathBuf,
}

impl Default for ReadSkill {
    fn default() -> Self {
        Self::new(local_dir(), user_dir())
    }
}

impl ReadSkill {
    pub fn new(local: PathBuf, user: PathBuf) -> Self {
        Self { local, user }
    }
}

#[async_trait]
impl Tool for ReadSkill {
    fn name(&self) -> String {
        "ReadSkill".to_string()
    }

    fn description(&self) -> String {
        "Reads the SKILL.md body of one named skill (read-only, cannot write). Skills live as <name>/SKILL.md under .axonerai/skills (local, wins) or ~/.axonerai/skills (fallback); same-named built-in skills shipped in the binary are used last. To discover folder skill names, ListDir .axonerai/skills first. Usage: {\"name\": \"greeting\"}".to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Skill name (the folder under .axonerai/skills, e.g. \"lint\"); no slashes or '..'"
                }
            },
            "required": ["name"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing name"))?;
        Ok(
            skills::read_skill_from(&self.local, &self.user, name)
                .unwrap_or_else(|message| message),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::fs;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Isolated skills fixture: temp root with `.axonerai/skills` plus a
    /// stand-in `home/.axonerai/skills` user dir.
    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "axonerai-read-skill-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".axonerai/skills")).expect("create local skills");
        fs::create_dir_all(root.join("home/.axonerai/skills")).expect("create user skills");
        root
    }

    fn skill_md(name: &str, description: &str, body: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n{body}\n")
    }

    fn seed(dir: &Path, name: &str, content: &str) {
        let skill_dir = dir.join(name);
        fs::create_dir_all(&skill_dir).expect("create skill dir");
        fs::write(skill_dir.join("SKILL.md"), content).expect("write SKILL.md");
    }

    fn tool(root: &Path) -> ReadSkill {
        ReadSkill::new(
            root.join(".axonerai/skills"),
            root.join("home/.axonerai/skills"),
        )
    }

    #[tokio::test]
    async fn reads_a_local_skill_body() {
        let root = temp_root("local");
        seed(
            &root.join(".axonerai/skills"),
            "greeting",
            &skill_md(
                "greeting",
                "greet politely",
                "Greetings from the axonerai skill system.",
            ),
        );
        let out = tool(&root)
            .execute(json!({"name": "greeting"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("Greetings from the axonerai skill system."),
            "got: {out}"
        );
        assert!(
            out.starts_with("---"),
            "the full SKILL.md (frontmatter included)"
        );
    }

    #[tokio::test]
    async fn local_masks_user() {
        let root = temp_root("mask");
        seed(
            &root.join(".axonerai/skills"),
            "shared",
            &skill_md("shared", "local", "LOCAL BODY"),
        );
        seed(
            &root.join("home/.axonerai/skills"),
            "shared",
            &skill_md("shared", "user", "USER BODY"),
        );
        let out = tool(&root)
            .execute(json!({"name": "shared"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("LOCAL BODY"), "local must win: {out}");
        assert!(!out.contains("USER BODY"), "user must be masked: {out}");
    }

    #[tokio::test]
    async fn falls_back_to_user_when_local_missing() {
        let root = temp_root("fallback");
        seed(
            &root.join("home/.axonerai/skills"),
            "useronly",
            &skill_md("useronly", "user only", "USER ONLY BODY"),
        );
        let out = tool(&root)
            .execute(json!({"name": "useronly"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("USER ONLY BODY"), "got: {out}");
    }

    #[tokio::test]
    async fn missing_skill_is_an_error_result_not_an_abort() {
        let root = temp_root("missing");
        let out = tool(&root)
            .execute(json!({"name": "nope"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.starts_with("Error:") && out.contains("no skill named 'nope'"),
            "got: {out}"
        );
    }

    #[tokio::test]
    async fn traversal_names_are_rejected() {
        let root = temp_root("traversal");
        let out = tool(&root)
            .execute(json!({"name": "../escape"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.starts_with("Error:") && out.contains("invalid skill name"),
            "got: {out}"
        );
    }

    #[tokio::test]
    async fn missing_input_name_is_an_execute_error() {
        let root = temp_root("no-input");
        let result = tool(&root).execute(json!({})).await;
        assert!(result.is_err(), "missing name must error: {result:?}");
    }

    #[tokio::test]
    async fn builtin_skills_resolve_through_the_tool() {
        // item50: with both folder dirs empty, the built-in deepresearch
        // resolves (source builtin); the module doc's "reserved" note is gone.
        let root = temp_root("builtin");
        let out = tool(&root)
            .execute(json!({"name": "deepresearch"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("name: deepresearch") && out.contains("tavily_search"),
            "the builtin SKILL.md body is returned: {out}"
        );
    }

    #[test]
    fn read_only_by_default_and_honest_description() {
        let tool = ReadSkill::default();
        assert!(tool.is_read_only(), "ReadSkill must be read-only");
        assert!(
            tool.description().contains("read-only") && tool.description().contains("cannot write"),
            "description must be honest: {}",
            tool.description()
        );
    }
}
