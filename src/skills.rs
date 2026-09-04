//! item49 — skills folder resolution + listing + skill-body reading.
//!
//! A "skill" is a directory `<name>/SKILL.md` under either `.axonerai/skills/`
//! (repo-local, shareable) or `~/.axonerai/skills/` (user fallback). LOCAL
//! MASKS USER — the same precedence as the per-provider models config
//! (src/models_config.rs). The SKILL.md frontmatter is the official format: a
//! `---` fenced block of `key: value` lines carrying at least `name` and
//! `description`.
//!
//! Everything here is missing-safe and never crashes on broken content: a
//! missing skills dir yields no entries; an unreadable or unparsable SKILL.md
//! is skipped with a one-line stderr note. The `Builtin` source variant is
//! RESERVED (item50 will serve in-code built-in skills even with empty
//! folders) — nothing lists or resolves a builtin skill yet.

use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where a listed skill comes from. `Builtin` is reserved for item50 (skills
/// shipped in code, servable with empty folders); the current listing only
/// ever produces `local` / `user` entries.
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

/// Parse the official `---`-fenced `key: value` frontmatter of a SKILL.md.
/// Returns `(name, description)`; `None` when the fence is missing, the
/// block has no closing fence, or either required key is absent/blank.
/// Unknown keys are tolerated.
pub fn parse_frontmatter(content: &str) -> Option<(String, String)> {
    let mut lines = content.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut name: Option<String> = None;
    let mut description: Option<String> = None;
    for line in lines {
        let trimmed = line.trim();
        if trimmed == "---" {
            break;
        }
        if let Some((key, value)) = trimmed.split_once(':') {
            let value = value.trim();
            match key.trim() {
                "name" if !value.is_empty() => name = Some(value.to_string()),
                "description" if !value.is_empty() => description = Some(value.to_string()),
                _ => {}
            }
        }
    }
    Some((name?, description?))
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

/// List all skills from both candidate dirs, LOCAL MASKS USER. A name that
/// exists in both dirs appears once, sourced `local`. Missing dirs are
/// empty; the result is sorted by name for a deterministic listing.
pub fn list_skills_from(local: &Path, user: &Path) -> Vec<SkillEntry> {
    let local_entries = list_dir_skills(local, SkillSource::Local);
    let local_names: HashSet<String> = local_entries
        .iter()
        .map(|entry| entry.name.clone())
        .collect();
    let mut all = local_entries;
    for entry in list_dir_skills(user, SkillSource::User) {
        if !local_names.contains(&entry.name) {
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

/// Resolve one skill by name against both dirs, LOCAL MASKS USER, and
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
    Err(format!(
        "Error: no skill named '{name}' (looked in .axonerai/skills and ~/.axonerai/skills; use ListDir on .axonerai/skills to discover names)"
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
        // nonexistent root is also empty.
        assert!(
            list_skills_from(&root.join(".axonerai/skills"), &root.join("nope/skills")).is_empty()
        );
        let nowhere =
            std::env::temp_dir().join(format!("axonerai-skills-nowhere-{}", std::process::id()));
        assert!(list_skills_from(&nowhere, &root.join(".axonerai/skills")).is_empty());
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
        assert_eq!(names, vec!["deploy", "greeting", "lint"], "sorted by name");
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
        assert_eq!(skills.len(), 1, "masked: one entry");
        assert_eq!(skills[0].source, SkillSource::Local);
        assert_eq!(skills[0].description, "the local one");
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
        let names: Vec<&str> = skills.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["good"], "broken entries skipped, good one kept");
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
}
