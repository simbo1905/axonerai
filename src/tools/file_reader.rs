//! item46 — read-only repo-inspection builtins: `ReadFile` and `ListDir`.
//!
//! A linter skill must be able to read the repo it reviews. Both tools are
//! jailed to the REPO ROOT (the server CWD threaded in at construction;
//! `Default` uses the process CWD), read-only, and reject absolute paths,
//! `..` components and canonicalise+prefix symlink escapes with the same
//! pattern as the write_file jail (src/tools/file_writer.rs).
//!
//! `ReadFile` is text-only: binary-ish content (a NUL byte in the first 8KB)
//! is rejected with a clear error, and reads are hard-capped at 200KB with a
//! truncation marker appended. `ListDir` is a flat (non-recursive) listing
//! of one directory: name, type, size. Both default `is_read_only()` → true
//! so they survive the `--tools-readonly` registry filter.

use crate::tool::Tool;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

/// Hard read cap: 200KB.
pub const MAX_READ_BYTES: u64 = 200 * 1024;
/// How many leading bytes to sniff for binary content (NUL byte check).
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

const REJECT_ABSOLUTE: &str = "Error: For security, absolute paths are not allowed; paths are jailed to the repo root (the server's working directory).";
const REJECT_PARENT_DIR: &str = "Error: For security, parent directory traversal (..) is not allowed; paths are jailed to the repo root (the server's working directory).";
const REJECT_ESCAPE: &str = "Error: path escapes the repo root; read rejected.";

/// Shared jail resolution for both read tools. The outer `Result` carries
/// genuine io failures; the inner one carries a user-facing rejection
/// message (surfaced as `Ok("Error: ...")` to keep the pre-jail tools'
/// convention).
///
/// Same shape as WriteFile's jail: reject absolute/`..` up front, then
/// canonicalise the deepest existing ancestor (resolving any symlink in the
/// existing prefix, including dangling ones) and re-join the not-yet-existing
/// tail before the prefix escape assert.
fn resolve_within_root(root: &Path, path_str: &str) -> Result<Result<PathBuf, String>> {
    let rel = Path::new(path_str);
    if rel.is_absolute() {
        return Ok(Err(REJECT_ABSOLUTE.to_string()));
    }
    if rel.components().any(|c| matches!(c, Component::ParentDir)) {
        return Ok(Err(REJECT_PARENT_DIR.to_string()));
    }

    let canonical_root = fs::canonicalize(root)?;
    let target = root.join(rel);

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
                "Error: cannot resolve path inside the repo root: {e}"
            )));
        }
    };
    let mut canonical_target = canonical_ancestor;
    for part in tail.iter().rev() {
        canonical_target = canonical_target.join(part);
    }
    if !canonical_target.starts_with(&canonical_root) {
        return Ok(Err(REJECT_ESCAPE.to_string()));
    }
    Ok(Ok(canonical_target))
}

/// True when the content looks binary: a NUL byte anywhere in the first
/// [`BINARY_SNIFF_BYTES`] bytes.
fn looks_binary(bytes: &[u8]) -> bool {
    let sniff_len = bytes.len().min(BINARY_SNIFF_BYTES);
    bytes[..sniff_len].contains(&0)
}

fn not_found_error(path_str: &str, e: &io::Error) -> String {
    if e.kind() == io::ErrorKind::NotFound {
        format!("Error: path not found: {path_str}")
    } else {
        format!("Error: cannot read '{path_str}': {e}")
    }
}

/// Read-only text file reader jailed to the repo root.
pub struct ReadFile {
    root: PathBuf,
}

impl Default for ReadFile {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(cwd)
    }
}

impl ReadFile {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> String {
        "ReadFile".to_string()
    }

    fn description(&self) -> String {
        "Reads a TEXT file from the repository (read-only, cannot write; paths are jailed to the repo root — absolute paths and '..' are rejected). Files larger than 200KB are truncated with a marker; binary files are rejected. Usage: {\"path\": \"src/foo.rs\"}".to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path relative to the repo root (e.g. \"src/main.rs\"); absolute paths and '..' are rejected"
                }
            },
            "required": ["path"]
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing path"))?;

        let target = match resolve_within_root(&self.root, path_str)? {
            Ok(target) => target,
            Err(message) => return Ok(message),
        };

