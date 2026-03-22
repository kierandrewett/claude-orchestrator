//! Alpine-based rootfs builder.
//!
//! Uses Docker to assemble an Alpine Linux root filesystem with the
//! requested packages and the compiled `vm-agent` binary, then packages
//! it as an ext4 image using `mke2fs -d`.
//!
//! Requires on the host:
//!   - `docker` (or `podman` aliased to `docker`)
//!   - `mke2fs` with `-d` directory support (e2fsprogs ≥ 1.45)

use std::path::Path;

use anyhow::{Context, Result};
use tokio::process::Command;
use tracing::info;

/// Default Alpine packages always installed regardless of user config.
const BASE_PACKAGES: &[&str] = &[
    "alpine-base",
    "busybox",
    "openrc",
    "e2fsprogs",
    "bash",
    "git",
    "curl",
    "ca-certificates",
    "rsync",
    "util-linux",
];

/// Build (or rebuild) the Alpine rootfs image at `rootfs_path`.
///
/// `extra_packages` are additional Alpine package names to install
/// (e.g. `["nodejs", "python3", "ripgrep"]`).
///
/// `vm_agent_path` is the path to the compiled static vm-agent binary on
/// the host; it is copied into `/usr/local/bin/vm-agent` in the rootfs.
///
/// If `rootfs_path` already exists it is overwritten.
pub async fn build(
    rootfs_path: &Path,
    extra_packages: &[String],
    vm_agent_path: &Path,
) -> Result<()> {
    anyhow::ensure!(
        vm_agent_path.exists(),
        "vm-agent binary not found at {} — run:\n  \
         cargo build --release --target x86_64-unknown-linux-musl -p vm-agent",
        vm_agent_path.display()
    );

    if let Some(dir) = rootfs_path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }

    // Collect all packages
    let mut pkgs: Vec<&str> = BASE_PACKAGES.to_vec();
    let extra: Vec<&str> = extra_packages.iter().map(|s| s.as_str()).collect();
    pkgs.extend_from_slice(&extra);
    let pkg_list = pkgs.join(" ");

    // Build the init script content
    let init_script = INIT_SCRIPT;

    // We build a Docker image, export its filesystem, then create an ext4 image.
    let tag = "claude-vm-rootfs-builder:latest";

    // Write Dockerfile to a temp file
    let tmp_dir = tempfile::tempdir().context("create temp dir")?;
    let dockerfile = format!(
        r#"FROM alpine:3.20
RUN apk add --no-cache {pkg_list}
# Install claude (placeholder — user must pre-install or bind-mount)
COPY vm-agent /usr/local/bin/vm-agent
RUN chmod +x /usr/local/bin/vm-agent
# Write the init script
RUN printf '%s' '{init_escaped}' > /sbin/init && chmod +x /sbin/init
# Ensure required directories
RUN mkdir -p /dev /proc /sys /tmp /run /home/user
"#,
        init_escaped = init_script.replace('\'', "'\\''")
    );

    let dockerfile_path = tmp_dir.path().join("Dockerfile");
    tokio::fs::write(&dockerfile_path, &dockerfile).await?;
    tokio::fs::copy(vm_agent_path, tmp_dir.path().join("vm-agent")).await?;

    info!("vm: building rootfs Docker image (packages: {})", pkg_list);

    // docker build
    let status = Command::new("docker")
        .arg("build")
        .arg("-t")
        .arg(tag)
        .arg(tmp_dir.path())
        .status()
        .await
        .context("docker build")?;
    anyhow::ensure!(status.success(), "docker build failed");

    // Create a container, export its filesystem, pipe into mke2fs -d
    // docker export <container> | mke2fs -t ext4 -d - <rootfs_path> <blocks>
    // (1 GiB rootfs — 1048576 blocks of 1K each)
    let rootfs_size_blocks = 1_048_576u64; // 1 GiB

    info!("vm: exporting rootfs to {}", rootfs_path.display());

    let docker_create = Command::new("docker")
        .args(["create", "--name", "claude-vm-rootfs-tmp", tag])
        .output()
        .await
        .context("docker create")?;
    anyhow::ensure!(
        docker_create.status.success(),
        "docker create failed: {}",
        String::from_utf8_lossy(&docker_create.stderr)
    );
    let container_id = String::from_utf8_lossy(&docker_create.stdout)
        .trim()
        .to_string();

    // docker export | mke2fs -d - (pipe)
    let export_result = (|| async {
        let tmp_tar = tmp_dir.path().join("rootfs.tar");
        let export_status = Command::new("docker")
            .args(["export", "-o", tmp_tar.to_str().unwrap(), &container_id])
            .status()
            .await
            .context("docker export")?;
        anyhow::ensure!(export_status.success(), "docker export failed");

        // Unpack tar into a temp dir
        let rootfs_dir = tmp_dir.path().join("rootfs");
        tokio::fs::create_dir_all(&rootfs_dir).await?;
        let unpack = Command::new("tar")
            .args(["-xf", tmp_tar.to_str().unwrap(), "-C", rootfs_dir.to_str().unwrap()])
            .status()
            .await
            .context("tar -xf")?;
        anyhow::ensure!(unpack.success(), "tar extraction failed");

        // Create ext4 image from directory using mke2fs -d
        let mke2fs = Command::new("mke2fs")
            .arg("-t")
            .arg("ext4")
            .arg("-d")
            .arg(&rootfs_dir)
            .arg(rootfs_path)
            .arg(rootfs_size_blocks.to_string())
            .status()
            .await
            .context("mke2fs")?;
        anyhow::ensure!(mke2fs.success(), "mke2fs -d failed");

        Ok::<(), anyhow::Error>(())
    })()
    .await;

    // Clean up the temp container regardless of result
    let _ = Command::new("docker")
        .args(["rm", "-f", &container_id])
        .status()
        .await;

    export_result?;

    info!("vm: rootfs built at {}", rootfs_path.display());
    Ok(())
}

