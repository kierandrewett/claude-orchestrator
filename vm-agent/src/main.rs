//! vm-agent — runs inside the Alpine microVM.
//!
//! Listens on vsock port 5000, accepts one connection, reads a JSON config
//! line, spawns `claude` with the configured args, and relays stdin/stdout
//! through the vsock stream using two threads.
//!
//! Compile as a musl static binary:
//!   cargo build --release --target x86_64-unknown-linux-musl -p vm-agent

use std::io::{Read, Write};
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::process::Stdio;

use serde::Deserialize;
use vsock::{VsockAddr, VsockListener, VMADDR_CID_ANY};

const AGENT_PORT: u32 = 5000;

#[derive(Debug, Deserialize)]
struct AgentConfig {
    session_id: String,
    initial_prompt: Option<String>,
    claude_session_id: String,
    is_resume: bool,
    cwd: String,
    #[serde(default)]
    extra_args: Vec<String>,
}

fn main() {
    eprintln!("vm-agent: binding vsock port {AGENT_PORT}");
    let addr = VsockAddr::new(VMADDR_CID_ANY, AGENT_PORT);
    let listener = VsockListener::bind(&addr).expect("bind vsock");

    eprintln!("vm-agent: ready");
    match listener.accept() {
        Ok((stream, _)) => handle(stream),
        Err(e) => {
            eprintln!("vm-agent: accept: {e}");
            std::process::exit(1);
        }
    }
}

fn handle(mut stream: vsock::VsockStream) {
    // Read newline-terminated config JSON
    let config: AgentConfig = match read_line_json(&mut stream) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vm-agent: config read error: {e}");
            return;
        }
    };
    eprintln!(
        "vm-agent: session={} resume={} cwd={}",
        config.session_id, config.is_resume, config.cwd
    );

    // Build and spawn claude
    let mut cmd = std::process::Command::new("claude");
    cmd.args([
        "--print",
        "--verbose",
        "--input-format",
        "stream-json",
        "--output-format",
        "stream-json",
        "--include-partial-messages",
        "--dangerously-skip-permissions",
    ]);
    if config.is_resume {
        cmd.args(["--resume", &config.claude_session_id]);
    } else {
        cmd.args(["--session-id", &config.claude_session_id]);
    }
    for a in &config.extra_args {
        cmd.arg(a);
    }
    cmd.current_dir(&config.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("vm-agent: spawn claude: {e}");
            return;
        }
    };

    let mut claude_stdin = child.stdin.take().unwrap();
    let claude_stdout = child.stdout.take().unwrap();

    // Write initial prompt before relay starts
    if let Some(ref prompt) = config.initial_prompt {
        let _ = claude_stdin.write_all(format_user_msg(prompt).as_bytes());
    }

    // Duplicate the vsock fd so read and write halves can live in separate threads
    // SAFETY: stream.as_raw_fd() is valid for the lifetime of stream.
    let dup_fd = unsafe { libc::dup(stream.as_raw_fd()) };
    assert!(dup_fd >= 0, "dup() failed");
    // Use the dup'd fd as a plain File for reading (avoids borrowing stream)
    let vsock_reader = unsafe { std::fs::File::from_raw_fd(dup_fd) };

    // Thread: vsock → claude stdin
    let stdin_thread = std::thread::spawn(move || relay(vsock_reader, claude_stdin));

    // Main: claude stdout → vsock (uses original stream as writer)
    relay(claude_stdout, &mut stream);

    stdin_thread.join().ok();
    child.wait().ok();
    eprintln!("vm-agent: session ended");
}

/// Read a newline-terminated line from `stream` byte-by-byte and deserialise.
fn read_line_json<T: for<'de> Deserialize<'de>>(
    stream: &mut vsock::VsockStream,
) -> Result<T, String> {
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    loop {
        stream
            .read_exact(&mut b)
            .map_err(|e| format!("read: {e}"))?;
        if b[0] == b'\n' {
            break;
        }
        buf.push(b[0]);
    }
    serde_json::from_slice(&buf).map_err(|e| format!("json: {e}"))
}

/// Pump bytes from `src` to `dst` until EOF or error.
fn relay<R: Read, W: Write>(mut src: R, mut dst: W) {
    let mut buf = [0u8; 8192];
    loop {
        match src.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                if dst.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        }
    }
}

fn format_user_msg(text: &str) -> String {
    let escaped = serde_json::to_string(text).unwrap_or_default();
    format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{escaped}}}}}\n"
    )
}
