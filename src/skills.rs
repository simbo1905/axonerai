//! item49 + item50 — skills folder resolution + listing + skill-body reading.
//!
//! A "skill" is a directory `<name>/SKILL.md` under either `.axonerai/skills/`
//! (repo-local, shareable) or `~/.axonerai/skills/` (user fallback). LOCAL
//! MASKS USER MASKS BUILTIN — the same precedence as the per-provider models
//! config (src/models_config.rs); built-in skills ship IN THE CODE (item50,
//! src/builtin_skills.rs) and are shadowed by same-named folder skills. The
//! SKILL.md frontmatter is the official format: a `---` fenced block of
//! `key: value` lines carrying at least `name` and `description`.
//!
//! Everything here is missing-safe and never crashes on broken content: a
//! missing skills dir yields no entries; an unreadable or unparsable SKILL.md
//! is skipped with a one-line stderr note. A broken BUILT-IN skill is a
//! compile-time bug caught by the builtin_skills tests, not runtime data.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a listed skill comes from: a repo-local folder, the user folder, or
/// shipped in the binary (src/builtin_skills.rs).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SkillSource {
    Local,
    User,
    Builtin,
}

impl SkillSource {
    pub fn as_str(self) -> &'static str {
        match self {
            SkillSource::Local => "local",
            SkillSource::User => "user",
            SkillSource::Builtin => "builtin",
        }
    }
}

/// One listed skill: frontmatter name/description plus where it came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SkillEntry {
    pub name: String,
    pub description: String,
    pub source: SkillSource,
    /// The SKILL.md file backing this entry.
    pub path: String,
}

/// The local skills dir: `.axonerai/skills` under the current directory.
pub fn local_dir() -> PathBuf {
    PathBuf::from(".axonerai/skills")
}

/// The user skills dir: `~/.axonerai/skills`.
pub fn user_dir() -> PathBuf {
    match std::env::var_os("HOME") {
        Some(home) => Path::new(&home).join(".axonerai/skills"),
        None => PathBuf::from(""),
    }
}

/// Parse every `key: value` line of the official `---`-fenced frontmatter,
/// in file order. Returns `None` when the fence is missing or unclosed.
/// Unknown keys are tolerated; duplicate keys keep their last value when
/// consumed by `PromptPatch::from_fields` (single-line values only — the
/// format is flat `key: value` lines, same as item49).
pub fn parse_frontmatter_fields(content: &str) -> Option<Vec<(String, String)>> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut fields = Vec::new();
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            return Some(fields);
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            fields.push((key.trim().to_string(), value.trim().to_string()));
        }
    }
    None
}

/// Parse the official frontmatter's two required keys: `(name, description)`.
/// `None` when the fence is missing, the block has no closing fence, or
/// either required key is absent/blank.
pub fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let fields = parse_frontmatter_fields(content)?;
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for (key, value) in &fields {
        match key.as_str() {
            "name" if !value.is_empty() => name = Some(value.clone()),
            "description" if !value.is_empty() => description = Some(value.clone()),
            _ => {}
        }
    }
    Some((name?, description?))
}

/// A skill-declared system-prompt patch (item50, frontmatter-only): the
/// optional `system-prompt-prepend`, `system-prompt-append` and
/// `system-prompt-replace` SKILL.md fields. Only built-in skills' patches
/// are composed (src/prompt.rs, at agent-build time); a skill with none of
/// these fields parses to an empty, inert patch.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PromptPatch {
    pub prepend: Option<String>,
    pub append: Option<String>,
    pub replace: Option<String>,
}

impl PromptPatch {
    /// Extract the three patch fields from parsed frontmatter fields.
    /// Unrelated keys are ignored; blank values never count.
    pub fn from_fields(fields: &[(String, String)]) -> PromptPatch {
        let mut patch = PromptPatch::default();
        for (key, value) in fields {
            if value.is_empty() {
                continue;
            }
            match key.as_str() {
                "system-prompt-prepend" => patch.prepend = Some(value.clone()),
                "system-prompt-append" => patch.append = Some(value.clone()),
                "system-prompt-replace" => patch.replace = Some(value.clone()),
                _ => {}
            }
        }
        patch
    }

