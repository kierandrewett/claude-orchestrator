//! Host-side network setup for Firecracker VMs.
//!
//! Each VM session gets a unique /30 subnet derived deterministically from the
//! session ID. A TAP device is created, configured with a host-side IP, and
//! iptables NAT rules are installed so the guest can reach the internet.
//! Everything is cleaned up (best-effort) after the VM exits.
//!
//! # Required privileges
//!
//! `ip tuntap add` and `iptables` require either `root` or `CAP_NET_ADMIN`.
//! If the client process lacks these privileges, `setup()` will return an
//! error and the session will be aborted with a clear diagnostic.

use anyhow::{Context, Result};
use tokio::process::Command;
use tracing::{info, warn};

/// All network parameters for one VM session.
pub struct NetworkSpec {
    /// TAP device name on the host (≤15 chars).
    pub tap_name: String,
    /// Host-side IP address (gateway from the guest's perspective).
    pub gateway_ip: String,
    /// Guest-side IP address.
    pub guest_ip: String,
    /// Prefix length (always 30).
    pub prefix_len: u8,
    /// Guest MAC address.
    pub guest_mac: String,
    /// Outbound WAN interface detected at setup time (needed for cleanup).
    pub wan_iface: String,
}

impl NetworkSpec {
    /// Derive all network parameters from a session ID deterministically.
    fn derive(session_id: &str) -> (String, String, String, String) {
        // Collect hex digits from the UUID (strip hyphens).
        let hex: String = session_id.chars().filter(|c| c.is_ascii_hexdigit()).collect();

        // TAP name: "vmtap" (5) + last 10 hex chars = 15 chars exactly.
        let tap_suffix = if hex.len() >= 10 {
            hex[hex.len() - 10..].to_string()
        } else {
            format!("{:0>10}", hex)
        };
        let tap_name = format!("vmtap{tap_suffix}");

        // /30 subnet index from last 4 hex chars → 0..4095 (12-bit).
        let last4 = &hex[hex.len().saturating_sub(4)..];
        let idx = u16::from_str_radix(last4, 16).unwrap_or(0) as u32 & 0x0FFF;
        // Place subnets in 172.26.0.0/13 — 4096 possible /30 blocks.
        //   block_ip = 172.26.0.0 + idx*4
        let base = idx * 4;
        let a = (base >> 8) & 0xFF;
        let b = base & 0xFF;
        let gateway_ip = format!("172.26.{a}.{}", b + 1);
        let guest_ip = format!("172.26.{a}.{}", b + 2);

        // MAC: AA:FC:<4 bytes from last 8 hex chars>
        let mac_hex = &hex[hex.len().saturating_sub(8)..];
        let mac = if mac_hex.len() >= 8 {
            format!(
                "AA:FC:{:02X}:{:02X}:{:02X}:{:02X}",
                u8::from_str_radix(&mac_hex[0..2], 16).unwrap_or(0),
                u8::from_str_radix(&mac_hex[2..4], 16).unwrap_or(0),
                u8::from_str_radix(&mac_hex[4..6], 16).unwrap_or(0),
                u8::from_str_radix(&mac_hex[6..8], 16).unwrap_or(0),
            )
        } else {
            "AA:FC:00:00:00:01".to_string()
        };

        (tap_name, gateway_ip, guest_ip, mac)
    }
}

