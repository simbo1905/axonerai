//! `--oneshot` CLI mode (item45): run the agent ONCE for a single prompt and
//! exit, printing the final assistant text to stdout (exit 0; exit 1 on run
//! error) so the output is pipeable. Tool traces are collected here and the
//! caller prints them to STDERR when `--verbose` is set — stdout stays the
//! final answer only.
//!
//! Stdout purity is HARDENED at two levels: the agent runs in quiet mode
//! (no loop chatter) and, on unix, [`StdoutGuard`] discards at the fd level
//! anything written to stdout during the run — a stray debug print in a
//! provider or tool can never precede the final report.
//!
//! Session scope: STATELESS. No session file is created or reused; the
//! interactive web session store is untouched.

use std::path::Path;

use anyhow::Result;

use crate::agent::{Agent, ToolTrace};
use crate::provider::Provider;
use crate::tool::ToolRegistry;

/// The single line prefixed to a skill file's content when the `--oneshot`
/// argument resolves to an existing `.md` path. Nothing else is added.
pub const SKILL_PROMPT_PREFIX: &str = "Follow this skill exactly.";

/// The handful of libc fd calls needed for the stdout purity guard (unix).
/// std links libc anyway; no new dependency is introduced.
#[cfg(unix)]
mod fd {
    use std::os::raw::c_int;
    #[cfg(test)]
    use std::os::raw::c_void;

    unsafe extern "C" {
        pub fn dup(fd: c_int) -> c_int;
        pub fn dup2(src: c_int, dst: c_int) -> c_int;
        pub fn close(fd: c_int) -> c_int;
        #[cfg(test)]
        pub fn write(fd: c_int, buf: *const c_void, count: usize) -> isize;
    }
}

/// Stdout purity guard (unix): while alive, anything written to the process
/// stdout fd (1) is discarded at the fd level. This hardens `--oneshot`
/// against intermediate model chatter — a stray debug print in a provider,
/// a tool, or the agent loop can never precede the final report on stdout,
/// no matter which code path emits it. Diagnostics and `--verbose` traces
/// stay on stderr (fd 2, untouched).
#[cfg(unix)]
struct StdoutGuard {
    saved: std::os::fd::RawFd,
}

#[cfg(unix)]
impl StdoutGuard {
    /// Redirect process stdout (fd 1) to `/dev/null` for the guard's
    /// lifetime. Best-effort: a failure to install the guard simply leaves
    /// stdout alone (quiet mode still suppresses the loop's own chatter).
    fn suppress() -> std::io::Result<Self> {
        use std::io::Write as _;
        use std::os::fd::AsRawFd;

        let _ = std::io::stdout().flush();
        let saved = unsafe { fd::dup(1) };
        if saved < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let devnull = std::fs::File::create("/dev/null")?;
        if unsafe { fd::dup2(devnull.as_raw_fd(), 1) } < 0 {
            let err = std::io::Error::last_os_error();
            unsafe { fd::close(saved) };
            return Err(err);
        }
        drop(devnull);
        Ok(Self { saved })
    }
}

#[cfg(unix)]
impl Drop for StdoutGuard {
    fn drop(&mut self) {
        use std::io::Write as _;

        let _ = std::io::stdout().flush();
        unsafe { fd::dup2(self.saved, 1) };
        unsafe { fd::close(self.saved) };
    }
}

/// Resolve the `--oneshot` argument into the prompt actually sent to the
/// model: an argument that is a path to an EXISTING `.md` file becomes the
/// file's content prefixed with the single [`SKILL_PROMPT_PREFIX`] line;
/// anything else (including a non-existent path or an existing non-`.md`
/// file) is treated as a literal prompt.
pub fn resolve_prompt(arg: &str) -> String {
    let path = Path::new(arg);
    if arg.ends_with(".md") && path.is_file() {
        if let Ok(content) = std::fs::read_to_string(path) {
            return format!("{SKILL_PROMPT_PREFIX}\n{content}");
        }
    }
    arg.to_string()
}

