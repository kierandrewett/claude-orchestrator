//! Firecracker REST API client.
//!
//! All calls go over the Unix domain socket Firecracker exposes via
//! `--api-sock`, using raw HTTP/1.1 over `tokio::net::UnixStream`.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tracing::debug;
use tokio::process::{Child, Command};
use tracing::{info, warn};

/// A running Firecracker VM. `kill_on_drop` is set on the child so the VM is
/// always torn down when this value is dropped.
pub struct FirecrackerVm {
    pub child: Child,
    pub api_socket: String,
    pub vsock_socket: String,
}

/// Configuration for one extra virtio-blk drive (used for volume images).
pub struct DriveSpec {
    pub drive_id: String,
    pub image_path: String,
    pub readonly: bool,
}

impl FirecrackerVm {
    /// Spawn Firecracker, wait for its API socket, configure the VM, and start it.
    pub async fn start(
        firecracker_path: &str,
        api_socket: &str,
        vsock_socket: &str,
        kernel_path: &str,
        rootfs_path: &str,
        drives: &[DriveSpec],
        vcpus: u32,
        memory_mb: u32,
        boot_args: &str,
    ) -> Result<Self> {
        // Remove stale sockets from a previous run.
        let _ = std::fs::remove_file(api_socket);
        let _ = std::fs::remove_file(vsock_socket);

        let child = Command::new(firecracker_path)
            .arg("--api-sock")
            .arg(api_socket)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawn firecracker at {firecracker_path}"))?;

        wait_for_socket(api_socket, Duration::from_secs(5))
            .await
            .context("Firecracker API socket did not appear within 5s")?;

        info!("vm: firecracker API socket ready, configuring");

        // Boot source (kernel + args)
        api_put(api_socket, "/boot-source", json!({
            "kernel_image_path": kernel_path,
            "boot_args": boot_args,
        }))
        .await
        .context("PUT /boot-source")?;

        // Machine config
        api_put(api_socket, "/machine-config", json!({
            "vcpu_count": vcpus,
            "mem_size_mib": memory_mb,
        }))
        .await
        .context("PUT /machine-config")?;

        // Root filesystem (drive id must be "rootfs")
        api_put(api_socket, "/drives/rootfs", json!({
            "drive_id": "rootfs",
            "is_root_device": true,
            "path_on_host": rootfs_path,
            "is_read_only": false,
        }))
        .await
        .context("PUT /drives/rootfs")?;

        // Extra drives (volume images)
        for spec in drives {
            api_put(api_socket, &format!("/drives/{}", spec.drive_id), json!({
                "drive_id": spec.drive_id,
                "is_root_device": false,
                "path_on_host": spec.image_path,
                "is_read_only": spec.readonly,
            }))
            .await
            .with_context(|| format!("PUT /drives/{}", spec.drive_id))?;
        }

        // vsock device (guest CID 3, host-side Unix socket)
        api_put(api_socket, "/vsock", json!({
            "guest_cid": 3,
            "uds_path": vsock_socket,
        }))
        .await
        .context("PUT /vsock")?;

        // Start the VM
        api_put(api_socket, "/actions", json!({ "action_type": "InstanceStart" }))
            .await
            .context("PUT /actions InstanceStart")?;

        info!("vm: started");

        Ok(Self {
            child,
            api_socket: api_socket.to_string(),
            vsock_socket: vsock_socket.to_string(),
        })
    }

    /// Ask the guest to shut down gracefully (Ctrl+Alt+Del).
    pub async fn graceful_shutdown(&self) {
        let _ = api_put(
            &self.api_socket,
            "/actions",
            json!({ "action_type": "SendCtrlAltDel" }),
        )
        .await;
    }

    /// Wait for the Firecracker process to exit and return its exit code.
    pub async fn wait(mut self) -> i32 {
        match self.child.wait().await {
            Ok(s) => s.code().unwrap_or(-1),
            Err(e) => {
                warn!("vm: wait() error: {e}");
                -1
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Raw HTTP/1.1 over Unix domain socket
// ---------------------------------------------------------------------------

/// Sends a PUT request to the Firecracker API socket.
async fn api_put(socket_path: &str, path: &str, body: Value) -> Result<()> {
    debug!("firecracker: PUT {path}");
    let body_str = body.to_string();
    let request = format!(
        "PUT {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body_str}",
        body_str.len(),
    );

    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("connect to Firecracker socket {socket_path}"))?;

    let (reader, mut writer) = stream.into_split();

    writer
        .write_all(request.as_bytes())
        .await
        .context("write HTTP request")?;

    // Signal EOF on the write side so Firecracker knows the request is complete
    // and can send its response without waiting for more data.
    writer.shutdown().await.context("shutdown write half")?;

    // Read the status line to check for errors.
    let mut reader = BufReader::new(reader);
    let mut status_line = String::new();
    reader
        .read_line(&mut status_line)
        .await
        .context("read HTTP status line")?;

    let status_code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);

    debug!("firecracker: PUT {path} → {status_code}");

    if !(200..300).contains(&status_code) {
        // Read rest of response for the error body.
        let mut rest = String::new();
        while reader.read_line(&mut rest).await.unwrap_or(0) > 0 {}
        anyhow::bail!("PUT {path} → {status_code}: {}", rest.trim());
    }

    Ok(())
}

async fn wait_for_socket(path: &str, timeout: Duration) -> Result<()> {
    let deadline = Instant::now() + timeout;
    while !Path::new(path).exists() {
        if Instant::now() > deadline {
            anyhow::bail!("socket {path} did not appear");
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    Ok(())
}
