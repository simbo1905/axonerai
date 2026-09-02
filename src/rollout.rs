use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use serde::Serialize;
use serde_json::Value;

/// Append-only, timestamped JSONL log (`.jsonlts`).
///
/// Each line is `<unix_epoch_ms>\0<json>\n`. Records are append-only; reads are
/// streaming linear scans — nothing is ever materialised wholesale in memory.
#[derive(Debug, Clone)]
pub struct Rollout {
    path: PathBuf,
}

/// A summary row for one rollout file on disk.
#[derive(Debug, Clone, Serialize)]
pub struct SessionInfo {
    pub uuid: String,
    pub title: String,
    pub updated: u64,
    pub size: u64,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Split a raw rollout line into `(ts, json)`. Returns `None` for malformed
/// lines (no `\0` separator or a non-numeric timestamp).
fn split_line(line: &str) -> Option<(u64, &str)> {
    let (ts_raw, json) = line.split_once('\0')?;
    let ts = ts_raw.parse::<u64>().ok()?;
    Some((ts, json))
}

impl Rollout {
    /// Create a fresh rollout file for `session_id` in `dir`.
    ///
    /// Errors if the file already exists — the caller must pick a fresh id.
    pub fn create(dir: &Path, session_id: &str) -> Result<Rollout> {
        fs::create_dir_all(dir)
            .with_context(|| format!("failed to create sessions dir {}", dir.display()))?;
        let path = dir.join(format!("{session_id}.jsonlts"));
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .with_context(|| format!("failed to create rollout {}", path.display()))?;
        Ok(Rollout { path })
    }

    /// Open an existing rollout file for `session_id` in `dir`.
    pub fn open(dir: &Path, session_id: &str) -> Result<Rollout> {
        let path = dir.join(format!("{session_id}.jsonlts"));
        File::open(&path).with_context(|| format!("rollout not found: {}", path.display()))?;
        Ok(Rollout { path })
    }

    /// Full path of the rollout file.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append `event` stamped with the current time and return that timestamp.
    ///
    /// Monotonic guard: if the wall clock is not ahead of the last written ts,
    /// the event is stamped `last_ts + 1` so ts ordering always matches file
    /// order.
    pub fn append_event(&self, event: &Value) -> Result<u64> {
        let json = serde_json::to_string(event)?;
        let mut ts = now_ms();
        let last = self.last_ts()?;
        if ts <= last {
            ts = last + 1;
        }
        let mut file = OpenOptions::new()
            .append(true)
            .open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        file.write_all(format!("{ts}\0{json}\n").as_bytes())
            .with_context(|| format!("failed to append to rollout {}", self.path.display()))?;
        file.flush()?;
        Ok(ts)
    }

    /// Stream-scan the rollout, invoking `visit(ts, json)` for every well-formed
    /// record with `ts > after_ts`, in file order. Malformed lines are skipped.
    /// Never buffers the whole file.
    pub fn scan_from(
        &self,
        after_ts: u64,
        mut visit: impl FnMut(u64, &str) -> Result<()>,
    ) -> Result<()> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some((ts, json)) = split_line(&line) else {
                continue;
            };
            if ts <= after_ts {
                continue;
            }
            visit(ts, json)?;
        }
        Ok(())
    }

    /// The greatest timestamp written to this rollout (0 for an empty file).
    pub fn last_ts(&self) -> Result<u64> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        let mut last = 0u64;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if let Some((ts, _)) = split_line(&line) {
                last = ts;
            }
        }
        Ok(last)
    }

    /// Number of (well-formed) records in this rollout.
    pub fn line_count(&self) -> Result<u64> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        let mut count = 0u64;
        for line in BufReader::new(file).lines() {
            let line = line?;
            if split_line(&line).is_some() {
                count += 1;
            }
        }
        Ok(count)
    }

    /// Streaming scan: the session title — the first `session_meta` line's
    /// `title`, else the rollout's uuid (file stem).
    pub fn title(&self) -> Result<String> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        let fallback = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .to_string();
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some((_, json)) = split_line(&line) else {
                continue;
            };
            let Ok(v) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            if v.get("_type").and_then(Value::as_str) == Some("session_meta") {
                if let Some(t) = v.get("title").and_then(Value::as_str) {
                    return Ok(t.to_string());
                }
            }
        }
        Ok(fallback)
    }
}

