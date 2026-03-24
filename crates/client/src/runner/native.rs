use std::process::Stdio;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::{Child, Command};
use libc;

use super::Runner;
use crate::session_runner::SessionConfig;

/// Runs Claude directly on the host as a subprocess.
pub struct NativeRunner;

#[async_trait]
impl Runner for NativeRunner {
    async fn spawn(&self, config: &SessionConfig) -> Result<Child> {
        let mut cmd = Command::new("claude");
        cmd.args([
            "--output-format", "stream-json",
            "--input-format", "stream-json",
            "--verbose",
            "--dangerously-skip-permissions",
        ]);

        if config.is_resume {
            cmd.args(["--resume", &config.claude_session_id]);
        } else {
            cmd.args(["--session-id", &config.claude_session_id]);
        }

        for arg in &config.extra_args {
            cmd.arg(arg);
        }

        if let Some(ref prompt) = config.system_prompt {
            cmd.args(["--system-prompt", prompt]);
        }

        cmd.current_dir(&config.default_cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        cmd.spawn().context("spawning claude — is it installed and on PATH?")
    }

    async fn kill(&self, child: &mut Child, _config: &SessionConfig) {
        let _ = child.kill().await;
    }

    async fn interrupt(&self, child: &mut Child, _config: &SessionConfig) {
        if let Some(pid) = child.id() {
            // SAFETY: kill() is always safe to call with a valid pid and signal.
            unsafe {
                libc::kill(pid as libc::pid_t, libc::SIGINT);
            }
        }
    }
}
