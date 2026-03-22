//! Firecracker microVM session orchestrator.
//!
//! `run_vm_session` is a drop-in replacement for `session_runner::run_session`
//! that executes Claude inside an isolated Alpine microVM.
//!
//! # Session flow
//! 1. Prepare ext4 volume images (rsync host → image via fuse2fs).
//! 2. Start Firecracker with rootfs + volume drives + vsock device.
//! 3. Connect to the vsock port 5000 where `vm-agent` is listening.
//! 4. Send a JSON config line so the agent knows which session to start.
//! 5. Relay I/O: WebSocket commands → vsock stdin; vsock stdout → SessionEvent.
//! 6. On EOF (claude exited) or KillSession: shut down VM, sync volumes back.

pub mod config;
pub mod firecracker;
pub mod rootfs;
pub mod volume;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, Mutex};
use tracing::{info, warn};

use crate::protocol::{C2S, S2C, SessionStats};
use crate::session_runner::{update_stats, ws_send, WsSink};
use config::VmConfig;
use firecracker::{DriveSpec, FirecrackerVm};

// ---------------------------------------------------------------------------
// Wire config sent to vm-agent over vsock
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct VmAgentConfig<'a> {
    session_id: &'a str,
    initial_prompt: Option<&'a str>,
    claude_session_id: &'a str,
    is_resume: bool,
    cwd: &'a str,
    extra_args: &'a [String],
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Run a Claude session inside a Firecracker VM.
/// Mirrors the signature and behaviour of `session_runner::run_session`.
pub async fn run_vm_session(
    config: crate::session_runner::SessionConfig,
    ws_tx: Arc<Mutex<WsSink>>,
    mut cmd_rx: mpsc::Receiver<S2C>,
    vm_cfg: VmConfig,
) {
    let session_id = config.session_id.clone();
    info!("vm: starting session {session_id}");

    if let Err(e) = do_run(&config, &ws_tx, &mut cmd_rx, &vm_cfg).await {
        warn!("vm: session {session_id} error: {e:#}");
    }

    // Always report session ended to the server.
    ws_send(
        &ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code: -1,
            stats: SessionStats::default(),
            error: None,
        },
    )
    .await;

    info!("vm: session {session_id} cleaned up");
}

// ---------------------------------------------------------------------------
// Inner implementation
// ---------------------------------------------------------------------------

