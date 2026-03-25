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

        cmd.env("ORCHESTRATOR_URL", &config.orchestrator_url);
        cmd.env("ORCHESTRATOR_SESSION_ID", &config.session_id);
        if let Some(ref token) = config.orchestrator_token {
            cmd.env("ORCHESTRATOR_TOKEN", token);
        }

        // Write a per-session MCP config so Claude Code discovers the orchestrator
        // tools. Uses stdio transport (command) since Claude Code's --mcp-config
        // does not support URL-based servers. Env vars are embedded so the helper
        // subprocess receives them regardless of what Claude Code inherits.
        let mcp_config_path = format!("/tmp/orchestrator_mcp_{}.json", config.session_id);
        let mut mcp_env = serde_json::Map::new();
        mcp_env.insert(
            "ORCHESTRATOR_URL".into(),
            serde_json::Value::String(config.orchestrator_url.clone()),
        );
        mcp_env.insert(
            "ORCHESTRATOR_SESSION_ID".into(),
            serde_json::Value::String(config.session_id.clone()),
        );
        if let Some(ref token) = config.orchestrator_token {
            mcp_env.insert(
                "ORCHESTRATOR_TOKEN".into(),
                serde_json::Value::String(token.clone()),
            );
        }
        if !config.suppress_mcp_tools.is_empty() {
            mcp_env.insert(
                "ORCHESTRATOR_SUPPRESS_TOOLS".into(),
                serde_json::Value::String(config.suppress_mcp_tools.join(",")),
            );
        }
        for (k, v) in &config.mcp_extra_env {
            mcp_env.insert(k.clone(), serde_json::Value::String(v.clone()));
        }
        // Use this binary itself as the MCP helper — it handles the `mcp`
        // subcommand directly, so no separate helper binary is required.
        let helper_cmd = std::env::current_exe()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|_| {
                warn!("could not resolve current_exe, falling back to claude-client on PATH");
                "claude-client".to_string()
            });

        info!(helper = %helper_cmd, config = %mcp_config_path, "writing MCP config");

        let disabled: std::collections::HashSet<&str> =
            config.disabled_mcp_servers.iter().map(|s| s.as_str()).collect();

        let mut mcp_servers = serde_json::Map::new();

        // Built-in orchestrator server (unless disabled).
        // Uses URL-based (streamable HTTP) transport so all server-side tools
        // (create_scheduled_event, list_tasks, rename_conversation, etc.) are
        // available without duplicating them in the helper binary.
        if !disabled.contains("orchestrator") {
            let mut url = format!(
                "{}/mcp?session_id={}",
                config.orchestrator_url.trim_end_matches('/'),
                config.session_id,
            );
            if !config.suppress_mcp_tools.is_empty() {
                url.push_str(&format!("&suppress={}", config.suppress_mcp_tools.join(",")));
            }
            if let Some(emojis) = mcp_env.get("ORCHESTRATOR_ALLOWED_EMOJIS") {
                if let serde_json::Value::String(e) = emojis {
                    if !e.is_empty() {
                        // URL-encode the emoji string (percent-encode non-ASCII).
                        let encoded: String = e.bytes().flat_map(|b| {
                            if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b',' {
                                vec![b as char]
                            } else {
                                format!("%{:02X}", b).chars().collect()
                            }
                        }).collect();
                        url.push_str(&format!("&emojis={encoded}"));
                    }
                }
            }
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
            let srv_env: serde_json::Map<String, serde_json::Value> = srv
                .env
                .iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect();
            mcp_servers.insert(
                srv.name.clone(),
                serde_json::json!({
                    "command": srv.command,
                    "args": srv.args,
                    "env": srv_env
                }),
            );
        }

        let mcp_config = serde_json::json!({ "mcpServers": mcp_servers });
        match serde_json::to_string(&mcp_config) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&mcp_config_path, &json) {
                    warn!("failed to write MCP config to {mcp_config_path}: {e}");
                } else {
                    cmd.args(["--mcp-config", &mcp_config_path]);
                }
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
