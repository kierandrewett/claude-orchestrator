//! Volume lifecycle management.
//!
//! Each `VolumeMount` is backed by a persistent ext4 image stored in
//! `<data_dir>/volumes/<name>.ext4`.  Before a VM session the host directory
//! is rsynced *into* the image; after the session it is rsynced *back*.
//!
//! Mounting the image without root privileges uses `fuse2fs` (part of
//! `e2fsprogs-fuse`).  If `fuse2fs` is absent the operation fails with a
//! clear actionable error.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tokio::process::Command;
use tracing::{info, warn};

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Ensure an ext4 image exists at `image_path` and is populated from
/// `host_path` (using rsync with the given exclude patterns).
///
/// Creates the image with `dd` + `mkfs.ext4` on first use.
pub async fn prepare(
    image_path: &Path,
    host_path: &Path,
    size_gb: u32,
    excludes: &[String],
) -> Result<()> {
    // Create the image if it doesn't exist yet.
    if !image_path.exists() {
        if let Some(dir) = image_path.parent() {
            tokio::fs::create_dir_all(dir).await?;
        }
        create_image(image_path, size_gb)
            .await
            .with_context(|| format!("create volume image {}", image_path.display()))?;
        info!("vm: created volume image {}", image_path.display());
    }

    // Rsync host → image.
    with_fuse_mount(image_path, |mnt| async move {
        rsync_to(host_path, &mnt, excludes).await
    })
    .await
    .with_context(|| {
        format!(
            "populate volume {} from {}",
            image_path.display(),
            host_path.display()
        )
    })?;

    info!(
        "vm: volume {} ready (host: {})",
        image_path.display(),
        host_path.display()
    );
    Ok(())
}

/// Rsync the contents of the image back to the host directory.
/// Called after the VM session exits for writable volumes.
pub async fn sync_back(image_path: &Path, host_path: &Path) -> Result<()> {
    with_fuse_mount(image_path, |mnt| async move {
        rsync_from(&mnt, host_path).await
    })
    .await
    .with_context(|| {
        format!(
            "sync-back volume {} → {}",
            image_path.display(),
            host_path.display()
        )
    })?;

    info!(
        "vm: synced {} back to {}",
        image_path.display(),
        host_path.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Image creation
// ---------------------------------------------------------------------------

async fn create_image(image_path: &Path, size_gb: u32) -> Result<()> {
    // Sparse allocation so we don't actually consume `size_gb` GiB up front.
    let status = Command::new("dd")
        .args([
            "if=/dev/zero",
            &format!("of={}", image_path.display()),
            "bs=1M",
            "count=1",
            &format!("seek={}", (size_gb as u64) * 1024 - 1),
        ])
        .status()
        .await
        .context("dd")?;
    anyhow::ensure!(status.success(), "dd failed");

    let status = Command::new("mkfs.ext4")
        .arg("-F")
        .arg(image_path)
        .status()
        .await
        .context("mkfs.ext4")?;
    anyhow::ensure!(status.success(), "mkfs.ext4 failed");

    Ok(())
}

// ---------------------------------------------------------------------------
// fuse2fs mount / unmount
// ---------------------------------------------------------------------------

/// Mount `image_path` with `fuse2fs`, call `f` with the mount point, then
/// unmount — even if `f` returns an error.
async fn with_fuse_mount<F, Fut>(image_path: &Path, f: F) -> Result<()>
where
    F: FnOnce(PathBuf) -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    check_fuse2fs()?;

    // Unique temp mount point.
    let mount_point = std::env::temp_dir().join(format!(
        "claude-vm-{}-{}",
        std::process::id(),
        image_path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    ));
    tokio::fs::create_dir_all(&mount_point).await?;

    // Mount
    let status = Command::new("fuse2fs")
        .arg(image_path)
        .arg(&mount_point)
        .arg("-o")
        .arg("fakeroot,rw")
        .status()
        .await
        .context("fuse2fs mount")?;
    anyhow::ensure!(status.success(), "fuse2fs mount failed");

    // Run the callback
    let result = f(mount_point.clone()).await;

    // Always unmount
    let umount_status = Command::new("fusermount")
        .arg("-u")
        .arg(&mount_point)
        .status()
        .await;
    if let Err(e) = umount_status {
        warn!("vm: fusermount -u failed: {e}");
    }
    let _ = tokio::fs::remove_dir(&mount_point).await;

    result
}

fn check_fuse2fs() -> Result<()> {
    if which::which("fuse2fs").is_err() {
        anyhow::bail!(
            "fuse2fs not found — install e2fsprogs-fuse:\n\
             Ubuntu/Debian: sudo apt install e2fsprogs-fuse\n\
             Fedora:        sudo dnf install fuse2fs\n\
             Alpine:        apk add e2fsprogs-extra"
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// rsync helpers
// ---------------------------------------------------------------------------

async fn rsync_to(src: &Path, dst: &PathBuf, excludes: &[String]) -> Result<()> {
    let mut cmd = Command::new("rsync");
    cmd.arg("-a")
        .arg("--delete")
        .arg("--checksum")
        // Trailing slash on src means "contents of src", not src itself.
        .arg(format!("{}/", src.display()))
        .arg(format!("{}/", dst.display()));

    for ex in excludes {
        cmd.arg(format!("--exclude={ex}"));
    }

    let status = cmd.status().await.context("rsync (host→image)")?;
    anyhow::ensure!(status.success(), "rsync (host→image) failed");
    Ok(())
}

async fn rsync_from(src: &PathBuf, dst: &Path) -> Result<()> {
    let status = Command::new("rsync")
        .arg("-a")
        .arg("--checksum")
        .arg(format!("{}/", src.display()))
        .arg(format!("{}/", dst.display()))
        .status()
        .await
        .context("rsync (image→host)")?;
    anyhow::ensure!(status.success(), "rsync (image→host) failed");
    Ok(())
}
