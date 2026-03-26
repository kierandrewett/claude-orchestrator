use std::process::Stdio;

use anyhow::{Context, Result};
use async_trait::async_trait;
use tokio::process::{Child, Command};
use libc;

use tracing::{info, warn};

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

        let disabled: std::collections::HashSet<&str> =
            config.disabled_mcp_servers.iter().map(|s| s.as_str()).collect();

        let mut mcp_servers = serde_json::Map::new();

        // Built-in orchestrator server — URL only carries session_id; suppress
        // and emoji lists are looked up server-side from the task config.
        if !disabled.contains("orchestrator") {
            let url = format!(
                "{}/mcp?session_id={}",
                config.orchestrator_url.trim_end_matches('/'),
                config.session_id,
            );
            if let Some(ref token) = config.orchestrator_token {
                mcp_servers.insert("orchestrator".to_string(), serde_json::json!({
                    "type": "sse",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {token}") }
                }));
            } else {
                mcp_servers.insert("orchestrator".to_string(), serde_json::json!({
                    "type": "sse",
                    "url": url
                }));
            }
        }

        // User-configured custom servers.
        for srv in &config.mcp_servers {
            if disabled.contains(srv.name.as_str()) {
                continue;
            }
            let entry = if let Some(ref url) = srv.url {
                // Default to "http" (streamable HTTP, MCP 2025-06-18) for user servers.
                // Users can set transport = "sse" explicitly for older servers.
                let transport = srv.transport.as_deref().unwrap_or("http");
                if srv.headers.is_empty() {
                    serde_json::json!({ "type": transport, "url": url })
                } else {
                    let hdrs: serde_json::Map<String, serde_json::Value> = srv.headers.iter()
                        .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                        .collect();
                    serde_json::json!({ "type": transport, "url": url, "headers": hdrs })
                }
            } else {
                let srv_env: serde_json::Map<String, serde_json::Value> = srv.env.iter()
                    .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                    .collect();
                serde_json::json!({ "command": srv.command, "args": srv.args, "env": srv_env })
            };
            mcp_servers.insert(srv.name.clone(), entry);
        }

        let mcp_config = serde_json::json!({ "mcpServers": mcp_servers });
        match serde_json::to_string(&mcp_config) {
            Ok(json) => {
                info!("passing MCP config as inline string");
                cmd.args(["--mcp-config", &json]);
            }
            Err(e) => warn!("failed to serialise MCP config: {e}"),
        }

        // Suppress Claude Code's built-in cron tools — our orchestrator MCP
        // provides create/list/delete/enable/disable scheduled events instead.
        if !disabled.contains("orchestrator") {
            cmd.args(["--disallowedTools", "CronCreate,CronDelete,CronList"]);
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
