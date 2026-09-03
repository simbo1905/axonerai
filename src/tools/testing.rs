//! Test-only helpers shared by the tool unit tests: a hand-rolled minimal
//! HTTP/1.1 stub server and race-safe env-var access. No new crates.

#![cfg(test)]

use std::sync::{Arc, Mutex, MutexGuard};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

static ENV_MUTEX: Mutex<()> = Mutex::new(());

pub fn env_guard() -> MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn set_key(value: &str) {
    // SAFETY: callers hold env_guard(), serialising all env mutations in tests.
    unsafe { std::env::set_var("TAVILY_API_KEY", value) };
}

pub fn unset_key() {
    // SAFETY: callers hold env_guard(), serialising all env mutations in tests.
    unsafe { std::env::remove_var("TAVILY_API_KEY") };
}

pub fn set_context7_key(value: &str) {
    // SAFETY: callers hold env_guard(), serialising all env mutations in tests.
    unsafe { std::env::set_var("CONTEXT7_API_KEY", value) };
}

pub fn unset_context7_key() {
    // SAFETY: callers hold env_guard(), serialising all env mutations in tests.
    unsafe { std::env::remove_var("CONTEXT7_API_KEY") };
}

pub struct CapturedRequest {
    pub path: String,
    pub authorization: Option<String>,
    pub body: String,
}

pub struct StubServer {
    pub addr: std::net::SocketAddr,
}

impl StubServer {
    pub fn base_url(&self) -> String {
        format!("http://{}", self.addr)
    }
}

async fn read_request(
    stream: &mut TcpStream,
) -> std::io::Result<Option<(String, Option<String>, String)>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let header_end;
    loop {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            return Ok(None);
        }
        buf.extend_from_slice(&chunk[..n]);
        if let Some(pos) = find_subsequence(&buf, b"\r\n\r\n") {
            header_end = pos + 4;
            break;
        }
    }

    let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or("");
    let mut parts = request_line.split_whitespace();
    let _method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/").to_string();

    let mut content_length: usize = 0;
    let mut authorization: Option<String> = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim();
            let value = value.trim();
            if name.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            } else if name.eq_ignore_ascii_case("authorization") {
                authorization = Some(value.to_string());
            }
        }
    }

    while buf.len() < header_end + content_length {
        let n = stream.read(&mut chunk).await?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }

    let body = String::from_utf8_lossy(&buf[header_end..header_end + content_length]).to_string();
    Ok(Some((path, authorization, body)))
}

fn find_subsequence(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Spawn a stub HTTP/1.1 server on 127.0.0.1:0. `responder` inspects each
/// captured request (asserting headers/body) and returns (status, body).
pub async fn spawn_stub<F>(responder: F) -> StubServer
where
    F: Fn(&CapturedRequest) -> (u16, String) + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind stub server");
    let addr = listener.local_addr().expect("local_addr");
    let responder = Arc::new(responder);

    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let responder = responder.clone();
            tokio::spawn(async move {
                let Ok(Some((path, authorization, body))) = read_request(&mut stream).await else {
                    return;
                };
                let captured = CapturedRequest {
                    path,
                    authorization,
                    body,
                };
                let (status, response_body) = responder(&captured);
                let response = format!(
                    "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    status,
                    if status < 400 { "OK" } else { "Error" },
                    response_body.len(),
                    response_body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.flush().await;
                let _ = stream.shutdown().await;
            });
        }
    });

    StubServer { addr }
}