async fn do_run(
    config: &crate::session_runner::SessionConfig,
    ws_tx: &Arc<Mutex<WsSink>>,
    cmd_rx: &mut mpsc::Receiver<S2C>,
    vm_cfg: &VmConfig,
) -> Result<()> {
    let session_id = &config.session_id;

    // --- 1. Prepare volume images (rsync host → ext4) ---
    let mut drives: Vec<DriveSpec> = Vec::new();
    for mount in &vm_cfg.mounts {
        let image = vm_cfg.volume_image_path(&mount.name);
        info!(
            "vm: preparing volume '{}' ({} → {})",
            mount.name, mount.host_path, mount.guest_path
        );
        volume::prepare(
            &image,
            &PathBuf::from(&mount.host_path),
            mount.size_gb,
            &mount.excludes,
        )
        .await
        .with_context(|| format!("prepare volume '{}'", mount.name))?;

        drives.push(DriveSpec {
            drive_id: format!("vol{}", drives.len()),
            image_path: image.to_string_lossy().into_owned(),
            readonly: false,
        });
    }

    // --- 2. Build boot args with mount map ---
    let boot_args = build_boot_args(&vm_cfg.mounts);

    // --- 3. Unique per-session socket paths ---
    let tmp = std::env::temp_dir();
    let api_sock = tmp
        .join(format!("fc-api-{session_id}.sock"))
        .to_string_lossy()
        .into_owned();
    let vsock_sock = tmp
        .join(format!("fc-vsock-{session_id}.sock"))
        .to_string_lossy()
        .into_owned();

    // --- 4. Start Firecracker ---
    let mut vm = FirecrackerVm::start(
        &vm_cfg.firecracker_path,
        &api_sock,
        &vsock_sock,
        &vm_cfg.kernel_path,
        &vm_cfg.rootfs_path,
        &drives,
        vm_cfg.vcpus,
        vm_cfg.memory_mb,
        &boot_args,
    )
    .await
    .context("start Firecracker VM")?;

    // Report that the session has started (use pid=0 for VM sessions)
    ws_send(
        ws_tx,
        &C2S::SessionStarted {
            session_id: session_id.clone(),
            pid: 0,
            cwd: config.default_cwd.clone(),
        },
    )
    .await;

    // --- 5. Connect vsock (with retry — VM needs ~0.5s to boot) ---
    let vsock_stream = connect_vsock(&vsock_sock, 5000, Duration::from_secs(15))
        .await
        .context("vsock CONNECT to vm-agent")?;

    let (vsock_rx, mut vsock_tx) = vsock_stream.into_split();

    // --- 6. Send agent config ---
    let agent_cfg = VmAgentConfig {
        session_id: session_id,
        initial_prompt: config.initial_prompt.as_deref(),
        claude_session_id: &config.claude_session_id,
        is_resume: config.is_resume,
        cwd: &config.default_cwd,
        extra_args: &config.extra_args,
    };
    let mut cfg_line = serde_json::to_string(&agent_cfg).context("serialize agent config")?;
    cfg_line.push('\n');
    vsock_tx
        .write_all(cfg_line.as_bytes())
        .await
        .context("write agent config to vsock")?;

    info!("vm: vm-agent configured, relaying I/O for session {session_id}");

    // --- 7. I/O relay loop ---
    let mut stats = SessionStats::default();
    let mut stdout_lines = BufReader::new(vsock_rx).lines();

    loop {
        tokio::select! {
            biased;

            // Incoming command from server
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(S2C::SendInput { text, .. }) => {
                        let line = format_user_message(&text);
                        if let Err(e) = vsock_tx.write_all(line.as_bytes()).await {
                            warn!("vm: vsock write error: {e}");
                            break;
                        }
                    }
                    Some(S2C::SendInputWithFiles { text, files, .. }) => {
                        let formatted = crate::session_runner::format_user_message_with_files(&text, &files).await;
                        if !formatted.is_empty() {
                            if let Err(e) = vsock_tx.write_all(formatted.as_bytes()).await {
                                warn!("vm: vsock write error (files): {e}");
                                break;
                            }
                        }
                    }
                    Some(S2C::KillSession { .. }) => {
                        info!("vm: KillSession received, shutting down VM");
                        vm.kill().await;
                        // Give VM a moment before we close the streams
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        break;
                    }
                    Some(_) | None => break,
                }
            }

            // Output from claude (via vm-agent → vsock)
            line_result = stdout_lines.next_line() => {
                match line_result {
                    Ok(Some(line)) => {
                        if line.trim().is_empty() { continue; }
                        match serde_json::from_str::<serde_json::Value>(&line) {
                            Ok(event) => {
                                update_stats(&mut stats, &event);
                                ws_send(ws_tx, &C2S::SessionEvent {
                                    session_id: session_id.clone(),
                                    event,
                                }).await;
                            }
                            Err(e) => {
                                warn!("vm: failed to parse stdout JSON: {e}\nline: {line}");
                                let raw_event = serde_json::json!({
                                    "type": "raw_text",
                                    "text": line,
                                });
                                ws_send(ws_tx, &C2S::SessionEvent {
                                    session_id: session_id.clone(),
                                    event: raw_event,
                                }).await;
                            }
                        }
                    }
                    Ok(None) => {
                        // EOF — claude exited
                        info!("vm: vsock EOF, claude session ended");
                        break;
                    }
                    Err(e) => {
                        warn!("vm: vsock read error: {e}");
                        break;
                    }
                }
            }
        }
    }

    // --- 8. Shutdown VM ---
    vm.kill().await;
    let exit_code = tokio::time::timeout(Duration::from_secs(10), vm.wait())
        .await
        .unwrap_or(-1);

    // Clean up socket files
    let _ = std::fs::remove_file(&api_sock);
    let _ = std::fs::remove_file(&vsock_sock);

    // --- 9. Sync volumes back to host ---
    for mount in &vm_cfg.mounts {
        let image = vm_cfg.volume_image_path(&mount.name);
        info!("vm: syncing volume '{}' back to host", mount.name);
        if let Err(e) =
            volume::sync_back(&image, &PathBuf::from(&mount.host_path)).await
        {
            warn!("vm: sync-back '{}' failed: {e:#}", mount.name);
        }
    }

    // --- 10. Report completion ---
    ws_send(
        ws_tx,
        &C2S::SessionEnded {
            session_id: session_id.clone(),
            exit_code,
            stats,
            error: None,
        },
    )
    .await;

    Ok(())
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Connect to the Firecracker vsock host socket and perform the CONNECT
/// handshake to reach the agent listening on `port` inside the VM.
async fn connect_vsock(
    uds_path: &str,
    port: u32,
    timeout: Duration,
) -> Result<UnixStream> {
    let deadline = tokio::time::Instant::now() + timeout;

    loop {
        match tokio::net::UnixStream::connect(uds_path).await {
            Ok(mut stream) => {
                // Firecracker vsock CONNECT handshake
                let handshake = format!("CONNECT {port}\n");
                stream
                    .write_all(handshake.as_bytes())
                    .await
                    .context("vsock handshake write")?;

                let mut response = String::new();
                let mut reader = BufReader::new(&mut stream);
                tokio::time::timeout(
                    Duration::from_secs(5),
                    reader.read_line(&mut response),
                )
                .await
                .context("vsock handshake timeout")?
                .context("vsock handshake read")?;

                if response.starts_with("OK ") {
                    return Ok(stream);
                }
                anyhow::bail!("unexpected vsock response: {response}");
            }
            Err(_) if tokio::time::Instant::now() < deadline => {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            Err(e) => {
                anyhow::bail!("vsock connect to {uds_path}: {e}");
            }
        }
    }
}

/// Construct the kernel boot args string, encoding the volume mount map.
pub fn build_boot_args(mounts: &[config::VolumeMount]) -> String {
    // Firecracker assigns drives in order: /dev/vda = rootfs, /dev/vdb, /dev/vdc, ...
    let pairs: Vec<String> = mounts
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let dev = format!("/dev/vd{}", (b'b' + i as u8) as char);
            format!("{}:{}", dev, m.guest_path)
        })
        .collect();

    let vm_mounts = if pairs.is_empty() {
        String::new()
    } else {
        format!(" vm_mounts={}", pairs.join(","))
    };

    format!("console=ttyS0 reboot=k panic=1 pci=off nomodules{vm_mounts}")
}

fn format_user_message(text: &str) -> String {
    let escaped = serde_json::to_string(text).unwrap_or_default();
    format!(
        "{{\"type\":\"user\",\"message\":{{\"role\":\"user\",\"content\":{escaped}}}}}\n"
    )
}
