//! Firecracker microVM management via the `firecracker` crate.

use anyhow::{Context, Result};
use std::num::NonZeroU64;
use tokio::process::{Child, Command};
use tracing::info;

use firecracker::sdk::types::{BootSource, Drive, DriveCacheType, DriveIoEngine, MachineConfiguration, Vsock};
use firecracker::sdk::VmBuilder;

/// A running Firecracker VM. The child process is killed on drop.
pub struct FirecrackerVm {
    pub child: Child,
    pub vsock_socket: String,
}

/// Configuration for one extra virtio-blk drive (used for volume images).
pub struct DriveSpec {
    pub drive_id: String,
    pub image_path: String,
    pub readonly: bool,
}

impl FirecrackerVm {
    /// Spawn Firecracker, configure the VM via the SDK, and start it.
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

        info!("vm: firecracker spawned, waiting for API socket");

        // Wait for the socket file to appear before connecting.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if std::path::Path::new(api_socket).exists() {
                break;
            }
            if std::time::Instant::now() >= deadline {
                anyhow::bail!("timed out waiting for Firecracker API socket {api_socket}");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        // Small settling delay so micro-http is ready to accept connections.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let mut builder = VmBuilder::new(api_socket)
            .boot_source(BootSource {
                kernel_image_path: kernel_path.to_string(),
                boot_args: Some(boot_args.to_string()),
                initrd_path: None,
            })
            .machine_config(MachineConfiguration {
                vcpu_count: NonZeroU64::new(vcpus as u64).unwrap_or(NonZeroU64::new(2).unwrap()),
                mem_size_mib: memory_mb as i64,
                smt: false,
                track_dirty_pages: false,
                cpu_template: None,
                huge_pages: None,
            })
            .drive(Drive {
                drive_id: "rootfs".to_string(),
                path_on_host: Some(rootfs_path.to_string()),
                is_root_device: true,
                is_read_only: Some(false),
                partuuid: None,
                cache_type: DriveCacheType::Unsafe,
                io_engine: DriveIoEngine::Sync,
                rate_limiter: None,
                socket: None,
            })
            .vsock(Vsock {
                guest_cid: 3,
                uds_path: vsock_socket.to_string(),
                vsock_id: None,
            });

        for spec in drives {
            builder = builder.drive(Drive {
                drive_id: spec.drive_id.clone(),
                path_on_host: Some(spec.image_path.clone()),
                is_root_device: false,
                is_read_only: Some(spec.readonly),
                partuuid: None,
                cache_type: DriveCacheType::Unsafe,
                io_engine: DriveIoEngine::Sync,
                rate_limiter: None,
                socket: None,
            });
        }

        builder.start().await.map_err(|e| anyhow::anyhow!("{e}")).context("start Firecracker VM")?;

        info!("vm: started");

        Ok(Self {
            child,
            vsock_socket: vsock_socket.to_string(),
        })
    }

    /// Kill the Firecracker process.
    pub async fn kill(&mut self) {
        let _ = self.child.kill().await;
    }

    /// Wait for the Firecracker process to exit and return its exit code.
    pub async fn wait(mut self) -> i32 {
        match self.child.wait().await {
            Ok(s) => s.code().unwrap_or(-1),
            Err(e) => {
                tracing::warn!("vm: wait() error: {e}");
                -1
            }
        }
    }
}