/// Index every `*.jsonlts` rollout in `dir`, sorted by `updated` descending.
///
/// Title resolution, in a single streaming pass per file: the last
/// `session_rename` line's `title` wins; otherwise the first `session_meta`
/// line's `title`; otherwise the uuid.
pub fn sessions_index(dir: &Path) -> Result<Vec<SessionInfo>> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(out),
        Err(e) => {
            return Err(anyhow!(
                "failed to read sessions dir {}: {e}",
                dir.display()
            ));
        }
    };

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonlts") {
            continue;
        }
        let Some(uuid) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let size = entry.metadata()?.len();

        let mut updated = 0u64;
        let mut rename_title: Option<String> = None;
        let mut meta_title: Option<String> = None;
        let file = File::open(&path)?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let Some((ts_raw, json)) = line.split_once('\0') else {
                continue;
            };
            let Ok(ts) = ts_raw.parse::<u64>() else {
                continue;
            };
            if ts > updated {
                updated = ts;
            }
            let Ok(v) = serde_json::from_str::<Value>(json) else {
                continue;
            };
            match v.get("_type").and_then(Value::as_str) {
                Some("session_rename") => {
                    if let Some(t) = v.get("title").and_then(Value::as_str) {
                        rename_title = Some(t.to_string());
                    }
                }
                Some("session_meta") => {
                    if meta_title.is_none() {
                        if let Some(t) = v.get("title").and_then(Value::as_str) {
                            meta_title = Some(t.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        let title = rename_title
            .or(meta_title)
            .unwrap_or_else(|| uuid.to_string());
        out.push(SessionInfo {
            uuid: uuid.to_string(),
            title,
            updated,
            size,
        });
    }

    out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| a.uuid.cmp(&b.uuid)));
    Ok(out)
}

/// Default directory for rollout files, relative to the current directory.
pub fn default_dir() -> PathBuf {
    PathBuf::from(".axonerai/sessions")
}

/// Truncate `text` to at most `max_chars` characters for egress to the browser.
///
/// Short input is returned as-is. Longer input is cut to `max_chars - 1`
/// characters and a single `…` is appended. Truncation is character-based so a
/// `\0` (or any other multi-byte/combining sequence) is never split.
/// Rollouts always store FULL text — abridge is applied only at egress.
pub fn abridge(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let mut out: String = text.chars().take(max_chars.saturating_sub(1)).collect();
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::Duration;

    /// Unique gitignored scratch dir per test; cleaned up on drop so panics
    /// don't leak dirs.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> TestDir {
            let dir = PathBuf::from(format!(
                ".tmp/rollout-test-{label}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::create_dir_all(&dir).unwrap();
            TestDir(dir)
        }
    }

    impl Drop for TestDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn append_raw(rollout: &Rollout, raw: &str) {
        let mut f = OpenOptions::new()
            .append(true)
            .open(rollout.path())
            .unwrap();
        f.write_all(raw.as_bytes()).unwrap();
        f.flush().unwrap();
    }

    #[test]
    fn create_errors_if_exists_and_open_errors_if_missing() {
        let dir = TestDir::new("create-open");
        let id = uuid::Uuid::now_v7().to_string();

        let r = Rollout::create(&dir.0, &id).unwrap();
        assert!(
            Rollout::create(&dir.0, &id).is_err(),
            "second create must fail"
        );
        assert!(Rollout::open(&dir.0, "no-such-uuid").is_err());
        assert!(Rollout::open(&dir.0, &id).is_ok());
        assert!(r.path().exists());
    }

    #[test]
    fn append_scan_from_and_monotonic_ts() {
        let dir = TestDir::new("scan");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();

        let t1 = r.append_event(&json!({"_type": "a"})).unwrap();
        let t2 = r.append_event(&json!({"_type": "b"})).unwrap();
        assert!(t1 <= t2);

        // Force a future ts directly, then the next append must bump past it.
        append_raw(&r, &format!("{}\0{}\n", t2 + 10_000, json!({"_type": "c"})));
        let t4 = r.append_event(&json!({"_type": "d"})).unwrap();
        assert_eq!(t4, t2 + 10_001, "monotonic guard must use last+1");

        // Malformed lines are skipped by the scan.
        append_raw(&r, "not-a-record\n");
        append_raw(&r, "abc\0def\n");

        let mut seen = Vec::new();
        r.scan_from(0, |ts, json| {
            seen.push((ts, json.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen.len(), 4, "malformed lines must be skipped");
        assert_eq!(seen[0].0, t1);
        assert!(seen[1].1.contains("\"_type\":\"b\""));
        assert_eq!(seen[3].0, t4);

        // Filtering: after_ts is exclusive.
        let mut filtered = Vec::new();
        r.scan_from(t2, |ts, _| {
            filtered.push(ts);
            Ok(())
        })
        .unwrap();
        assert_eq!(filtered, vec![t2 + 10_000, t4]);

        assert_eq!(r.last_ts().unwrap(), t4);
        assert_eq!(r.line_count().unwrap(), 4);
    }

    #[test]
    fn last_ts_empty_file_is_zero() {
        let dir = TestDir::new("empty");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        assert_eq!(r.last_ts().unwrap(), 0);
        assert_eq!(r.line_count().unwrap(), 0);
    }

    #[test]
    fn sessions_index_title_resolution_and_order() {
        let dir = TestDir::new("index");

        // rename wins over meta
        let a_id = uuid::Uuid::now_v7().to_string();
        let a = Rollout::create(&dir.0, &a_id).unwrap();
        let a_meta_ts = a
            .append_event(
                &json!({"_type": "session_meta", "session_id": a_id, "title": "meta title"}),
            )
            .unwrap();
        let a_ren_ts = a
            .append_event(&json!({"_type": "session_rename", "title": "rename title"}))
            .unwrap();
        assert!(a_ren_ts >= a_meta_ts);

        // meta fallback only
        let b_id = uuid::Uuid::now_v7().to_string();
        let b = Rollout::create(&dir.0, &b_id).unwrap();
        b.append_event(&json!({"_type": "session_meta", "session_id": b_id, "title": "meta only"}))
            .unwrap();

        // uuid fallback (rename present but without title keeps uuid fallback)
        let c_id = uuid::Uuid::now_v7().to_string();
        let c = Rollout::create(&dir.0, &c_id).unwrap();
        c.append_event(&json!({"_type": "session_rename"})).unwrap();

        // let wall-clock advance so ordering is deterministic
        std::thread::sleep(Duration::from_millis(5));
        let d_id = uuid::Uuid::now_v7().to_string();
        let d = Rollout::create(&dir.0, &d_id).unwrap();
        d.append_event(&json!({"_type": "session_meta", "session_id": d_id, "title": "newest"}))
            .unwrap();

        // a stray non-rollout file must be ignored
        fs::write(dir.0.join("notes.txt"), "ignore me").unwrap();

        let idx = sessions_index(&dir.0).unwrap();
        assert_eq!(idx.len(), 4);
        assert!(
            idx.windows(2).all(|w| w[0].updated >= w[1].updated),
            "sorted newest first"
        );
        assert_eq!(idx[0].uuid, d_id);
        assert_eq!(idx[0].title, "newest");

        let find = |id: &str| idx.iter().find(|s| s.uuid == id).unwrap();
        assert_eq!(find(&a_id).title, "rename title", "last rename wins");
        assert_eq!(find(&b_id).title, "meta only", "meta fallback");
        assert_eq!(find(&c_id).title, c_id, "uuid fallback");
        assert!(find(&a_id).size > 0);
    }

    #[test]
    fn abridge_truncates_and_never_splits_nul() {
        assert_eq!(abridge("short", 10), "short");
        assert_eq!(abridge("exact-len", 9), "exact-len");

        let long = "x".repeat(50);
        let out = abridge(&long, 10);
        assert_eq!(out.chars().count(), 10);
        assert!(out.ends_with('…'));
        assert_eq!(out.chars().take(9).collect::<String>(), "x".repeat(9));

        // \0 must survive intact, never be split or dropped mid-sequence
        let with_nul = format!("a\0{}", "y".repeat(20));
        let out = abridge(&with_nul, 5);
        assert!(
            out.contains('\0'),
            "embedded \\0 must remain intact: {out:?}"
        );
        assert_eq!(out.chars().count(), 5);

        // multibyte chars are counted per character
        let multi = "é".repeat(20);
        let out = abridge(&multi, 4);
        assert_eq!(out.chars().count(), 4);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn default_dir_is_axonerai_sessions() {
        assert_eq!(default_dir(), PathBuf::from(".axonerai/sessions"));
    }
}