    /// A patch with no fields at all composes nothing.
    pub fn is_empty(&self) -> bool {
        self.prepend.is_none() && self.append.is_none() && self.replace.is_none()
    }

    /// Apply this patch to a prompt. `replace` wins outright (it replaces
    /// the whole prompt, including this skill's own prepend/append);
    /// otherwise prepend goes above and append goes below, joined by a
    /// blank line.
    pub fn apply_to(&self, prompt: &str) -> String {
        if let Some(replace) = &self.replace {
            return replace.clone();
        }
        let mut out = prompt.to_string();
        if let Some(prepend) = &self.prepend {
            out = format!("{prepend}\n\n{out}");
        }
        if let Some(append) = &self.append {
            out = format!("{out}\n\n{append}");
        }
        out
    }
}

/// List every `<name>/SKILL.md` under one skills dir. A missing or
/// unreadable dir is empty (missing-safe); broken SKILL.md files are skipped
/// with a stderr note (never a crash).
fn list_dir_skills(dir: &Path, source: SkillSource) -> Vec<SkillEntry> {
    let mut entries = Vec::new();
    let Ok(dirs) = std::fs::read_dir(dir) else {
        return entries;
    };
    for entry in dirs {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let skill_file = path.join("SKILL.md");
        let content = match std::fs::read_to_string(&skill_file) {
            Ok(content) => content,
            Err(e) => {
                eprintln!(
                    "axonerai: skipping unreadable skill {}: {e}",
                    skill_file.display()
                );
                continue;
            }
        };
        match parse_frontmatter(&content) {
            Some((name, description)) => entries.push(SkillEntry {
                name,
                description,
                source,
                path: skill_file.display().to_string(),
            }),
            None => {
                eprintln!(
                    "axonerai: skipping skill with invalid frontmatter: {}",
                    skill_file.display()
                );
            }
        }
    }
    entries
}

/// List all skills from both candidate dirs plus the built-in skills, LOCAL
/// MASKS USER MASKS BUILTIN. A name that exists at a higher-precedence
/// source appears once, sourced from the winner. Missing dirs are empty;
/// the result is sorted by name for a deterministic listing.
pub fn list_skills_from(local: &Path, user: &Path) -> Vec<SkillEntry> {
    list_skills_from_filtered(local, user, &HashSet::new())
}

/// item54: [`list_skills_from`] with builtin-source deactivation. A name in
/// `disabled` drops entries whose source is [`SkillSource::Builtin`] ONLY —
/// local/user folder skills of the same name are unaffected (a folder skill
/// shadowing a disabled builtin stays listed). Disabled names that match no
/// builtin are inert.
pub fn list_skills_from_filtered(
    local: &Path,
    user: &Path,
    disabled: &HashSet<String>,
) -> Vec<SkillEntry> {
    let local_entries = list_dir_skills(local, SkillSource::Local);
    let local_names: HashSet<String> = local_entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let mut all = local_entries;
    for entry in list_dir_skills(user, SkillSource::User) {
        if !local_names.contains(&entry.name) {
            all.push(entry.clone());
        }
    }
    let taken: HashSet<String> = all.iter().map(|entry| entry.name.clone()).collect();
    for entry in crate::builtin_skills::entries() {
        if !taken.contains(&entry.name) && !disabled.contains(&entry.name) {
            all.push(entry);
        }
    }
    all.sort_by(|a, b| a.name.cmp(&b.name));
    all
}

/// Convenience: list skills from the default dirs (cwd-local + `~`).
pub fn list_skills() -> Vec<SkillEntry> {
    list_skills_from(&local_dir(), &user_dir())
}

/// item54: [`list_skills`] with builtin-source deactivation (see
/// [`list_skills_from_filtered`]) — the /api/skills listing path.
pub fn list_skills_filtered(disabled: &HashSet<String>) -> Vec<SkillEntry> {
    list_skills_from_filtered(&local_dir(), &user_dir(), disabled)
}

