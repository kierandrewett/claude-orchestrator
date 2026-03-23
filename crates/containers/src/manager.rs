use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use claude_ndjson::{ClaudeEvent, NdjsonTransport, UserInput};
use tracing::{info, warn};

use crate::auth::AuthManager;
use crate::config::{new_session_id, ContainerConfig, NetworkMode, SessionData};
use crate::handle::ContainerHandle;

/// A slash command discovered by running `/help` inside a container.
#[derive(Debug, Clone)]
pub struct SlashCommand {
    pub name: String,
    pub description: String,
}

/// Manages Docker container lifecycle for Claude Code sessions.
pub struct ContainerManager {
    pub docker: bollard::Docker,
    pub auth: AuthManager,
    /// Path to the directory containing profile TOML files.
    pub profiles_dir: PathBuf,
}

impl ContainerManager {
    /// Create a new `ContainerManager` connecting to the Docker socket.
    pub fn new(socket: &str, auth: AuthManager, profiles_dir: PathBuf) -> Result<Self> {
        let docker = if socket == "unix:///var/run/docker.sock" {
            bollard::Docker::connect_with_socket_defaults()
                .context("connecting to Docker socket")?
        } else {
            bollard::Docker::connect_with_socket(socket, 120, bollard::API_DEFAULT_VERSION)
                .context("connecting to Docker socket")?
        };

        Ok(Self {
            docker,
            auth,
            profiles_dir,
        })
    }

