use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

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
    /// Spins up a container with `claude login`, watches stdout for the OAuth
    /// URL, prints it for the user to open, waits for credentials to appear,
    /// copies them out, and destroys the container.
    pub async fn login(&self, docker: &bollard::Docker, image: &str) -> Result<AuthCredentials> {
        use bollard::container::{Config, CreateContainerOptions, StartContainerOptions};
        use bollard::exec::{CreateExecOptions, StartExecResults};

        std::fs::create_dir_all(&self.claude_home_path)
            .context("creating auth credentials directory")?;

        let container_name = format!("claude-auth-{}", uuid::Uuid::new_v4());

        info!("auth: creating temporary login container {container_name}");

        // Create a container that runs `claude login` (not the NDJSON entrypoint).
        let config = Config {
            image: Some(image),
            cmd: Some(vec!["sh", "-c", "claude login"]),
            entrypoint: Some(vec!["sh", "-c", "claude login"]),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            tty: Some(false),
            ..Default::default()
        };

        let create_result = docker
            .create_container(
                Some(CreateContainerOptions {
                    name: container_name.as_str(),
                    platform: None,
                }),
                config,
            )
            .await
            .context("creating auth container")?;

        let container_id = create_result.id;

        // Ensure we clean up even on error.
        let cleanup = CleanupGuard {
            docker: docker.clone(),
            container_id: container_id.clone(),
        };

        docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .context("starting auth container")?;

        // Attach to stdout to watch for the OAuth URL.
        use bollard::container::AttachContainerOptions;
        let mut attach = docker
            .attach_container(
                &container_id,
                Some(AttachContainerOptions::<String> {
                    stdout: Some(true),
                    stderr: Some(true),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .context("attaching to auth container")?;

        use futures_util::StreamExt;
        let timeout = tokio::time::Duration::from_secs(300);
        let deadline = tokio::time::Instant::now() + timeout;

        // Read lines until we find the OAuth URL.
        loop {
            if tokio::time::Instant::now() > deadline {
                bail!("auth: timed out waiting for OAuth URL (5 minutes)");
            }

            tokio::select! {
                output = attach.output.next() => {
                    match output {
                        None => bail!("auth: container stdout ended without producing OAuth URL"),
                        Some(Ok(msg)) => {
                            use bollard::container::LogOutput;
                            let text = match msg {
                                LogOutput::StdOut { message } | LogOutput::StdErr { message } => {
                                    String::from_utf8_lossy(&message).to_string()
                                }
                                _ => continue,
                            };
                            // Print all output so the user can see the URL.
                            print!("{text}");
                            if text.contains("claude.ai/oauth") || text.contains("https://") {
                                println!("\n\nWaiting for authentication to complete...");
                                break;
                            }
                        }
                        Some(Err(e)) => bail!("auth: container output error: {e}"),
                    }
                }
                _ = tokio::time::sleep_until(deadline) => {
                    bail!("auth: timed out waiting for OAuth URL");
                }
            }
        }

        // Poll for the credentials file to appear inside the container.
        let credentials_inside = "/root/.claude/.credentials.json";
        for _ in 0..120 {
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

            let exec = docker
                .create_exec(
                    &container_id,
                    CreateExecOptions {
                        cmd: Some(vec!["test", "-f", credentials_inside]),
                        attach_stdout: Some(false),
                        attach_stderr: Some(false),
                        ..Default::default()
                    },
                )
                .await
                .context("creating exec to check credentials")?;

            let result = docker
                .start_exec(&exec.id, None)
                .await
                .context("running exec to check credentials")?;

            if let StartExecResults::Detached = result {
                // Wait for the exec to finish.
                let inspect = docker.inspect_exec(&exec.id).await.ok();
                let exit_code = inspect
                    .and_then(|i| i.exit_code)
                    .unwrap_or(1);

                if exit_code == 0 {
                    info!("auth: credentials file appeared in container");
                    break;
                }
            }
        }

        // Copy the credentials file out of the container.
        self.copy_credentials_from_container(docker, &container_id, credentials_inside)
            .await
            .context("copying credentials from container")?;

        let creds = self.load().context("loading copied credentials")?;
        info!("auth: credentials saved to {}", self.claude_home_path.display());

        // cleanup runs on drop — destroys the container.
        drop(cleanup);

        Ok(creds)
    }

    async fn copy_credentials_from_container(
        &self,
        docker: &bollard::Docker,
        container_id: &str,
        src_path: &str,
    ) -> Result<()> {
        use futures_util::StreamExt;

        let mut stream = docker.download_from_container(
            container_id,
            Some(bollard::container::DownloadFromContainerOptions { path: src_path }),
        );

        let mut tar_data = Vec::new();
        while let Some(chunk) = stream.next().await {
            tar_data.extend_from_slice(&chunk.context("reading tar chunk")?);
        }

        // Unpack the single-file tar into our credentials path.
        let cursor = std::io::Cursor::new(tar_data);
        let mut archive = tar::Archive::new(cursor);
        if let Some(entry) = archive.entries().context("reading tar entries")?.next() {
            let mut entry = entry.context("reading tar entry")?;
            let dest = self.credentials_path.clone();
            std::fs::create_dir_all(dest.parent().unwrap()).ok();
            entry
                .unpack(&dest)
                .context("unpacking credentials file")?;
        }

        Ok(())
    }
}

/// RAII guard that destroys the container when dropped.
struct CleanupGuard {
    docker: bollard::Docker,
    container_id: String,
}

impl Drop for CleanupGuard {
    fn drop(&mut self) {
        let docker = self.docker.clone();
        let id = self.container_id.clone();
        // Spawn a detached cleanup task.
        tokio::spawn(async move {
            warn!("auth: removing temporary container {id}");
            let _ = docker
                .remove_container(
                    &id,
                    Some(bollard::container::RemoveContainerOptions {
                        force: true,
                        ..Default::default()
                    }),
                )
                .await;
        });
    }
}