/// Returns a description of what `build` would do (for display to the user).
pub fn describe(extra_packages: &[String]) -> String {
    let mut pkgs: Vec<&str> = BASE_PACKAGES.to_vec();
    let extra: Vec<&str> = extra_packages.iter().map(|s| s.as_str()).collect();
    pkgs.extend_from_slice(&extra);
    format!(
        "Alpine 3.20 base + packages: {}",
        pkgs.join(", ")
    )
}

// ---------------------------------------------------------------------------
// Init script (written to /sbin/init inside the rootfs)
// ---------------------------------------------------------------------------

const INIT_SCRIPT: &str = r#"#!/bin/sh
set -e

# Mount essential virtual filesystems
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev 2>/dev/null || mdev -s
mount -t tmpfs tmpfs /tmp

# Configure loopback
ip link set lo up 2>/dev/null || true

# Parse vm_mounts= from kernel command line
# Format: vm_mounts=/dev/vdb:/guest/path,/dev/vdc:/other/path
MOUNTS=""
for arg in $(cat /proc/cmdline); do
    case "$arg" in
        vm_mounts=*) MOUNTS="${arg#vm_mounts=}" ;;
    esac
done

# Mount each volume (wait up to 2s for block device to appear)
if [ -n "$MOUNTS" ]; then
    IFS=','
    for entry in $MOUNTS; do
        DEV="${entry%%:*}"
        MNTPT="${entry#*:}"
        mkdir -p "$MNTPT"
        waited=0
        while [ ! -b "$DEV" ] && [ $waited -lt 20 ]; do
            sleep 0.1
            waited=$((waited + 1))
        done
        if [ -b "$DEV" ]; then
            mount -t ext4 -o noatime "$DEV" "$MNTPT" \
                && echo "init: mounted $DEV -> $MNTPT" \
                || echo "init: WARNING: failed to mount $DEV -> $MNTPT"
        else
            echo "init: WARNING: $DEV did not appear, skipping"
        fi
    done
    unset IFS
fi

# Set default HOME
mkdir -p /home/user
export HOME=/home/user
export PATH=/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin

echo "init: starting vm-agent"
exec /usr/local/bin/vm-agent
"#;
