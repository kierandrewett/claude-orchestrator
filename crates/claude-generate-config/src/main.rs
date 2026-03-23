use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context, Result};

const DEFAULT_IMAGE: &str = "orchestrator/claude-code:base";
const DEFAULT_AUTH_DIR: &str = "~/.local/share/claude-orchestrator/auth";

fn main() -> Result<()> {
    let image = std::env::var("CLAUDE_IMAGE").unwrap_or_else(|_| DEFAULT_IMAGE.to_string());
    let auth_dir = expand_tilde(
        &std::env::var("CLAUDE_AUTH_DIR").unwrap_or_else(|_| DEFAULT_AUTH_DIR.to_string()),
    );

    std::fs::create_dir_all(&auth_dir)
        .with_context(|| format!("creating auth dir {}", auth_dir.display()))?;

    // Pre-seed /home/claude/.claude.json so Claude Code skips the theme wizard,
    // workspace trust dialogs, and other first-run prompts.
    let global_cfg = auth_dir.join("global.json");
    std::fs::write(
        &global_cfg,
        serde_json::json!({
            "hasCompletedOnboarding": true,
            "lastOnboardingVersion": "2.1.2",
            "numStartups": 1,
            "autoUpdates": false,
            "projects": {
                "/workspace": {
                    "hasTrustDialogAccepted": true,
                    "projectOnboardingSeenCount": 1,
                    "allowedTools": [],
                    "mcpContextUris": [],
                    "mcpServers": {},
                    "enabledMcpjsonServers": [],
                    "disabledMcpjsonServers": [],
                    "hasClaudeMdExternalIncludesApproved": false,
                    "hasClaudeMdExternalIncludesWarningShown": false
                },
                "/tmp": {
                    "hasTrustDialogAccepted": true,
                    "projectOnboardingSeenCount": 1,
                    "allowedTools": [],
                    "mcpContextUris": [],
                    "mcpServers": {},
                    "enabledMcpjsonServers": [],
                    "disabledMcpjsonServers": [],
                    "hasClaudeMdExternalIncludesApproved": false,
                    "hasClaudeMdExternalIncludesWarningShown": false
                }
            }
        })
        .to_string(),
    )
    .context("writing global.json")?;

    let claude_dir_mount = format!("{}:/home/claude/.claude", auth_dir.display());
    let global_cfg_mount = format!("{}:/home/claude/.claude.json", global_cfg.display());

    eprintln!("image:    {image}");
    eprintln!("auth dir: {}", auth_dir.display());
    eprintln!();

    let status = Command::new("docker")
        .args([
            "run", "--rm", "-it",
            "--entrypoint", "sh",
            "-w", "/tmp",
            "-v", &claude_dir_mount,
            "-v", &global_cfg_mount,
            &image,
            "-c", "claude login",
        ])
        .status()
        .context("running docker — is the daemon running?")?;

    if !status.success() {
        bail!("docker exited with {status}");
    }

    let creds = auth_dir.join(".credentials.json");
    if !creds.exists() {
        bail!(
            "login appeared to succeed but {} was not created — did you complete the OAuth flow?",
            creds.display()
        );
    }

    eprintln!();
    eprintln!("credentials saved to {}", creds.display());
    Ok(())
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/"))
            .join(rest)
    } else {
        PathBuf::from(path)
    }
}