/// Run the agent ONCE with the given (already resolved) prompt.
///
/// Returns `(final_text, traces)`: the final assistant text and one
/// [`ToolTrace`] per tool execution. The agent runs STATELESS (no
/// `FileSessionManager` — nothing is persisted) and in quiet mode so no
/// loop chatter reaches stdout; the caller decides where traces go (the
/// CLI prints them to stderr under `--verbose`).
pub async fn run_oneshot(
    provider: Box<dyn Provider>,
    registry: ToolRegistry,
    system_prompt: Option<String>,
    prompt: &str,
) -> Result<(String, Vec<ToolTrace>)> {
    let mut agent = Agent::new(provider, registry, system_prompt, None);
    agent.set_quiet(true);

    // Hardening: any stdout chatter written during the run is discarded at
    // the fd level, so stdout carries ONLY the final text the caller emits
    // after this returns. The returned traces go to stderr under --verbose.
    #[cfg(unix)]
    let _stdout_guard = StdoutGuard::suppress().ok();

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<ToolTrace>();
    let text = agent.run_with_traces(prompt, tx).await?;

    // The run owns the sender and drops it on return, so this drains the
    // traces collected during the run and then ends.
    let mut traces = Vec::new();
    while let Ok(trace) = rx.try_recv() {
        traces.push(trace);
    }
    Ok((text, traces))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{
        CompletionResponse, Message, StopReason, Tool as ProviderTool, ToolCall,
    };
    use crate::tool::Tool as ToolTrait;
    use anyhow::Result;
    use async_trait::async_trait;
    use serde_json::{Value, json};
    use std::sync::{Arc, Mutex};

    struct EchoTool;

    #[async_trait]
    impl ToolTrait for EchoTool {
        fn name(&self) -> String {
            "echo_tool".to_string()
        }

        fn description(&self) -> String {
            "echoes its input".to_string()
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        async fn execute(&self, input: Value) -> Result<String> {
            Ok(input.to_string())
        }
    }

    #[derive(Clone, Default)]
    struct SharedCalls(Arc<Mutex<Vec<Vec<Message>>>>);

    impl SharedCalls {
        fn push(&self, messages: Vec<Message>) {
            self.0.lock().unwrap().push(messages);
        }

        fn len(&self) -> usize {
            self.0.lock().unwrap().len()
        }
    }

    /// One ToolUse round (echo_tool) then EndTurn with the fixed final text.
    struct MockProvider {
        calls: SharedCalls,
    }

    #[async_trait]
    impl Provider for MockProvider {
        async fn complete(
            &self,
            messages: Vec<Message>,
            _tools: Option<Vec<ProviderTool>>,
            _max_tokens: Option<u32>,
            _system_prompt: Option<String>,
        ) -> Result<CompletionResponse> {
            self.calls.push(messages);
            if self.calls.len() == 1 {
                Ok(CompletionResponse {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".to_string(),
                        name: "echo_tool".to_string(),
                        input: json!({"expression": "2^7.2"}),
                    }],
                    stop_reason: StopReason::ToolUse,
                })
            } else {
                Ok(CompletionResponse {
                    text: Some("147.03".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                })
            }
        }
    }

    #[tokio::test]
    async fn oneshot_returns_final_text_and_traces_to_sink() {
        let calls = SharedCalls::default();
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(EchoTool));

        let (text, traces) = run_oneshot(
            Box::new(MockProvider {
                calls: calls.clone(),
            }),
            registry,
            None,
            "what is 2^7.2",
        )
        .await
        .expect("oneshot run");

        assert_eq!(text, "147.03", "final assistant text is returned");
        assert_eq!(calls.len(), 2, "one ToolUse round + one EndTurn round");
        assert_eq!(traces.len(), 1, "one trace per tool execution");
        assert_eq!(traces[0].tool, "echo_tool");
        assert!(traces[0].result_json.contains("expression"));
    }

    #[tokio::test]
    async fn oneshot_without_tool_use_returns_text_and_no_traces() {
        struct PlainProvider;

        #[async_trait]
        impl Provider for PlainProvider {
            async fn complete(
                &self,
                _messages: Vec<Message>,
                _tools: Option<Vec<ProviderTool>>,
                _max_tokens: Option<u32>,
                _system_prompt: Option<String>,
            ) -> Result<CompletionResponse> {
                Ok(CompletionResponse {
                    text: Some("42".to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                })
            }
        }

        let (text, traces) = run_oneshot(Box::new(PlainProvider), ToolRegistry::new(), None, "hi")
            .await
            .expect("oneshot run");
        assert_eq!(text, "42");
        assert!(traces.is_empty());
    }

    // --- stdout purity ------------------------------------------------------

    /// Write bytes straight to the process stdout fd (1), bypassing Rust's
    /// print machinery (which libtest captures per-thread and would never
    /// reach the fd). This is what stray process-level chatter looks like.
    #[cfg(unix)]
    fn write_stdout_raw(message: &str) {
        unsafe {
            fd::write(1, message.as_ptr().cast(), message.len());
        }
    }

    /// A provider that chatters on process stdout mid-run, simulating stray
    /// model chatter (e.g. a debug print inside a provider) that must never
    /// precede the final report on stdout in `--oneshot` mode.
    struct ChattyProvider;

    #[async_trait]
    impl Provider for ChattyProvider {
        async fn complete(
            &self,
            _messages: Vec<Message>,
            _tools: Option<Vec<ProviderTool>>,
            _max_tokens: Option<u32>,
            _system_prompt: Option<String>,
        ) -> Result<CompletionResponse> {
            write_stdout_raw("DEBUG: intermediate model chatter\n");
            Ok(CompletionResponse {
                text: Some("42".to_string()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
            })
        }
    }

    /// Redirect the process stdout fd (1) to a fresh temp file for the
    /// duration of `f`, then read back whatever leaked to it. Unix only.
    #[cfg(unix)]
    fn capture_stdout<T>(f: impl FnOnce() -> T) -> (String, T) {
        use std::io::{Read, Write};
        use std::os::fd::AsRawFd;

        let _ = std::io::stdout().flush();
        let saved = unsafe { fd::dup(1) };
        assert!(saved >= 0, "dup(1) failed");
        let capture = tempfile_for_capture();
        let capture_fd = capture.as_raw_fd();
        assert!(unsafe { fd::dup2(capture_fd, 1) } >= 0, "dup2 failed");
        let outcome = f();
        let _ = std::io::stdout().flush();
        assert!(unsafe { fd::dup2(saved, 1) } >= 0, "restore failed");
        unsafe { fd::close(saved) };
        drop(capture);

        let mut file = std::fs::File::open(capture_path()).expect("reopen capture");
        let mut leaked = String::new();
        file.read_to_string(&mut leaked).expect("read capture");
        (leaked, outcome)
    }

    #[cfg(unix)]
    fn capture_path() -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "axoner-oneshot-stdout-{}",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    fn tempfile_for_capture() -> std::fs::File {
        use std::io::Write;
        let path = capture_path();
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)
            .expect("create capture file");
        file.write_all(b"").expect("truncate capture file");
        file
    }

    #[test]
    #[cfg(unix)]
    fn oneshot_stdout_carries_no_intermediate_chatter() {
        let (leaked, result) = capture_stdout(|| {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .expect("runtime");
            rt.block_on(run_oneshot(
                Box::new(ChattyProvider),
                ToolRegistry::new(),
                None,
                "hi",
            ))
        });
        let (text, traces) = result.expect("oneshot run");
        assert_eq!(text, "42", "final text still returned");
        assert!(traces.is_empty());
        // The capture window is process-global fd redirection: the test
        // harness's own result lines may interleave into the capture file,
        // so assert the chatter itself is gone rather than the file empty.
        assert!(
            !leaked.contains("DEBUG: intermediate model chatter"),
            "intermediate chatter leaked to stdout: {leaked:?}"
        );
    }

    // --- prompt resolution --------------------------------------------------

    fn temp_md(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("axoner-oneshot-test-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn existing_md_path_resolves_to_skill_content_with_single_prefix_line() {
        let path = temp_md("SKILL.md");
        std::fs::write(&path, "Step 1: do the thing.\nStep 2: report.").unwrap();

        let prompt = resolve_prompt(path.to_str().unwrap());
        assert_eq!(
            prompt,
            "Follow this skill exactly.\nStep 1: do the thing.\nStep 2: report."
        );

        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn missing_path_is_treated_as_literal_prompt() {
        assert_eq!(resolve_prompt("hello world"), "hello world");
        assert_eq!(
            resolve_prompt("no/such/dir/SKILL.md"),
            "no/such/dir/SKILL.md",
            "a non-existent .md path is NOT a skill — literal prompt"
        );
    }

    #[test]
    fn existing_non_md_file_is_literal_prompt() {
        let path = temp_md("notes.txt");
        std::fs::write(&path, "not a skill").unwrap();
        let literal = path.to_str().unwrap().to_string();
        assert_eq!(resolve_prompt(&literal), literal);
        std::fs::remove_file(&path).unwrap();
        std::fs::remove_dir(path.parent().unwrap()).unwrap();
    }
}