        let bytes = match fs::read(&target) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Ok(format!("Error: path not found: {path_str}"));
            }
            Err(e) if target.is_dir() || e.raw_os_error() == Some(21) => {
                return Ok(format!(
                    "Error: '{path_str}' is a directory, not a file; use ListDir to list it"
                ));
            }
            Err(e) => {
                return Ok(not_found_error(path_str, &e));
            }
        };

        if looks_binary(&bytes) {
            return Ok(format!(
                "Error: '{path_str}' looks binary (NUL byte in the first {BINARY_SNIFF_BYTES} bytes); only text files can be read"
            ));
        }

        let total = bytes.len() as u64;
        if total > MAX_READ_BYTES {
            let truncated = &bytes[..MAX_READ_BYTES as usize];
            let text = String::from_utf8_lossy(truncated);
            return Ok(format!(
                "{text}\n\n[TRUNCATED: showing the first {MAX_READ_BYTES} of {total} bytes of '{path_str}']"
            ));
        }

        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Read-only flat (non-recursive) directory listing jailed to the repo root.
pub struct ListDir {
    root: PathBuf,
}

impl Default for ListDir {
    fn default() -> Self {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        Self::new(cwd)
    }
}

impl ListDir {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }
}

#[async_trait]
impl Tool for ListDir {
    fn name(&self) -> String {
        "ListDir".to_string()
    }