    /// Spawn a new container and return a handle with an attached NdjsonTransport.
    pub async fn spawn(
        &self,
        config: ContainerConfig,
        initial_prompt: Option<String>,
        resume_session: Option<String>,
    ) -> Result<ContainerHandle> {
        let claude_session_id = resume_session
            .clone()
            .unwrap_or_else(new_session_id);

        // Build mounts, always adding the auth credentials.
        let mut mounts = config.mounts.clone();
        mounts.push(crate::config::MountPoint {
            host_path: self.auth.claude_home_path.clone(),
            container_path: PathBuf::from("/home/claude/.claude"),
            read_only: false,
        });

        let bollard_mounts: Vec<Mount> = mounts
            .iter()
            .map(|m| Mount {
                target: Some(m.container_path.to_string_lossy().to_string()),
                source: Some(m.host_path.to_string_lossy().to_string()),
                typ: Some(MountTypeEnum::BIND),
                read_only: Some(m.read_only),
                ..Default::default()
            })
            .collect();

        let network_mode = match config.network {
            NetworkMode::Bridge => "bridge",
            NetworkMode::None => "none",
            NetworkMode::Host => "host",
        };

        // Build entrypoint args.
        let mut entrypoint_args: Vec<String> = vec![];
        if let Some(ref session_id) = resume_session {
            entrypoint_args.extend_from_slice(&["--resume".to_string(), session_id.clone()]);
        } else {
            entrypoint_args.extend_from_slice(&[
                "--session-id".to_string(),
                claude_session_id.clone(),
            ]);
        }

        let env_strings: Vec<String> = config
            .env
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        let env_refs: Vec<&str> = env_strings.iter().map(|s| s.as_str()).collect();

        let container_config = Config {
            image: Some(config.image.as_str()),
            cmd: if entrypoint_args.is_empty() {
                None
            } else {
                Some(entrypoint_args.iter().map(|s| s.as_str()).collect())
            },
            env: Some(env_refs),
            working_dir: Some(config.workdir.as_str()),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(true),
            open_stdin: Some(true),
            stdin_once: Some(false),
            host_config: Some(HostConfig {
                mounts: Some(bollard_mounts),
                network_mode: Some(network_mode.to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(None::<CreateContainerOptions<String>>, container_config)
            .await
            .context("creating container")?;

        let container_id = container.id;
        info!("container: created {container_id}");

        self.docker
            .start_container(&container_id, None::<StartContainerOptions<String>>)
            .await
            .context("starting container")?;

        info!("container: started {container_id}");

        // Attach stdin/stdout.
        let attach = self
            .docker
            .attach_container(
                &container_id,
                Some(AttachContainerOptions::<String> {
                    stdin: Some(true),
                    stdout: Some(true),
                    stderr: Some(false),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .context("attaching to container")?;

        // The bollard attach gives us a demuxed stream + a stdin write handle.
        // We wrap these into an NdjsonTransport.
        let transport = NdjsonTransport::from_bollard_attach(attach);

        let session_data = SessionData::new(container_id.clone(), claude_session_id, config);
        let mut handle = ContainerHandle::new(container_id, session_data, transport);

        // Send the initial prompt if provided.
        if let Some(prompt) = initial_prompt {
            handle
                .transport
                .send(&UserInput::user(prompt))
                .await
                .context("sending initial prompt")?;
        }

        Ok(handle)
    }

    /// Stop a container (SIGTERM → docker stop), returning its SessionData.
    pub async fn hibernate(&self, handle: ContainerHandle) -> Result<SessionData> {
        let container_id = handle.container_id.clone();
        let session_data = handle.session_data.clone();
        info!("container: hibernating {container_id}");

        // Drop the handle (and its transport) so the stdin pipe closes.
        drop(handle);

        self.docker
            .stop_container(
                &container_id,
                Some(StopContainerOptions { t: 10 }),
            )
            .await
            .with_context(|| format!("stopping container {container_id}"))?;

        Ok(session_data)
    }

    /// Restart a hibernated container and reattach.
    pub async fn wake(&self, session_data: &SessionData) -> Result<ContainerHandle> {
        let container_id = &session_data.container_id;
        info!("container: waking {container_id}");

        self.docker
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await
            .with_context(|| format!("starting container {container_id}"))?;

        let attach = self
            .docker
            .attach_container(
                container_id,
                Some(AttachContainerOptions::<String> {
                    stdin: Some(true),
                    stdout: Some(true),
                    stderr: Some(false),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .context("attaching to woken container")?;

        let transport = NdjsonTransport::from_bollard_attach(attach);
        Ok(ContainerHandle::new(
            container_id.clone(),
            session_data.clone(),
            transport,
        ))
    }

    /// Force-remove a container.
    pub async fn destroy(&self, container_id: &str) -> Result<()> {
        info!("container: destroying {container_id}");
        self.docker
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
            .with_context(|| format!("removing container {container_id}"))?;
        Ok(())
    }

    /// Recreate a container from stored session data (e.g. after a crash).
    pub async fn recreate(&self, session_data: &SessionData) -> Result<ContainerHandle> {
        info!(
            "container: recreating from session {}",
            session_data.claude_session_id
        );

        // Try to remove the old container if it still exists.
        let _ = self.destroy(&session_data.container_id).await;

        self.spawn(
            session_data.config.clone(),
            None,
            Some(session_data.claude_session_id.clone()),
        )
        .await
    }

    /// Run `/help` inside a one-shot container and return the slash commands Claude exposes.
    ///
    /// Creates a temporary container from `image`, sends `/help` via stream-json stdin,
    /// parses the text response, then destroys the container.
    pub async fn discover_slash_commands(&self, image: &str) -> Result<Vec<SlashCommand>> {
        let container_config = Config {
            image: Some(image),
            // Run in --print (non-interactive) stream-json mode, same as the client did locally.
            cmd: Some(vec![
                "--print",
                "--input-format",
                "stream-json",
                "--output-format",
                "stream-json",
                "--dangerously-skip-permissions",
            ]),
            attach_stdin: Some(true),
            attach_stdout: Some(true),
            attach_stderr: Some(false),
            open_stdin: Some(true),
            stdin_once: Some(true),
            host_config: Some(HostConfig {
                mounts: Some(vec![Mount {
                    target: Some("/home/claude/.claude".to_string()),
                    source: Some(self.auth.claude_home_path.to_string_lossy().to_string()),
                    typ: Some(MountTypeEnum::BIND),
                    read_only: Some(false),
                    ..Default::default()
                }]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let container = self
            .docker
            .create_container(None::<CreateContainerOptions<String>>, container_config)
            .await
            .context("creating slash-command discovery container")?;
        let container_id = container.id;
        info!("slash-command discovery: created container {container_id}");

        let result = self.run_help_query(&container_id).await;
        let _ = self.destroy(&container_id).await;
        result
    }

    async fn run_help_query(&self, container_id: &str) -> Result<Vec<SlashCommand>> {
        let attach = self
            .docker
            .attach_container(
                container_id,
                Some(AttachContainerOptions::<String> {
                    stdin: Some(true),
                    stdout: Some(true),
                    stderr: Some(false),
                    stream: Some(true),
                    ..Default::default()
                }),
            )
            .await
            .context("attaching to discovery container")?;

        self.docker
            .start_container(container_id, None::<StartContainerOptions<String>>)
            .await
            .context("starting discovery container")?;

        let mut transport = NdjsonTransport::from_bollard_attach(attach);
        transport
            .send(&UserInput::user("/help"))
            .await
            .context("sending /help to discovery container")?;
        transport.close_stdin().await;

        let mut full_text = String::new();
        let read_fut = async {
            loop {
                match transport.next_event().await {
                    Ok(Some(ClaudeEvent::Result(r))) => {
                        if let Some(text) = r.result {
                            full_text = text;
                        }
                        break;
                    }
                    Ok(None) => break,
                    Ok(Some(_)) => continue,
                    Err(e) => {
                        warn!("slash-command discovery: read error: {e}");
                        break;
                    }
                }
            }
        };

        if tokio::time::timeout(Duration::from_secs(30), read_fut)
            .await
            .is_err()
        {
            warn!("slash-command discovery: timed out");
        }

        Ok(parse_slash_commands(&full_text))
    }

    /// Check whether a container exists and is running.
    pub async fn is_running(&self, container_id: &str) -> bool {
        match self.docker.inspect_container(container_id, None).await {
            Ok(info) => info
                .state
                .and_then(|s| s.running)
                .unwrap_or(false),
            Err(_) => false,
        }
    }
}

fn parse_slash_commands(text: &str) -> Vec<SlashCommand> {
    let mut commands = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.starts_with('/') {
            continue;
        }
        if let Some((name, desc)) = line.split_once(" - ").or_else(|| line.split_once(": ")) {
            let name = name.trim().to_string();
            if name.starts_with('/') && !name.contains(' ') {
                commands.push(SlashCommand { name, description: desc.trim().to_string() });
            }
        } else {
            let name = line.split_whitespace().next().unwrap_or("").to_string();
            if name.starts_with('/') {
                commands.push(SlashCommand { name, description: String::new() });
            }
        }
    }
    commands
}
