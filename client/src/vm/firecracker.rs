//! Firecracker microVM management via the `firecracker` crate.
//!
//! When `enable_network` is true, Firecracker is started inside a private
//! network namespace (`unshare --net`) and `slirp4netns` is used to provide
//! internet access without any root privileges.

use anyhow::{Context, Result};
use std::num::NonZeroU64;
use std::process::Stdio;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tracing::info;

use firecracker::sdk::types::{
    BootSource, Drive, DriveCacheType, DriveIoEngine, MachineConfiguration, NetworkInterface,
    Vsock,
};
use firecracker::sdk::VmBuilder;

/// A running Firecracker VM. Both child processes are killed on drop.
pub struct FirecrackerVm {
    pub child: Child,
    pub vsock_socket: String,
    /// slirp4netns process, kept alive for the duration of the VM session.
    slirp_child: Option<Child>,
}

/// Configuration for one extra virtio-blk drive (used for volume images).
pub struct DriveSpec {
    pub drive_id: String,
    pub image_path: String,
    pub readonly: bool,
}

impl FirecrackerVm {
    /// Spawn Firecracker, configure the VM via the SDK, and start it.
    ///
    /// When `enable_network` is `true`:
    ///  - Firecracker is started inside a private network namespace via `unshare --net`
    ///  - `slirp4netns` is spawned to provide internet access (no root required)
    ///  - A `tap0` device is created inside the namespace; the guest should use DHCP
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
        enable_network: bool,
    ) -> Result<Self> {
        // Remove stale sockets from a previous run.
        let _ = std::fs::remove_file(api_socket);
        let _ = std::fs::remove_file(vsock_socket);

        // When networking is requested, run Firecracker inside a private
        // user+network namespace via `unshare --user --map-root-user --net`.
        // The user namespace is required so that slirp4netns (which runs as the
        // same unprivileged user) can enter the network namespace without needing
        // CAP_SYS_ADMIN. --map-root-user makes the process appear as uid 0
        // inside the namespace (but it's still the real user on the host).
        let child = if enable_network {
            Command::new("unshare")
                .args(["--user", "--map-root-user", "--net", "--", firecracker_path, "--api-sock", api_socket])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("spawn firecracker via unshare --net"))?
        } else {
            Command::new(firecracker_path)
                .arg("--api-sock")
                .arg(api_socket)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .kill_on_drop(true)
                .spawn()
                .with_context(|| format!("spawn firecracker at {firecracker_path}"))?
        };

        info!("vm: firecracker spawned (pid {:?}), waiting for API socket", child.id());

        // If networking is enabled, start slirp4netns against the Firecracker
        // process. It will create `tap0` inside Firecracker's network namespace
        // and handle all user-space NAT — no root required.
        let slirp_child = if enable_network {
            let fc_pid = child
                .id()
                .ok_or_else(|| anyhow::anyhow!("could not get Firecracker PID"))?;

            Some(start_slirp4netns(fc_pid).await.context("start slirp4netns")?)
        } else {
            None
        };

        // Wait for the API socket to appear.
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

        if enable_network {
            builder = builder.network_interface(NetworkInterface {
                iface_id: "eth0".to_string(),
                host_dev_name: "tap0".to_string(), // created by slirp4netns
                guest_mac: None,
                rx_rate_limiter: None,
                tx_rate_limiter: None,
            });
        }

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

        builder
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
            .context("start Firecracker VM")?;

        info!("vm: started");

        Ok(Self {
            child,
            vsock_socket: vsock_socket.to_string(),
            slirp_child,
        })
    }

    /// Kill the Firecracker process (and slirp4netns if running).
    pub async fn kill(&mut self) {
        if let Some(ref mut s) = self.slirp_child {
            let _ = s.kill().await;
        }
        let _ = self.child.kill().await;
    }

    /// Wait for the Firecracker process to exit and return its exit code.
    pub async fn wait(mut self) -> i32 {
        // slirp4netns exits on its own when Firecracker's netns disappears.
        if let Some(mut s) = self.slirp_child.take() {
            let _ = s.wait().await;
        }
        match self.child.wait().await {
            Ok(s) => s.code().unwrap_or(-1),
            Err(e) => {
                tracing::warn!("vm: wait() error: {e}");
                -1
            }
        }
    }
}

/// Spawn slirp4netns targeting `fc_pid`'s network namespace.
///
/// Waits until slirp4netns prints "network [N] configured" to stdout,
/// indicating that `tap0` is up and the user-space NAT is running.
/// Spawn slirp4netns targeting `fc_pid`'s network namespace and wait for it
/// to initialise. Rather than parsing stdout (unreliable across versions),
/// we give it 500 ms then check whether the process is still alive:
///  - still running  → successfully daemonised, tap0 is up
///  - exited quickly → startup error; stderr is collected and reported
async fn start_slirp4netns(fc_pid: u32) -> Result<Child> {
    info!("vm: starting slirp4netns for pid {fc_pid}");

    let mut slirp = Command::new("slirp4netns")
        .args([
            "--configure",         // auto-configure tap0 with IP 10.0.2.1/24
            "--mtu=65520",
            "--disable-host-loopback",
            &fc_pid.to_string(),
            "tap0",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("spawn slirp4netns")?;

    // Give it time to initialise (or fail fast on a permission error).
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;

    match slirp.try_wait().context("slirp4netns wait")? {
        Some(status) => {
            // Exited already — collect stderr and report the real error.
            let mut err_output = String::new();
            if let Some(mut stderr) = slirp.stderr.take() {
                let _ = tokio::time::timeout(
                    std::time::Duration::from_millis(200),
                    tokio::io::AsyncReadExt::read_to_string(&mut stderr, &mut err_output),
                )
                .await;
            }
            let detail = err_output.trim();
            if detail.is_empty() {
                anyhow::bail!("slirp4netns exited immediately (exit {})", status);
            } else {
                anyhow::bail!("slirp4netns failed (exit {}): {detail}", status);
            }
        }
        None => {
            // Still running — startup succeeded, tap0 is configured.
            info!("vm: slirp4netns running (tap0 up, NAT active)");
            Ok(slirp)
        }
    }
}