/// Resolve one skill by name: `.axonerai/skills`, then `~/.axonerai/skills`,
/// then the built-in skills (item50) — LOCAL MASKS USER MASKS BUILTIN — and
/// return the SKILL.md content. `Err` carries the user-facing error message
/// (surfaced as `Ok("Error: ...")` by the ReadSkill tool, per the pre-jail
/// tools' convention). Missing dirs are missing-safe: the error says so
/// rather than crashing.
pub fn read_skill_from(local: &Path, user: &Path, name: &str) -> Result<String, String> {
    if name.trim().is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(format!("Error: invalid skill name '{name}'"));
    }
    let skill_file = |dir: &Path| dir.join(name).join("SKILL.md");
    for (dir, source) in [(local, "local"), (user, "user")] {
        let path = skill_file(dir);
        match std::fs::read_to_string(&path) {
            Ok(content) => {
                if parse_frontmatter(&content).is_none() {
                    return Err(format!(
                        "Error: the {source} skill '{name}' has invalid frontmatter: {}",
                        path.display()
                    ));
                }
                return Ok(content);
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!(
                    "Error: cannot read the {source} skill '{name}': {e}"
                ));
            }
            _ => {}
        }
    }
    if let Some(content) = crate::builtin_skills::read_builtin(name) {
        return Ok(content.to_string());
    }
    Err(format!(
        "Error: no skill named '{name}' (looked in .axonerai/skills, ~/.axonerai/skills and the built-in skills; use ListDir on .axonerai/skills to discover the folder skills)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Isolated skills-dir fixture: temp "repo" root with `.axonerai/skills`.
    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!(
            "axonerai-skills-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(root.join(".axonerai/skills")).expect("create skills dir");
        root
    }

    fn write_skill(root: &Path, which: &str, name: &str, content: &str) {
        let dir = root.join(".axonerai").join(which).join("skills").join(name);
        fs::create_dir_all(&dir).expect("create skill dir");
        fs::write(dir.join("SKILL.md"), content).expect("write SKILL.md");
    }

    fn skill_md(name: &str, description: &str) -> String {
        format!("---\nname: {name}\ndescription: {description}\n---\n\n# {name}\n\nBody.\n")
    }

    #[test]
    fn frontmatter_parses_name_and_description() {
        let (name, description) =
            parse_frontmatter("---\nname: lint\ndescription: Grade the repo.\n---\n\n# Lint\n")
                .expect("parses");
        assert_eq!(name, "lint");
        assert_eq!(description, "Grade the repo.");
    }

    #[test]
    fn frontmatter_tolerates_unknown_keys_and_extra_colons() {
        let (name, description) =
            parse_frontmatter("---\nunknown: x\nname: a\ndescription: b: c\n---\n\nbody")
                .expect("parses");
        assert_eq!(name, "a");
        assert_eq!(description, "b: c");
    }

    #[test]
    fn frontmatter_rejects_missing_fence_or_fields() {
        assert!(parse_frontmatter("no fence at all").is_none());
        assert!(
            parse_frontmatter("---\nname: a\n\nbody").is_none(),
            "no closing fence"
        );
        assert!(
            parse_frontmatter("---\nname: a\n---\n").is_none(),
            "no description"
        );
        assert!(
            parse_frontmatter("---\ndescription: a\n---\n").is_none(),
            "no name"
        );
        assert!(
            parse_frontmatter("---\nname:\ndescription: a\n---\n").is_none(),
            "blank name"
        );
    }

    #[test]
    fn missing_dirs_are_empty() {
        let root = temp_root("missing");
        // The skills dirs exist (fixture) but contain nothing; then a
        // nonexistent root is also missing-safe. item50: even with BOTH
        // folders empty the built-in skills are still listed.
        let skills = list_skills_from(&root.join(".axonerai/skills"), &root.join("nope/skills"));
        assert_eq!(
            skills.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
            vec!["deepresearch"],
            "only the builtin remains"
        );
        let nowhere =
            std::env::temp_dir().join(format!("axonerai-skills-nowhere-{}", std::process::id()));
        assert!(
            list_skills_from(&nowhere, &root.join(".axonerai/skills"))
                .iter()
                .all(|s| s.source == SkillSource::Builtin),
            "missing dirs contribute nothing beyond the builtins"
        );
    }

    #[test]
    fn local_and_user_both_listed_when_names_differ() {
        let root = temp_root("both");
        write_skill(&root, "", "lint", &skill_md("lint", "grade"));
        write_skill(&root, "", "greeting", &skill_md("greeting", "greet"));
        // The real user dir is ~/.axonerai/skills; tests pass explicit dirs,
        // so seed a stand-in home dir instead.
        let user_dir = root.join("home/.axonerai/skills");
        fs::create_dir_all(user_dir.join("deploy")).expect("seed user skill");
        fs::write(
            user_dir.join("deploy/SKILL.md"),
            skill_md("deploy", "ship it"),
        )
        .expect("write user skill");

        let skills = list_skills_from(&root.join(".axonerai/skills"), &user_dir);
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["deepresearch", "deploy", "greeting", "lint"],
            "sorted by name (builtin included)"
        );
        let by_name = |name: &str| {
            skills
                .iter()
                .find(|s| s.name == name)
                .unwrap_or_else(|| panic!("{name} missing"))
        };
        assert_eq!(by_name("lint").source, SkillSource::Local);
        assert_eq!(by_name("greeting").source, SkillSource::Local);
        assert_eq!(by_name("deploy").source, SkillSource::User);
        assert_eq!(by_name("deploy").description, "ship it");
        assert_eq!(by_name("deepresearch").source, SkillSource::Builtin);
        assert!(by_name("deploy").path.ends_with("SKILL.md"));
    }

    #[test]
    fn local_masks_user_for_the_same_name() {
        let root = temp_root("mask");
        let local_dir = root.join(".axonerai/skills");
        fs::create_dir_all(local_dir.join("shared")).expect("seed local");
        fs::write(
            local_dir.join("shared/SKILL.md"),
            skill_md("shared", "the local one"),
        )
        .expect("write local");
        let user_dir = root.join("home/.axonerai/skills");
        fs::create_dir_all(user_dir.join("shared")).expect("seed user");
        fs::write(
            user_dir.join("shared/SKILL.md"),
            skill_md("shared", "the user one"),
        )
        .expect("write user");

        let skills = list_skills_from(&local_dir, &user_dir);
        let shared: Vec<_> = skills.iter().filter(|s| s.name == "shared").collect();
        assert_eq!(shared.len(), 1, "masked: one entry");
        assert_eq!(shared[0].source, SkillSource::Local);
        assert_eq!(shared[0].description, "the local one");
    }

    #[test]
    fn broken_skill_files_are_skipped_not_fatal() {
        let root = temp_root("broken");
        let local_dir = root.join(".axonerai/skills");
        fs::create_dir_all(local_dir.join("good")).expect("seed good");
        fs::write(local_dir.join("good/SKILL.md"), skill_md("good", "fine")).expect("good");
        // No SKILL.md at all.
        fs::create_dir_all(local_dir.join("empty")).expect("seed empty dir");
        // Unparsable frontmatter.
        fs::create_dir_all(local_dir.join("fenceless")).expect("seed fenceless");
        fs::write(local_dir.join("fenceless/SKILL.md"), "just text\n").expect("fenceless");

        let skills = list_skills_from(&local_dir, &root.join("nowhere"));
        let names: Vec<&str> = skills
            .iter()
            .filter(|s| s.source != SkillSource::Builtin)
            .map(|s| s.name.as_str())
            .collect();
        assert_eq!(names, vec!["good"], "broken entries skipped, good one kept");
    }

    // ------------------------------------------------------- item54: toggles

    #[test]
    fn disabled_builtin_dropped_but_folder_skills_unaffected() {
        let root = temp_root("disabled");
        let local_dir = root.join(".axonerai/skills");
        let user_dir = root.join("home/.axonerai/skills");

        let disabled: HashSet<String> = ["deepresearch"].into_iter().map(str::to_string).collect();

        // With empty folders the disabled builtin disappears entirely.
        let skills = list_skills_from_filtered(&local_dir, &user_dir, &disabled);
        assert!(
            !skills.iter().any(|s| s.name == "deepresearch"),
            "disabled builtin must not be listed: {skills:?}"
        );
        // The unfiltered listing still has it (control).
        assert!(
            list_skills_from(&local_dir, &user_dir)
                .iter()
                .any(|s| s.name == "deepresearch")
        );

        // A local folder skill with the SAME name is unaffected: the
        // disabled list only filters builtin-sourced entries.
        write_skill(
            &root,
            "",
            "deepresearch",
            &skill_md("deepresearch", "local one"),
        );
        let skills = list_skills_from_filtered(&local_dir, &user_dir, &disabled);
        let entry = skills
            .iter()
            .find(|s| s.name == "deepresearch")
            .expect("local folder skill unaffected by the disabled builtin");
        assert_eq!(entry.source, SkillSource::Local);
        assert_eq!(entry.description, "local one");

        // Disabling a nonexistent builtin is inert.
        let unknown: HashSet<String> = ["no-such-skill"].into_iter().map(str::to_string).collect();
        assert_eq!(
            list_skills_from_filtered(&local_dir, &user_dir, &unknown),
            list_skills_from(&local_dir, &user_dir)
        );
    }

    #[test]
    fn read_skill_resolves_local_over_user_and_errors_when_missing() {
        let root = temp_root("read");
        let local_dir = root.join(".axonerai/skills");
        write_skill(&root, "", "shared", &skill_md("shared", "local body"));
        let user_dir = root.join("home/.axonerai/skills");
        fs::create_dir_all(user_dir.join("shared")).expect("seed user");
        fs::write(
            user_dir.join("shared/SKILL.md"),
            skill_md("shared", "user body"),
        )
        .expect("write user");
        fs::create_dir_all(user_dir.join("useronly")).expect("seed useronly");
        fs::write(
            user_dir.join("useronly/SKILL.md"),
            skill_md("useronly", "user only"),
        )
        .expect("write useronly");

        let local_body = read_skill_from(&local_dir, &user_dir, "shared").expect("local wins");
        assert!(local_body.contains("local body"), "got: {local_body}");
        assert!(
            local_body.starts_with("---"),
            "the full SKILL.md is returned"
        );

        let user_body = read_skill_from(&local_dir, &user_dir, "useronly").expect("user fallback");
        assert!(user_body.contains("user only"), "got: {user_body}");

        let missing = read_skill_from(&local_dir, &user_dir, "nope").unwrap_err();
        assert!(
            missing.starts_with("Error:") && missing.contains("no skill named 'nope'"),
            "got: {missing}"
        );

        let traversal = read_skill_from(&local_dir, &user_dir, "../escape").unwrap_err();
        assert!(
            traversal.starts_with("Error:") && traversal.contains("invalid skill name"),
            "got: {traversal}"
        );
    }

    // ------------------------------------------------------ item50: builtin

    #[test]
    fn builtin_skills_are_listed_when_folders_are_empty() {
        let root = temp_root("builtin-list");
        let skills = list_skills_from(&root.join(".axonerai/skills"), &root.join("home/skills"));
        let deepresearch = skills
            .iter()
            .find(|s| s.name == "deepresearch")
            .expect("deepresearch listed");
        assert_eq!(deepresearch.source, SkillSource::Builtin);
        assert!(
            deepresearch.path.starts_with("skills/builtin/"),
            "path points at the in-code source: {}",
            deepresearch.path
        );
        assert!(!deepresearch.description.is_empty());
    }

    #[test]
    fn local_masks_user_masks_builtin_for_the_same_name() {
        let root = temp_root("mask-builtin");
        let local_dir = root.join(".axonerai/skills");
        let user_dir = root.join("home/.axonerai/skills");
        fs::create_dir_all(user_dir.join("deepresearch")).expect("seed user");
        fs::write(
            user_dir.join("deepresearch/SKILL.md"),
            skill_md("deepresearch", "the user one"),
        )
        .expect("write user");

        let skills = list_skills_from(&local_dir, &user_dir);
        let entry = skills
            .iter()
            .find(|s| s.name == "deepresearch")
            .expect("listed once");
        assert_eq!(entry.source, SkillSource::User, "user masks builtin");
        assert_eq!(entry.description, "the user one");

        fs::create_dir_all(local_dir.join("deepresearch")).expect("seed local");
        fs::write(
            local_dir.join("deepresearch/SKILL.md"),
            skill_md("deepresearch", "the local one"),
        )
        .expect("write local");
        let skills = list_skills_from(&local_dir, &user_dir);
        let entry = skills
            .iter()
            .find(|s| s.name == "deepresearch")
            .expect("listed once");
        assert_eq!(entry.source, SkillSource::Local, "local masks user");
        assert_eq!(entry.description, "the local one");
    }

    #[test]
    fn read_skill_falls_back_to_builtin_last() {
        let root = temp_root("read-builtin");
        let local_dir = root.join(".axonerai/skills");
        let user_dir = root.join("home/.axonerai/skills");

        let body = read_skill_from(&local_dir, &user_dir, "deepresearch").expect("builtin");
        assert!(
            body.starts_with("---") && body.contains("name: deepresearch"),
            "the full builtin SKILL.md is returned: {body}"
        );

        // A same-named folder skill shadows the builtin.
        write_skill(
            &root,
            "",
            "deepresearch",
            &skill_md("deepresearch", "local"),
        );
        let shadowed = read_skill_from(&local_dir, &user_dir, "deepresearch").expect("local wins");
        assert!(
            shadowed.contains("local") && !shadowed.contains("cite"),
            "the folder skill must win: {shadowed}"
        );

        let missing = read_skill_from(&local_dir, &user_dir, "nope").unwrap_err();
        assert!(
            missing.contains("built-in skills"),
            "the error names all three sources: {missing}"
        );
    }

    #[test]
    fn prompt_patch_parses_frontmatter_fields() {
        let fields = vec![
            ("name".to_string(), "x".to_string()),
            ("system-prompt-prepend".to_string(), "P".to_string()),
            ("system-prompt-append".to_string(), "A".to_string()),
            ("system-prompt-replace".to_string(), "R".to_string()),
            ("system-prompt-prepend".to_string(), "P2".to_string()),
            ("system-prompt-prepend".to_string(), String::new()),
            ("unrelated".to_string(), "v".to_string()),
        ];
        let patch = PromptPatch::from_fields(&fields);
        assert_eq!(patch.prepend.as_deref(), Some("P2"), "last value wins");
        assert_eq!(patch.append.as_deref(), Some("A"));
        assert_eq!(patch.replace.as_deref(), Some("R"));
        assert!(!patch.is_empty());

        let inert = PromptPatch::from_fields(&[("name".to_string(), "x".to_string())]);
        assert!(inert.is_empty(), "no patch fields → inert");
        assert_eq!(inert.apply_to("base"), "base", "an inert patch is a no-op");
    }

    #[test]
    fn prompt_patch_composition_order_is_prepend_base_append() {
        let patch = PromptPatch {
            prepend: Some("BEFORE".to_string()),
            append: Some("AFTER".to_string()),
            replace: None,
        };
        assert_eq!(patch.apply_to("BASE"), "BEFORE\n\nBASE\n\nAFTER");
    }

    #[test]
    fn prompt_patch_replace_wins_over_prepend_and_append() {
        let patch = PromptPatch {
            prepend: Some("BEFORE".to_string()),
            append: Some("AFTER".to_string()),
            replace: Some("REPLACED".to_string()),
        };
        assert_eq!(patch.apply_to("BASE"), "REPLACED");
    }
}