    fn description(&self) -> String {
        "Lists ONE directory (read-only, cannot write; paths are jailed to the repo root — absolute paths and '..' are rejected). Flat, non-recursive: one line per entry as '<name>\\t<type>\\t<size>' where type is file|dir|symlink and size is bytes (0 for dirs). Usage: {\"path\": \"src\"} (default \".\")".to_string()
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to the repo root (default \".\"); absolute paths and '..' are rejected"
                }
            }
        })
    }

    async fn execute(&self, input: Value) -> Result<String> {
        let path_str = input["path"].as_str().unwrap_or(".");

        let target = match resolve_within_root(&self.root, path_str)? {
            Ok(target) => target,
            Err(message) => return Ok(message),
        };

        if !target.exists() {
            return Ok(format!("Error: path not found: {path_str}"));
        }
        if !target.is_dir() {
            return Ok(format!(
                "Error: '{path_str}' is not a directory; use ReadFile to read it"
            ));
        }

        let mut entries = match fs::read_dir(&target) {
            Ok(entries) => entries,
            Err(e) => return Ok(not_found_error(path_str, &e)),
        };

        let mut lines: Vec<String> = Vec::new();
        for entry in entries.by_ref() {
            let entry = match entry {
                Ok(entry) => entry,
                Err(e) => {
                    lines.push(format!("Error: cannot read an entry of '{path_str}': {e}"));
                    continue;
                }
            };
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(e) => {
                    lines.push(format!(
                        "Error: cannot type '{}': {e}",
                        entry.file_name().to_string_lossy()
                    ));
                    continue;
                }
            };
            let (kind, size) = if file_type.is_dir() {
                ("dir", 0)
            } else if file_type.is_symlink() {
                // Report the LINK size, not the target's — a flat listing
                // must not follow symlinks out of the jail.
                match entry.metadata() {
                    Ok(m) => ("symlink", m.len()),
                    Err(_) => ("symlink", 0),
                }
            } else {
                match entry.metadata() {
                    Ok(m) => ("file", m.len()),
                    Err(_) => ("file", 0),
                }
            };
            lines.push(format!(
                "{}\t{kind}\t{size}",
                entry.file_name().to_string_lossy()
            ));
        }
        lines.sort();
        Ok(lines.join("\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

    /// Isolated repo-root fixture: a temp "repo" with src/main.rs and
    /// src/lib.rs, plus a scratch dir for escape tests. Tools always get an
    /// explicit root, so no process-CWD mutation (and no CWD lock) is needed.
    fn temp_repo(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let n = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let repo = std::env::temp_dir().join(format!(
            "axonerai-file-reader-{tag}-{}-{nanos}-{n}",
            std::process::id()
        ));
        fs::create_dir_all(repo.join("src")).expect("create src");
        fs::write(repo.join("src/main.rs"), "fn main() {}\n").expect("seed main.rs");
        fs::write(repo.join("src/lib.rs"), "pub fn f() -> u32 { 1 }\n").expect("seed lib.rs");
        fs::write(repo.join("README.md"), "# fixture\n").expect("seed README");
        repo
    }

    fn read_tool(repo: &Path) -> ReadFile {
        ReadFile::new(repo.to_path_buf())
    }

    fn list_tool(repo: &Path) -> ListDir {
        ListDir::new(repo.to_path_buf())
    }

    #[tokio::test]
    async fn read_repo_file_ok() {
        let repo = temp_repo("read-ok");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/main.rs"}))
            .await
            .expect("execute succeeds");
        assert_eq!(out, "fn main() {}\n");
    }

    #[tokio::test]
    async fn read_file_utf8_round_trips() {
        let repo = temp_repo("utf8");
        fs::write(repo.join("src/u.txt"), "héllo — unicode ✓\n").expect("seed");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/u.txt"}))
            .await
            .expect("execute succeeds");
        assert_eq!(out, "héllo — unicode ✓\n");
    }

    #[tokio::test]
    async fn absolute_path_is_rejected() {
        let repo = temp_repo("abs");
        let outside = std::env::temp_dir().join("axonerai-file-reader-victim.txt");
        fs::write(&outside, "secret").expect("seed victim");

        let out = read_tool(&repo)
            .execute(json!({"path": outside.to_string_lossy()}))
            .await
            .expect("rejections are surfaced as an Error result, not a panic");
        assert!(out.contains("Error"), "expected rejection, got: {out}");

        let out = list_tool(&repo)
            .execute(json!({"path": std::env::temp_dir().to_string_lossy()}))
            .await
            .expect("rejections are surfaced as an Error result, not a panic");
        assert!(out.contains("Error"), "expected rejection, got: {out}");
    }

    #[tokio::test]
    async fn parent_traversal_is_rejected() {
        let repo = temp_repo("dotdot");
        let tool = read_tool(&repo);

        for path in ["../escape.txt", "src/../../escape.txt"] {
            let out = tool
                .execute(json!({"path": path}))
                .await
                .expect("rejections are surfaced as an Error result, not a panic");
            assert!(
                out.contains("Error"),
                "expected rejection for {path}: {out}"
            );
        }
        assert!(!repo.parent().unwrap().join("escape.txt").exists());

        let tool = list_tool(&repo);
        for path in ["..", "src/.."] {
            let out = tool
                .execute(json!({"path": path}))
                .await
                .expect("rejections are surfaced as an Error result, not a panic");
            assert!(
                out.contains("Error"),
                "expected rejection for {path}: {out}"
            );
        }
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn symlink_escape_is_rejected() {
        let repo = temp_repo("symlink");
        let outside = temp_repo("symlink-outside");
        let victim = outside.join("secret.txt");
        fs::write(&victim, "secret").expect("seed victim");
        std::os::unix::fs::symlink(&victim, repo.join("link.txt")).expect("file symlink");

        let outside_dir = outside.join("dir");
        fs::create_dir_all(&outside_dir).expect("seed outside dir");
        std::os::unix::fs::symlink(&outside_dir, repo.join("outdir")).expect("dir symlink");

        let read = read_tool(&repo);
        let list = list_tool(&repo);

        let out = read
            .execute(json!({"path": "link.txt"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("Error"),
            "file-symlink read must be rejected: {out}"
        );

        let out = read
            .execute(json!({"path": "outdir/secret.txt"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("Error"),
            "dir-symlink read must be rejected: {out}"
        );

        let out = list
            .execute(json!({"path": "outdir"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("Error"),
            "dir-symlink listing must be rejected: {out}"
        );
    }

    #[tokio::test]
    async fn binary_file_is_rejected() {
        let repo = temp_repo("binary");
        fs::write(repo.join("src/blob.bin"), b"OK\x00BINARY").expect("seed binary");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/blob.bin"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("binary"),
            "expected binary rejection, got: {out}"
        );
    }

    #[tokio::test]
    async fn binary_sniff_only_looks_at_first_8kb() {
        let repo = temp_repo("late-nul");
        let mut content = vec![b'a'; BINARY_SNIFF_BYTES];
        content.push(0);
        fs::write(repo.join("src/late.bin"), &content).expect("seed");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/late.bin"}))
            .await
            .expect("execute succeeds");
        assert!(
            !out.starts_with("Error"),
            "a NUL after the 8KB sniff window is read as text, not rejected: {out}"
        );
        assert!(
            !out.contains("TRUNCATED"),
            "8193 bytes is under the 200KB cap; no marker expected: {out}"
        );
    }

    #[tokio::test]
    async fn cap_truncates_with_marker() {
        let repo = temp_repo("cap");
        let content = vec![b'x'; (MAX_READ_BYTES + 1024) as usize];
        fs::write(repo.join("src/big.txt"), &content).expect("seed big file");

        let out = read_tool(&repo)
            .execute(json!({"path": "src/big.txt"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("[TRUNCATED: showing the first 204800 of 205824 bytes"),
            "expected truncation marker, got tail: {:?}",
            &out[out.len().saturating_sub(120)..]
        );
        assert_eq!(
            out.len(),
            MAX_READ_BYTES as usize
                + "\n\n[TRUNCATED: showing the first 204800 of 205824 bytes of 'src/big.txt']"
                    .len()
        );
    }

    #[tokio::test]
    async fn file_under_cap_is_not_truncated() {
        let repo = temp_repo("under-cap");
        let content = "y".repeat((MAX_READ_BYTES - 1) as usize);
        fs::write(repo.join("src/just-fits.txt"), &content).expect("seed");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/just-fits.txt"}))
            .await
            .expect("execute succeeds");
        assert_eq!(out, content);
    }

    #[tokio::test]
    async fn missing_path_errors() {
        let repo = temp_repo("missing");
        let out = read_tool(&repo)
            .execute(json!({"path": "src/nope.rs"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("not found"), "got: {out}");

        let out = list_tool(&repo)
            .execute(json!({"path": "no/such/dir"}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("not found"), "got: {out}");
    }

    #[tokio::test]
    async fn read_of_a_directory_directs_to_listdir() {
        let repo = temp_repo("read-dir");
        let out = read_tool(&repo)
            .execute(json!({"path": "src"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("is a directory") && out.contains("ListDir"),
            "got: {out}"
        );
    }

    #[tokio::test]
    async fn listdir_lists_flat_with_type_and_size() {
        let repo = temp_repo("list");
        fs::create_dir_all(repo.join("src/sub")).expect("seed subdir");
        fs::write(repo.join("src/sub/deep.txt"), "deep").expect("seed deep file");

        let out = list_tool(&repo)
            .execute(json!({"path": "src"}))
            .await
            .expect("execute succeeds");
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 3, "flat listing, no recursion: {out}");
        assert!(lines.contains(&"lib.rs\tfile\t24"), "got: {out}");
        assert!(lines.contains(&"main.rs\tfile\t13"), "got: {out}");
        assert!(lines.contains(&"sub\tdir\t0"), "got: {out}");
        assert!(!out.contains("deep.txt"), "must not recurse: {out}");
    }

    #[tokio::test]
    async fn listdir_defaults_to_repo_root() {
        let repo = temp_repo("default");
        let out = list_tool(&repo)
            .execute(json!({}))
            .await
            .expect("execute succeeds");
        assert!(out.contains("README.md\tfile\t10"), "got: {out}");
        assert!(out.contains("src\tdir\t0"), "got: {out}");
    }

    #[tokio::test]
    async fn listdir_of_a_file_directs_to_readfile() {
        let repo = temp_repo("list-file");
        let out = list_tool(&repo)
            .execute(json!({"path": "README.md"}))
            .await
            .expect("execute succeeds");
        assert!(
            out.contains("not a directory") && out.contains("ReadFile"),
            "got: {out}"
        );
    }

    #[test]
    fn read_only_defaults_true_for_both_tools() {
        fn assert_read_only<T: Tool>(tool: T) {
            assert!(tool.is_read_only(), "{} must be read-only", tool.name());
        }
        assert_read_only(ReadFile::default());
        assert_read_only(ListDir::default());
    }

    #[test]
    fn descriptions_are_honest_about_the_jail() {
        let read = ReadFile::default();
        let list = ListDir::default();
        for (name, description) in [
            (read.name(), read.description()),
            (list.name(), list.description()),
        ] {
            assert!(
                description.contains("read-only")
                    && description.contains("cannot write")
                    && description.contains("jailed to the repo root"),
                "{name} description must be honest: {description}"
            );
        }
    }
}
