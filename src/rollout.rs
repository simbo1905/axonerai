use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, anyhow};
use lineformat::LineError;
use serde::Serialize;
use serde_json::Value;

/// A rollout is corrupt: the line at `line_no` / `byte_offset` failed the
/// strict `<digits>\0<json>\n` prefix validation. Reading HALTS at the first
/// bad line (never skips) so callers can serve data up to the corruption
/// point and warn server-side.
#[derive(Debug, Clone)]
pub struct RolloutCorrupted {
    pub line_no: u64,
    pub byte_offset: u64,
    pub kind: LineError,
}

impl fmt::Display for RolloutCorrupted {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rollout corrupt at line {} (byte offset {}): {}",
            self.line_no, self.byte_offset, self.kind
        )
    }
}

impl std::error::Error for RolloutCorrupted {}

/// Is `s` a well-formed session id: a lowercase UUID v7
/// (`^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$`).
/// Every entry point for session ids (CLI `-s`, REST paths, index filenames)
/// must pass through this check.
pub fn is_session_id(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 36 {
        return false;
    }
    let hex = |b: u8| b.is_ascii_hexdigit() && !b.is_ascii_uppercase();
    let hyphens = [8, 13, 18, 23];
    for (i, &b) in bytes.iter().enumerate() {
        let is_hyphen_slot = hyphens.contains(&i);
        if is_hyphen_slot {
            if b != b'-' {
                return false;
            }
        } else if !hex(b) {
            return false;
        }
    }
    bytes[14] == b'7' && matches!(bytes[19], b'8' | b'9' | b'a' | b'b')
}

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

impl Rollout {
    /// Create a fresh rollout file for `session_id` in `dir`.
    ///
    /// Errors if the file already exists — the caller must pick a fresh id.
    pub fn create(dir: &Path, session_id: &str) -> Result<Rollout> {
        if !is_session_id(session_id) {
            return Err(anyhow!("not a valid session id: {session_id}"));
        }
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
        self.append_json(&json)
    }

