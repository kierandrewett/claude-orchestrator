use std::path::PathBuf;

use anyhow::{Context, Result};
use bollard::container::{
    AttachContainerOptions, Config, CreateContainerOptions, RemoveContainerOptions,
    StartContainerOptions, StopContainerOptions,
};
use bollard::models::{HostConfig, Mount, MountTypeEnum};
use claude_ndjson::{NdjsonTransport, UserInput};
use tracing::info;

use crate::auth::AuthManager;
use crate::config::{new_session_id, ContainerConfig, NetworkMode, SessionData};
use crate::handle::ContainerHandle;

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