/// Create the TAP device, assign an IP, install iptables NAT rules.
///
/// Returns a `NetworkSpec` that must be passed to `teardown()` after the VM exits.
pub async fn setup(session_id: &str) -> Result<NetworkSpec> {
    let (tap_name, gateway_ip, guest_ip, guest_mac) = NetworkSpec::derive(session_id);

    let host_cidr = format!("{gateway_ip}/30");
    let guest_cidr = format!("{guest_ip}/30");

    // Detect the default outbound interface (needed for NAT rules).
    let wan_iface = detect_wan_iface()
        .await
        .context("detect outbound network interface")?;

    info!(
        "vm: network: tap={tap_name} host={gateway_ip} guest={guest_ip} wan={wan_iface}"
    );

    // Create TAP device.
    run_cmd("ip", &["tuntap", "add", "dev", &tap_name, "mode", "tap"])
        .await
        .with_context(|| format!("create TAP device {tap_name}"))?;

    // Assign host-side IP.
    run_cmd("ip", &["addr", "add", &host_cidr, "dev", &tap_name])
        .await
        .with_context(|| format!("assign {host_cidr} to {tap_name}"))?;

    // Bring TAP up.
    run_cmd("ip", &["link", "set", &tap_name, "up"])
        .await
        .with_context(|| format!("bring up {tap_name}"))?;

    // Enable IP forwarding.
    run_cmd("sysctl", &["-w", "net.ipv4.ip_forward=1"])
        .await
        .context("enable IPv4 forwarding")?;

    // NAT masquerade: packets from the /30 leave via the WAN interface.
    run_cmd(
        "iptables",
        &[
            "-t", "nat", "-A", "POSTROUTING",
            "-s", &guest_cidr,
            "-o", &wan_iface,
            "-j", "MASQUERADE",
        ],
    )
    .await
    .context("iptables MASQUERADE rule")?;

    // Allow forwarding from the TAP to the WAN.
    run_cmd(
        "iptables",
        &["-A", "FORWARD", "-i", &tap_name, "-o", &wan_iface, "-j", "ACCEPT"],
    )
    .await
    .context("iptables FORWARD outbound rule")?;

    // Allow established/related return traffic back to the guest.
    run_cmd(
        "iptables",
        &[
            "-A", "FORWARD",
            "-i", &wan_iface, "-o", &tap_name,
            "-m", "state", "--state", "RELATED,ESTABLISHED",
            "-j", "ACCEPT",
        ],
    )
    .await
    .context("iptables FORWARD inbound rule")?;

    Ok(NetworkSpec {
        tap_name,
        gateway_ip,
        guest_ip,
        prefix_len: 30,
        guest_mac,
        wan_iface,
    })
}

/// Remove iptables rules and delete the TAP device. Best-effort; logs warnings.
pub async fn teardown(spec: &NetworkSpec) {
    let guest_cidr = format!("{}/30", spec.guest_ip);

    let rules: &[(&str, &[&str])] = &[
        (
            "iptables MASQUERADE",
            &[
                "-t", "nat", "-D", "POSTROUTING",
                "-s", &guest_cidr,
                "-o", &spec.wan_iface,
                "-j", "MASQUERADE",
            ],
        ),
        (
            "iptables FORWARD outbound",
            &["-D", "FORWARD", "-i", &spec.tap_name, "-o", &spec.wan_iface, "-j", "ACCEPT"],
        ),
        (
            "iptables FORWARD inbound",
            &[
                "-D", "FORWARD",
                "-i", &spec.wan_iface, "-o", &spec.tap_name,
                "-m", "state", "--state", "RELATED,ESTABLISHED",
                "-j", "ACCEPT",
            ],
        ),
    ];

    for (label, args) in rules {
        if let Err(e) = run_cmd("iptables", args).await {
            warn!("vm: network teardown: {label} failed: {e:#}");
        }
    }

    if let Err(e) = run_cmd("ip", &["link", "delete", &spec.tap_name]).await {
        warn!("vm: network teardown: delete {} failed: {e:#}", spec.tap_name);
    }
}

/// Detect the default outbound network interface by parsing `ip route get 1.1.1.1`.
async fn detect_wan_iface() -> Result<String> {
    let output = Command::new("ip")
        .args(["route", "get", "1.1.1.1"])
        .output()
        .await
        .context("spawn ip route get")?;

    let text = String::from_utf8_lossy(&output.stdout);
    // Output looks like: "1.1.1.1 via 192.168.1.1 dev eth0 src ..."
    let mut tokens = text.split_whitespace();
    while let Some(tok) = tokens.next() {
        if tok == "dev" {
            if let Some(iface) = tokens.next() {
                return Ok(iface.to_string());
            }
        }
    }
    anyhow::bail!("could not parse outbound interface from: {text}")
}

async fn run_cmd(prog: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(prog)
        .args(args)
        .status()
        .await
        .with_context(|| format!("spawn {prog}"))?;
    anyhow::ensure!(
        status.success(),
        "{prog} {} failed (exit {:?})",
        args.join(" "),
        status.code()
    );
    Ok(())
}
