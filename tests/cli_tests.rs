use std::process::{Command, Stdio};
use std::time::Duration;
use std::thread;

#[test]
#[ignore] // Ignored by default as it requires GROQ_API_KEY environment variable
fn test_serve_command_starts() {
    // Start the server in a background process with environment variable
    let mut child = Command::new("cargo")
        .args(&["run", "--bin", "agt", "serve", "--port", "9999"])
        .env("GROQ_API_KEY", "test_key")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to start agt serve");

    // Give the server time to start
    thread::sleep(Duration::from_secs(2));

    // Check if the process is still running
    match child.try_wait() {
        Ok(Some(status)) => {
            panic!("Server exited unexpectedly with status: {}", status);
        }
        Ok(None) => {
            // Server is still running, this is what we want
            println!("Server is running as expected");
        }
        Err(e) => {
            panic!("Error checking server status: {}", e);
        }
    }

    // Cleanup: kill the server
    child.kill().expect("Failed to kill server process");
    child.wait().expect("Failed to wait for server process");
}

#[test]
fn test_serve_help_command() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "agt", "serve", "--help"])
        .output()
        .expect("Failed to execute agt serve --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Start the web-based chat interface server"));
    assert!(stdout.contains("--port"));
}

#[test]
fn test_agt_help_command() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "agt", "--", "--help"])
        .output()
        .expect("Failed to execute agt --help");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{}{}", stdout, stderr);
    
    assert!(combined.contains("command-line interface") || combined.contains("Usage: agt"));
    assert!(combined.contains("serve"));
}
