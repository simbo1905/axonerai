//! item50 — built-in skills ship IN THE CODE.
//!
//! Each built-in skill is a `skills/builtin/<name>/SKILL.md` at the repo
//! root, embedded at compile time with `include_str!` (the same pattern as
//! the embedded default prompt in src/prompt.rs), so the skills work with
//! empty `.axonerai/skills` folders — they ship with the binary. Folder
//! skills shadow same-named built-ins (LOCAL MASKS USER MASKS BUILTIN, see
//! src/skills.rs); the *prompt patches* of built-in skills are composed by
//! src/prompt.rs at agent-build time in name-sorted order.
//!
//! Built-in content is compile-time code, not runtime data: a broken
//! frontmatter here is a programming bug, so the tests assert every
//! listed built-in parses and names are unique rather than silently
//! skipping entries.

use crate::skills::{
    PromptPatch, SkillEntry, SkillSource, parse_frontmatter, parse_frontmatter_fields,
};

/// The compile-time list of built-in skills: `(repo path, SKILL.md content)`.
pub fn builtin_skills() -> &'static [(&'static str, &'static str)] {
    &[(
        "skills/builtin/deepresearch/SKILL.md",
        include_str!("../skills/builtin/deepresearch/SKILL.md"),
    )]
}

/// The /api/skills listing rows for the built-in skills (source `builtin`).
pub fn entries() -> Vec<SkillEntry> {
    builtin_skills()
        .iter()
        .filter_map(|(path, content)| {
            let (name, description) = parse_frontmatter(content)?;
            Some(SkillEntry {
                name,
                description,
                source: SkillSource::Builtin,
                path: (*path).to_string(),
            })
        })
        .collect()
}

/// Resolve one built-in skill by frontmatter name → its full SKILL.md
/// content (frontmatter included). `None` when no built-in has that name.
pub fn read_builtin(name: &str) -> Option<&'static str> {
    builtin_skills()
        .iter()
        .find(|(_, content)| {
            parse_frontmatter(content)
                .map(|(n, _)| n == name)
                .unwrap_or(false)
        })
        .map(|(_, content)| *content)
}

/// The system-prompt patches of every built-in skill that declares one,
/// name-sorted for deterministic composition (src/prompt.rs). Skills with
/// no patch fields are absent from the list — they are inert.
pub fn prompt_patches() -> Vec<(String, PromptPatch)> {
    prompt_patches_excluding(&std::collections::HashSet::new())
}

/// item54: [`prompt_patches`] with builtin-skill deactivation — a name in
/// `disabled` drops that builtin's patch from composition (its /api/skills
/// listing row is dropped separately, see src/skills.rs). Names that match
/// no builtin are inert.
pub fn prompt_patches_excluding(
    disabled: &std::collections::HashSet<String>,
) -> Vec<(String, PromptPatch)> {
    let mut patches: Vec<(String, PromptPatch)> = builtin_skills()
        .iter()
        .filter_map(|(_, content)| {
            let (name, _) = parse_frontmatter(content)?;
            if disabled.contains(&name) {
                return None;
            }
            let patch = PromptPatch::from_fields(&parse_frontmatter_fields(content)?);
            if patch.is_empty() {
                None
            } else {
                Some((name, patch))
            }
        })
        .collect();
    patches.sort_by(|a, b| a.0.cmp(&b.0));
    patches
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_skill_has_valid_unique_frontmatter() {
        let skills = builtin_skills();
        assert!(!skills.is_empty(), "at least the flagship skill ships");
        let mut names = Vec::new();
        for (path, content) in skills {
            let (name, description) = parse_frontmatter(content)
                .unwrap_or_else(|| panic!("invalid frontmatter in {path}"));
            assert!(!description.is_empty(), "{name}: empty description");
            assert!(content.contains(&format!("name: {name}")), "{path}");
            names.push(name);
        }
        let unique = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), unique, "built-in names must be unique");
    }

    #[test]
    fn entries_point_at_their_repo_paths_with_builtin_source() {
        for entry in entries() {
            assert_eq!(entry.source, crate::skills::SkillSource::Builtin);
            let skills = builtin_skills()
                .iter()
                .find(|(path, _)| *path == entry.path)
                .expect("entry path matches a compiled-in path");
            assert!(skills.1.contains(&format!("name: {}", entry.name)));
        }
    }

    #[test]
    fn deepresearch_resolves_as_a_builtin() {
        let body = read_builtin("deepresearch").expect("the flagship builtin resolves");
        assert!(body.contains("# Deep research"), "got: {body}");
        assert!(body.contains("tavily_search") && body.contains("WebFetch"));
        assert!(read_builtin("no-such-builtin").is_none());
    }

    #[test]
    fn deepresearch_declares_a_prompt_append() {
        let patches = prompt_patches();
        let (name, patch) = patches
            .iter()
            .find(|(name, _)| name == "deepresearch")
            .expect("deepresearch has a patch");
        assert_eq!(name, "deepresearch");
        assert!(patch.replace.is_none() && patch.prepend.is_none());
        let append = patch.append.as_deref().expect("append set");
        assert!(
            append.contains("deep research mode") && append.contains("cite"),
            "modest deep-research append: {append}"
        );
        // Name-sorted: deterministic composition order.
        let mut sorted = patches.clone();
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(patches, sorted);
    }

    // ------------------------------------------------------- item54: toggles

    #[test]
    fn disabled_builtin_is_dropped_from_patch_composition() {
        let disabled: std::collections::HashSet<String> =
            ["deepresearch"].into_iter().map(str::to_string).collect();
        assert!(
            prompt_patches_excluding(&disabled).is_empty(),
            "the only builtin is disabled → no patches compose"
        );

        // An unknown name is inert: the full patch list is untouched.
        let unknown: std::collections::HashSet<String> =
            ["no-such-skill"].into_iter().map(str::to_string).collect();
        assert_eq!(
            prompt_patches_excluding(&unknown),
            prompt_patches(),
            "disabling a nonexistent skill changes nothing"
        );

        // Case: the unfiltered list is the empty-disabled default.
        assert_eq!(
            prompt_patches_excluding(&Default::default()),
            prompt_patches()
        );
    }
}