    /// Append pre-serialized JSON with the same monotonic timestamp guard as
    /// [`Rollout::append_event`]. Use this for records whose byte-level field
    /// order matters (the manual payload-last `Serialize` impls in
    /// `src/wire.rs`) — a `to_value` roundtrip would reorder keys
    /// alphabetically.
    pub fn append_json(&self, json: &str) -> Result<u64> {
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

    /// Stream-scan the rollout, invoking `visit(ts, json)` for every record
    /// with `ts > after_ts`, in file order. Strict: the scan HALTS at the
    /// first line failing the `<digits>\0` prefix validation with
    /// [`RolloutCorrupted`] — malformed data is never skipped. A crash-
    /// truncated final line (no trailing `\n`) is accepted when its prefix is
    /// valid; its JSON may be incomplete (the client's lenient path handles
    /// it). Never buffers the whole file.
    pub fn scan_from(
        &self,
        after_ts: u64,
        mut visit: impl FnMut(u64, &str) -> Result<()>,
    ) -> Result<()> {
        self.walk(|ts, json| {
            if ts > after_ts {
                visit(ts, json)?;
            }
            Ok(())
        })
    }

    /// The greatest timestamp written to this rollout (0 for an empty file).
    /// Halts on corruption like [`Rollout::scan_from`].
    pub fn last_ts(&self) -> Result<u64> {
        let mut last = 0u64;
        self.walk(|ts, _| {
            last = ts;
            Ok(())
        })?;
        Ok(last)
    }

    /// Number of records in this rollout. Halts on corruption like
    /// [`Rollout::scan_from`].
    pub fn line_count(&self) -> Result<u64> {
        let mut count = 0u64;
        self.walk(|_, _| {
            count += 1;
            Ok(())
        })?;
        Ok(count)
    }

    /// Streaming scan: the session title — the first `session_meta` line's
    /// `title`, else the rollout's uuid (file stem). Halts on corruption
    /// like [`Rollout::scan_from`].
    pub fn title(&self) -> Result<String> {
        let fallback = self
            .path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("session")
            .to_string();
        let mut found: Option<String> = None;
        self.walk(|_, json| {
            if found.is_some() {
                return Ok(());
            }
            let Ok(v) = serde_json::from_str::<Value>(json) else {
                return Ok(());
            };
            if v.get("_type").and_then(Value::as_str) == Some("session_meta") {
                if let Some(t) = v.get("title").and_then(Value::as_str) {
                    found = Some(t.to_string());
                }
            }
            Ok(())
        })?;
        Ok(found.unwrap_or(fallback))
    }

    /// Private line walker backing every read path: streams the file line by
    /// line (tracking 1-based line numbers and byte offsets) and HALTS at the
    /// first line whose ts prefix is invalid with [`RolloutCorrupted`]. The
    /// final line without a trailing `\n` is accepted when its prefix is
    /// valid — a crash during the last write must not lose the record.
    fn walk(&self, mut visit: impl FnMut(u64, &str) -> Result<()>) -> Result<()> {
        let file = File::open(&self.path)
            .with_context(|| format!("failed to open rollout {}", self.path.display()))?;
        let mut reader = BufReader::new(file);
        let mut byte_offset = 0u64;
        let mut line_no = 0u64;
        loop {
            let mut raw = String::new();
            let n = reader
                .read_line(&mut raw)
                .with_context(|| format!("failed to read rollout {}", self.path.display()))?;
            if n == 0 {
                return Ok(());
            }
            line_no += 1;
            let line = raw.strip_suffix('\n').unwrap_or(&raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            match lineformat::ts_prefix(line) {
                Ok(ts) => {
                    let json = &line[line.find('\0').expect("validated prefix has a NUL") + 1..];
                    visit(ts, json)?;
                }
                Err(kind) => {
                    return Err(RolloutCorrupted {
                        line_no,
                        byte_offset,
                        kind,
                    }
                    .into());
                }
            }
            byte_offset += n as u64;
        }
    }
}

/// Index every `*.jsonlts` rollout in `dir`, sorted by `updated` descending.
///
/// Only files whose stem is a valid UUID v7 session id ([`is_session_id`])
/// are indexed — junk or foreign files are ignored. The scan of each file is
/// strict: the first line failing the ts prefix validation HALTS the index
/// with [`RolloutCorrupted`] (malformed data is never skipped).
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
        if !is_session_id(uuid) {
            continue;
        }
        let size = entry.metadata()?.len();

        let mut updated = 0u64;
        let mut rename_title: Option<String> = None;
        let mut meta_title: Option<String> = None;
        let file = File::open(&path)?;
        let mut reader = BufReader::new(file);
        let mut line_no = 0u64;
        let mut byte_offset = 0u64;
        loop {
            let mut raw = String::new();
            let n = reader
                .read_line(&mut raw)
                .with_context(|| format!("failed to read {}", path.display()))?;
            if n == 0 {
                break;
            }
            line_no += 1;
            let line = raw.strip_suffix('\n').unwrap_or(&raw);
            let line = line.strip_suffix('\r').unwrap_or(line);
            let ts = match lineformat::ts_prefix(line) {
                Ok(ts) => ts,
                Err(kind) => {
                    return Err(RolloutCorrupted {
                        line_no,
                        byte_offset,
                        kind,
                    }
                    .into());
                }
            };
            byte_offset += n as u64;
            if ts > updated {
                updated = ts;
            }
            let json = &line[line.find('\0').expect("validated prefix has a NUL") + 1..];
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
    use lineformat::LineError;
    use serde_json::json;
    use std::time::Duration;

    /// Unique scratch dir per test; cleaned up on drop so panics don't leak
    /// dirs. ABSOLUTE (`std::env::temp_dir`), not relative `.tmp/`: lib
    /// tests share one process, and any test that mutates the process CWD
    /// (file_writer's jailed-Default test) would otherwise make these
    /// relative paths resolve under the wrong root mid-run.
    struct TestDir(PathBuf);

    impl TestDir {
        fn new(label: &str) -> TestDir {
            let dir = std::env::temp_dir().join(format!(
                "axonerai-rollout-test-{label}-{}",
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

        let mut seen = Vec::new();
        r.scan_from(0, |ts, json| {
            seen.push((ts, json.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen.len(), 4);
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
    fn scan_halts_at_first_bad_line_with_location() {
        let dir = TestDir::new("halt-mid");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        r.append_event(&json!({"_type": "a"})).unwrap();
        r.append_event(&json!({"_type": "b"})).unwrap();

        let mut raw = String::new();
        raw.push_str("oops\n");
        raw.push_str(&format!(
            "{}\0{}\n",
            1_717_238_400_009u64,
            json!({"_type": "c"})
        ));
        append_raw(&r, &raw);

        let mut seen = Vec::new();
        let err = r
            .scan_from(0, |ts, json| {
                seen.push((ts, json.to_string()));
                Ok(())
            })
            .unwrap_err();
        let corrupted = err
            .downcast_ref::<RolloutCorrupted>()
            .expect("scan must fail with RolloutCorrupted");
        assert_eq!(corrupted.line_no, 3);
        assert_eq!(corrupted.kind, LineError::NotDigit);
        // byte_offset points at the start of the offending line
        assert_eq!(
            corrupted.byte_offset,
            (std::fs::read(r.path()).unwrap().len() - raw.len()) as u64
        );
        // Data BEFORE the corruption point was served; the tail was not.
        assert_eq!(seen.len(), 2);
    }

    #[test]
    fn scan_halts_when_first_line_has_no_ts() {
        let dir = TestDir::new("halt-first");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        // A file that has no started ts is a bad file.
        append_raw(&r, "not-a-record\n");
        let err = r.scan_from(0, |_, _| Ok(())).unwrap_err();
        let corrupted = err.downcast_ref::<RolloutCorrupted>().unwrap();
        assert_eq!(corrupted.line_no, 1);
        assert_eq!(corrupted.byte_offset, 0);
    }

    #[test]
    fn scan_halts_at_raw_newline_mid_json() {
        let dir = TestDir::new("halt-raw-nl");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        // Realistic corruption: a raw \n inside JSON (outside a string) splits
        // the record into two physical lines; the second starts with
        // non-digits, so the scan must halt there.
        append_raw(
            &r,
            "1717238400000\0{\"_type\":\"assistant\",\n\"text\":\"x\"}\n",
        );
        let mut seen = Vec::new();
        let err = r
            .scan_from(0, |ts, json| {
                seen.push(ts);
                assert!(json.starts_with("{\"_type\":\"assistant\""), "{json}");
                Ok(())
            })
            .unwrap_err();
        let corrupted = err.downcast_ref::<RolloutCorrupted>().unwrap();
        assert_eq!(corrupted.line_no, 2);
        assert_eq!(seen.len(), 1, "line 1 has a valid prefix and is served");
    }

    #[test]
    fn scan_accepts_crash_truncated_final_line() {
        let dir = TestDir::new("truncated-final");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        let t1 = r.append_event(&json!({"_type": "a"})).unwrap();
        // Crash during the final write: valid ts prefix, incomplete JSON, no
        // trailing newline. Accepted — the client's lenient path handles it.
        append_raw(&r, "1717238400009\0{\"_type\":\"assistant\",\"text\":\"par");
        let mut seen = Vec::new();
        r.scan_from(0, |ts, json| {
            seen.push((ts, json.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].0, 1_717_238_400_009);
        assert!(seen[1].1.starts_with("{\"_type\":\"assistant\""));
        assert_eq!(r.last_ts().unwrap(), 1_717_238_400_009);
        assert_eq!(seen[0].0, t1);
    }

    #[test]
    fn scan_rejects_bad_prefix_final_line_without_newline() {
        let dir = TestDir::new("bad-final");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        r.append_event(&json!({"_type": "a"})).unwrap();
        append_raw(&r, "oops");
        let err = r.scan_from(0, |_, _| Ok(())).unwrap_err();
        let corrupted = err.downcast_ref::<RolloutCorrupted>().unwrap();
        assert_eq!(corrupted.line_no, 2);
    }

    #[test]
    fn last_ts_line_count_and_title_halt_on_corruption() {
        let dir = TestDir::new("halt-helpers");
        let r = Rollout::create(&dir.0, &uuid::Uuid::now_v7().to_string()).unwrap();
        r.append_event(&json!({"_type": "session_meta", "title": "t"}))
            .unwrap();
        append_raw(&r, "abc\0def\n");
        for op in [
            |r: &Rollout| r.last_ts().map(|_| ()),
            |r: &Rollout| r.line_count().map(|_| ()),
            |r: &Rollout| r.title().map(|_| ()),
        ] {
            let err = op(&r).unwrap_err();
            assert!(
                err.downcast_ref::<RolloutCorrupted>().is_some(),
                "helpers must halt with RolloutCorrupted: {err}"
            );
        }
    }

    #[test]
    fn sessions_index_skips_non_uuid_filenames() {
        let dir = TestDir::new("index-filter");
        let id = uuid::Uuid::now_v7().to_string();
        let r = Rollout::create(&dir.0, &id).unwrap();
        r.append_event(&json!({"_type": "session_meta", "title": "ok"}))
            .unwrap();
        fs::write(dir.0.join("garbage.jsonlts"), "junk\n").unwrap();
        fs::write(dir.0.join("not-a-uuid.jsonlts"), "1717238400000\0{}\n").unwrap();

        let idx = sessions_index(&dir.0).unwrap();
        assert_eq!(idx.len(), 1);
        assert_eq!(idx[0].uuid, id);
    }

    #[test]
    fn sessions_index_halts_on_corrupt_rollout() {
        let dir = TestDir::new("index-halt");
        let id = uuid::Uuid::now_v7().to_string();
        let r = Rollout::create(&dir.0, &id).unwrap();
        r.append_event(&json!({"_type": "session_meta"})).unwrap();
        append_raw(&r, "corrupt line\n");
        let err = sessions_index(&dir.0).unwrap_err();
        let corrupted = err.downcast_ref::<RolloutCorrupted>().unwrap();
        assert_eq!(corrupted.line_no, 2);
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

    #[test]
    fn is_session_id_accepts_lowercase_v7() {
        assert!(is_session_id("01890a5d-ac96-774b-bcce-b302099a8057"));
        assert!(is_session_id("ffffffff-ffff-7fff-abcd-0123456789ab"));
    }

    #[test]
    fn is_session_id_rejects_adversarial_shapes() {
        // v4 uuid
        assert!(!is_session_id("550e8400-e29b-41d4-a716-446655440000"));
        // uppercase
        assert!(!is_session_id("01890A5D-AC96-774B-BCCE-B302099A8057"));
        // wrong length
        assert!(!is_session_id("01890a5d-ac96-774b-bcce-b302099a805"));
        assert!(!is_session_id("01890a5d-ac96-774b-bcce-b302099a80577"));
        // missing hyphens
        assert!(!is_session_id("01890a5dac96774bbcceb302099a8057"));
        // path traversal
        assert!(!is_session_id("../../etc/passwd"));
        assert!(!is_session_id("..\\..\\x"));
        // junk
        assert!(!is_session_id(""));
        assert!(!is_session_id("sess_1"));
        // v7 shape but bad variant nibble (5th group / 4th group start)
        assert!(!is_session_id("01890a5d-ac96-774b-0cce-b302099a8057"));
        assert!(!is_session_id("01890a5d-ac96-774b-cce-b302099a8057"));
        // non-v7 version nibble in 3rd group
        assert!(!is_session_id("01890a5d-ac96-674b-bcce-b302099a8057"));
    }
}
