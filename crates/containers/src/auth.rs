use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Stored credentials from the Claude OAuth flow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthCredentials {
    #[serde(rename = "claudeAiOauth")]
    pub claude_ai_oauth: OAuthTokens,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    #[serde(rename = "accessToken")]
    pub access_token: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "expiresAt")]
    pub expires_at: u64,
    pub scopes: Vec<String>,
}

/// Manages Claude Code authentication credentials.
///
/// Credentials are captured via an interactive OAuth flow run inside a
/// temporary container, then stored in `claude_home_path` and bind-mounted
/// into every subsequent container at `/home/claude/.claude`.
pub struct AuthManager {
    /// Path to `.credentials.json` inside the captured auth directory.
    pub credentials_path: PathBuf,
    /// The full captured `~/.claude/` directory (bind-mounted into containers).
    pub claude_home_path: PathBuf,
}

impl AuthManager {
    pub fn new(credentials_dir: PathBuf) -> Self {
        let credentials_path = credentials_dir.join(".credentials.json");
        Self {
            credentials_path,
            claude_home_path: credentials_dir,
        }
    }

    /// Check whether credentials are stored on disk.
    pub fn has_credentials(&self) -> bool {
        self.credentials_path.exists()
    }

    /// Load stored credentials.
    pub fn load(&self) -> Result<AuthCredentials> {
        let data = std::fs::read_to_string(&self.credentials_path)
            .with_context(|| format!("reading credentials from {}", self.credentials_path.display()))?;
        serde_json::from_str(&data).context("parsing credentials JSON")
    }

    /// Check whether the stored credentials look valid (non-empty refresh token).
    pub fn credentials_look_valid(&self) -> bool {
        self.load()
            .map(|c| !c.claude_ai_oauth.refresh_token.is_empty())
            .unwrap_or(false)
    }

    /// Run the interactive OAuth login flow inside a temporary Docker container.
    ///
    /// Runs `docker run -it` with the credentials directory mounted as a volume
    /// so the user can complete the OAuth flow interactively and credentials are
    /// written directly to the host mount (no copy-from-container needed).
    pub async fn login(&self, _docker: &bollard::Docker, image: &str) -> Result<AuthCredentials> {
        std::fs::create_dir_all(&self.claude_home_path)
            .context("creating auth credentials directory")?;

        // Pre-seed .claude.json so Claude Code skips the first-run theme wizard.
        // This file lives at /home/claude/.claude.json inside the container, which
        // is the parent of our .claude/ mount, so we keep a copy in the credentials
        // dir and bind-mount it separately.
        //
        // Always overwrite so that stale seeds (e.g. missing hasCompletedOnboarding)
        // get corrected on the next login attempt.
        let global_cfg_path = self.claude_home_path.join("global.json");
        // Include /workspace as a trusted project so real sessions skip the
        // safety-check dialog.  The login container uses -w /tmp to avoid
        // triggering that dialog during setup itself.
        std::fs::write(
            &global_cfg_path,
            r#"{"hasCompletedOnboarding":true,"lastOnboardingVersion":"2.1.2","numStartups":1,"autoUpdates":false,"projects":{"/workspace":{"hasTrustDialogAccepted":true,"projectOnboardingSeenCount":1,"allowedTools":[],"mcpContextUris":[],"mcpServers":{},"enabledMcpjsonServers":[],"disabledMcpjsonServers":[],"hasClaudeMdExternalIncludesApproved":false,"hasClaudeMdExternalIncludesWarningShown":false},"/tmp":{"hasTrustDialogAccepted":true,"projectOnboardingSeenCount":1,"allowedTools":[],"mcpContextUris":[],"mcpServers":{},"enabledMcpjsonServers":[],"disabledMcpjsonServers":[],"hasClaudeMdExternalIncludesApproved":false,"hasClaudeMdExternalIncludesWarningShown":false}}}"#,
        )
        .context("pre-seeding global.json")?;

        info!("auth: starting interactive login container");

        let claude_dir_mount = format!(
            "{}:/home/claude/.claude",
            self.claude_home_path.display()
        );
        let global_cfg_mount = format!(
            "{}:/home/claude/.claude.json",
            global_cfg_path.display()
        );

        // Run interactively so the user can complete the OAuth flow in their
        // terminal.  The credentials directory is mounted directly so no
        // copy-from-container step is required.
        let status = tokio::process::Command::new("docker")
            .args([
                "run", "--rm", "-it",
                "--entrypoint", "sh",
                "-w", "/tmp",
                "-v", &claude_dir_mount,
                "-v", &global_cfg_mount,
                image,
                "-c", "claude login",
            ])
            .status()
            .await
            .context("running docker login container")?;

        if !status.success() {
            bail!("auth: docker login container exited with {status}");
        }

        let creds = self.load().context("loading credentials after login")?;
        info!("auth: credentials saved to {}", self.claude_home_path.display());
        Ok(creds)
    }
}

