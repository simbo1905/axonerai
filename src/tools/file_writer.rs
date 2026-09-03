use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::{Component, Path, PathBuf};

#[cfg(feature = "web")]
use tracing::{debug, info};

/// Tool that writes text files inside a dedicated scratch jail directory.
///
/// All writes resolve under the jail root (threaded in at construction; the
/// default is `CWD/.axonerai/scratch`). Relative paths are jailed, parent
/// directories are created on demand, and absolute paths, `..` components and
/// symlink escapes are rejected before anything touches the filesystem.
pub struct WriteFile {
    scratch_root: PathBuf,
}

impl Default for WriteFile {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(cwd.join(".axonerai").join("scratch"))
    }
}

impl WriteFile {
    pub fn new(scratch_root: PathBuf) -> Self {
        Self { scratch_root }
    }

    /// Resolve `path_str` to a path inside the scratch jail. The outer
    /// `Result` carries genuine io failures; the inner one carries a
    /// user-facing rejection message (surfaced as `Ok("Error: ...")` to keep
    /// the pre-jail tool's convention).
    fn resolve_within_jail(&self, path_str: &str) -> Result<Result<PathBuf, String>> {
        let rel = Path::new(path_str);
        if rel.is_absolute() {
            return Ok(Err(
                "Error: For security, absolute paths are not allowed; paths are relative to the agent scratch directory (.axonerai/scratch).".to_string(),
            ));
        }
        if rel.components().any(|c| matches!(c, Component::ParentDir)) {
            return Ok(Err(
                "Error: For security, parent directory traversal (..) is not allowed; paths are relative to the agent scratch directory (.axonerai/scratch).".to_string(),
            ));
        }

        fs::create_dir_all(&self.scratch_root)?;
        let canonical_root = fs::canonicalize(&self.scratch_root)?;

        // Walk up from the target to the deepest ancestor that exists on disk
        // (dangling symlinks count as existing), canonicalise that ancestor,
        // then re-join the not-yet-existing tail. This resolves any symlink
        // hidden in the existing prefix without creating anything outside the
        // jail before the escape check.
        let target = self.scratch_root.join(rel);
        let mut ancestor: &Path = target.as_path();
        let mut tail: Vec<std::ffi::OsString> = Vec::new();
        while !ancestor.exists() && !ancestor.is_symlink() {
            match ancestor.parent() {
                Some(parent) => {
                    tail.push(
                        ancestor
                            .file_name()
                            .expect("non-root paths have a file name")
                            .to_os_string(),
                    );
                    ancestor = parent;
                }
                None => break,
            }
        }
        let canonical_ancestor = match fs::canonicalize(ancestor) {
            Ok(p) => p,
            Err(e) => {
                return Ok(Err(format!(
                    "Error: cannot resolve path inside the agent scratch directory: {e}"
                )));
            }
        };
        let mut canonical_target = canonical_ancestor;
        for part in tail.iter().rev() {
            canonical_target = canonical_target.join(part);
        }
        if !canonical_target.starts_with(&canonical_root) {
            return Ok(Err(
                "Error: path escapes the agent scratch directory; write rejected.".to_string(),
            ));
        }
        Ok(Ok(target))
    }
}

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> String {
        "write_file".to_string()
    }

    fn description(&self) -> String {
        "Writes text to a file under the agent scratch directory (.axonerai/scratch). Paths are relative to that directory; absolute paths and '..' are rejected. Usage: {\"path\": \"example.txt\", \"content\": \"Hello world\"}".to_string()
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to the agent scratch directory (.axonerai/scratch); absolute paths and '..' are rejected"
                },
                "content": {
                    "type": "string",
                    "description": "The text content to write"
                }
            },
            "required": ["path", "content"]
        })
    }

    /// The one write tool: excluded from the future `--tools-readonly` index.
    fn is_read_only(&self) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing content"))?;

        #[cfg(feature = "web")]
        debug!(
            "WriteFile: path={}, content_len={}, scratch_root={:?}",
            path_str,
            content.len(),
            self.scratch_root
        );

        let target = match self.resolve_within_jail(path_str)? {
            Ok(target) => target,
            Err(message) => return Ok(message),
        };

        if let Some(parent) = target.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }

        fs::write(&target, content)?;
        #[cfg(feature = "web")]
        info!(
            "Wrote {} bytes to '{}' under {:?}",
            content.len(),
            path_str,
            self.scratch_root
        );
        Ok(format!(
            "Successfully wrote {} bytes to '{}'",
            content.len(),
            path_str
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Serialises tests that mutate the process CWD (`set_current_dir` is
    /// process-global). Tests that pass an explicit jail root never take this
    /// lock and stay parallel-safe.
    static CWD_LOCK: Mutex<()> = Mutex::new(());
    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    fn temp_root(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!(
            "axonerai-write-file-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp root");
        dir
    }

    /// Restores the process CWD even if an assertion panics mid-test.
    struct CwdGuard(PathBuf);
    impl Drop for CwdGuard {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.0).expect("restore cwd");
        }
    }

    fn jailed_tool(repo: &Path) -> WriteFile {
        WriteFile::new(repo.join(".axonerai").join("scratch"))
    }

    /// The fix for the item37e vulnerability: a relative path must land under
    /// the scratch jail derived from the CWD (the repo root in normal use)
    /// and must NOT land at the CWD root itself.
    #[tokio::test]
    async fn relative_path_is_jailed_under_cwd_scratch_not_cwd_root() {
        let _lock = CWD_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let repo = temp_root("cwd-jail");
        let old = std::env::current_dir().expect("current dir");
        std::env::set_current_dir(&repo).expect("set cwd fixture");
        let _restore = CwdGuard(old);

        let tool = WriteFile::default();
        let out = tool
            .execute(json!({"path": "AGENTS.md", "content": "jail me"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("Successfully"), "unexpected result: {out}");

        let jailed = repo.join(".axonerai/scratch/AGENTS.md");
        assert_eq!(
            fs::read_to_string(&jailed).expect("file must land under the scratch jail"),
            "jail me"
        );
        assert!(
            !repo.join("AGENTS.md").exists(),
            "write escaped the jail and hit the CWD (repo) root"
        );
    }

    #[tokio::test]
    async fn relative_path_lands_under_scratch_root_and_nowhere_else() {
        let repo = temp_root("jail");
        let tool = jailed_tool(&repo);

        let out = tool
            .execute(json!({"path": "notes/hello.txt", "content": "hi"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("Successfully"), "unexpected result: {out}");

        assert_eq!(
            fs::read_to_string(repo.join(".axonerai/scratch/notes/hello.txt")).expect("jailed"),
            "hi"
        );
        assert!(
            !repo.join("notes").exists(),
            "created dirs outside the jail"
        );
        assert!(!repo.join("hello.txt").exists(), "wrote outside the jail");
    }

    #[tokio::test]
    async fn nested_parent_dirs_are_created_on_demand() {
        let repo = temp_root("nested");
        let tool = jailed_tool(&repo);

        let path = "deep/deeper/deepest/file.txt";
        tool.execute(json!({"path": path, "content": "payload"}))
            .await
            .expect("execute succeeds");

        assert_eq!(
            fs::read_to_string(repo.join(".axonerai/scratch").join(path)).expect("jailed"),
            "payload"
        );
    }

    #[tokio::test]
    async fn absolute_path_is_rejected() {
        let repo = temp_root("abs");
        let outside = temp_root("abs-escape").join("victim.txt");
        let tool = jailed_tool(&repo);

        let out = tool
            .execute(json!({"path": outside.to_string_lossy(), "content": "evil"}))
            .await
            .expect("rejections are surfaced as an Error result, not a panic");
        assert!(out.contains("Error"), "expected rejection, got: {out}");
        assert!(!outside.exists(), "absolute path escaped the jail");
    }

    #[tokio::test]
    async fn parent_traversal_is_rejected() {
        let repo = temp_root("dotdot");
        let tool = jailed_tool(&repo);

        for path in ["../escape.txt", "notes/../../escape.txt"] {
            let out = tool
                .execute(json!({"path": path, "content": "evil"}))
                .await
                .expect("rejections are surfaced as an Error result, not a panic");
            assert!(
                out.contains("Error"),
                "expected rejection for {path}: {out}"
            );
            assert!(
                !repo.join("escape.txt").exists(),
                "'{path}' escaped the jail"
            );
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlink_escape_is_rejected() {
        let repo = temp_root("symlink");
        let scratch = repo.join(".axonerai/scratch");
        fs::create_dir_all(&scratch).expect("create scratch jail");
        let outside = temp_root("symlink-outside");
        let tool = jailed_tool(&repo);

        // File symlink pointing at an existing file outside the jail.
        let victim = outside.join("secret.txt");
        fs::write(&victim, "original").expect("seed victim");
        std::os::unix::fs::symlink(&victim, scratch.join("link.txt")).expect("symlink");

        // Directory symlink pointing at a directory outside the jail.
        let outside_dir = outside.join("dir");
        fs::create_dir_all(&outside_dir).expect("seed outside dir");
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, "original").expect("seed outside file");
        std::os::unix::fs::symlink(&outside_dir, scratch.join("outdir")).expect("symlink");

        // Dangling symlink pointing at a not-yet-existing path outside.
        std::os::unix::fs::symlink(outside.join("missing.txt"), scratch.join("dangling"))
            .expect("symlink");

        for (path, _file) in [
            ("link.txt", victim.clone()),
            ("outdir/secret.txt", outside_file.clone()),
            ("dangling", outside.join("missing.txt")),
        ] {
            let out = tool
                .execute(json!({"path": path, "content": "evil"}))
                .await
                .expect("rejections are surfaced as an Error result, not a panic");
            assert!(
                out.contains("Error"),
                "expected rejection for {path}: {out}"
            );
        }

        assert_eq!(
            fs::read_to_string(&victim).expect("victim intact"),
            "original",
            "write escaped through a file symlink"
        );
        assert_eq!(
            fs::read_to_string(&outside_file).expect("outside file intact"),
            "original",
            "write escaped through a directory symlink"
        );
        assert!(
            !outside.join("missing.txt").exists(),
            "dangling symlink escape"
        );
    }

    #[tokio::test]
    async fn overwrite_within_scratch_is_allowed() {
        let repo = temp_root("overwrite");
        let tool = jailed_tool(&repo);
        let input = json!({"path": "file.txt", "content": "first"});

        tool.execute(input).await.expect("first write");
        let out = tool
            .execute(json!({"path": "file.txt", "content": "second"}))
            .await
            .expect("overwrite within the jail is allowed");
        assert!(out.contains("Successfully"), "unexpected result: {out}");
        assert_eq!(
            fs::read_to_string(repo.join(".axonerai/scratch/file.txt")).expect("round trip"),
            "second"
        );
    }

    #[tokio::test]
    async fn content_round_trips_exactly() {
        let repo = temp_root("roundtrip");
        let tool = jailed_tool(&repo);
        let content = "pineapple\nline two\twith tabs — and unicode\n";

        tool.execute(json!({"path": "rt/out.txt", "content": content}))
            .await
            .expect("execute succeeds");

        assert_eq!(
            fs::read_to_string(repo.join(".axonerai/scratch/rt/out.txt")).expect("round trip"),
            content
        );
    }

    #[test]
    fn description_mentions_the_scratch_jail() {
        let tool = WriteFile::default();
        let description = tool.description();
        assert!(
            description.contains(".axonerai/scratch")
                && description.contains("absolute paths and '..' are rejected"),
            "description must tell the model about the jail: {description}"
        );
    }
}
