//! Host-side network helpers for Firecracker VMs.
//!
//! Instead of root-owned TAP devices and iptables, we use two standard
//! Linux tools that work without elevated privileges:
//!
//! - `unshare --net`      — gives Firecracker a private network namespace
//! - `slirp4netns`        — provides internet via user-space NAT inside that
//!                          namespace; no CAP_NET_ADMIN required
//!
//! # Required tools
//!
//! Both `unshare` (util-linux) and `slirp4netns` must be installed.
//! `unshare` is almost always present; `slirp4netns` is packaged as
//! `slirp4netns` on Fedora/Debian/Ubuntu/Arch.

use anyhow::{Context, Result};

/// Check that the tools needed for rootless VM networking are installed.
pub fn check_tools() -> Result<()> {
    which::which("unshare").context(
        "unshare not found — install util-linux (needed for rootless VM networking)",
    )?;
    which::which("slirp4netns").context(
        "slirp4netns not found — install slirp4netns (needed for rootless VM networking)",
    )?;
    Ok(())
}
