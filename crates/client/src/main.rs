mod connection;
mod protocol;
mod runner;
mod session_runner;
mod tray;

use std::sync::{Arc, Mutex};
use tracing::info;
use tray::TrayState;

fn main() -> anyhow::Result<()> {
    check_prerequisites();

    // When running without a terminal (e.g. systemd/autostart), default to
    // "warn" to keep logs quiet.  An explicit RUST_LOG always takes precedence.
    let log_level = if atty::is(atty::Stream::Stderr) {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string())
    } else {
        std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".to_string())
    };

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::new(&log_level))
        .init();

    // Read required environment variables
    let server_url = std::env::var("SERVER_URL")
        .expect("SERVER_URL environment variable is required (ws:// or wss:// URL)");
    let client_token =
        std::env::var("CLIENT_TOKEN").expect("CLIENT_TOKEN environment variable is required");

    let default_cwd = std::env::var("DEFAULT_CWD").unwrap_or_else(|_| {
        dirs::home_dir()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "/".to_string())
    });

    let client_id = load_or_create_client_id();
    info!("client_id = {client_id}");

    let hostname = std::env::var("HOSTNAME").unwrap_or_else(|_| {
        hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "unknown".to_string())
    });
    info!("hostname = {hostname}");
    info!("server_url = {server_url}");
    info!("default_cwd = {default_cwd}");

    let config = Arc::new(connection::Config {
        server_url,
        client_token,
        client_id,
        hostname,
        default_cwd,
    });

    let dashboard_url =
        std::env::var("DASHBOARD_URL").unwrap_or_else(|_| "http://localhost:8080".to_string());

    // Shared state between the tray UI and the connection background thread
    let tray_state = Arc::new(Mutex::new(TrayState {
        connected: false,
        hostname: None,
        active_sessions: 0,
        dashboard_url: dashboard_url.clone(),
    }));

    // Broadcast channel for coordinated shutdown
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    // Spawn the tokio runtime in a background thread so that the main thread
    // is free for the GTK / tray event loop (GTK requires the main thread).
    let config_clone = Arc::clone(&config);
    let tray_state_bg = Arc::clone(&tray_state);
    let shutdown_rx_bg = shutdown_tx.subscribe();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        rt.block_on(connection::run_forever(
            config_clone,
            tray_state_bg,
            shutdown_rx_bg,
        ));
        info!("background tokio runtime exiting");
    });

    // Build tray on the main thread (required by GTK)
    let mut tray = tray::Tray::new(Arc::clone(&tray_state))?;

    let open_id = tray.open_id();
    let restart_id = tray.restart_id();
    let quit_id = tray.quit_id();

    // Register Ctrl-C / SIGTERM to trigger a clean shutdown
    let shutdown_tx_signal = shutdown_tx.clone();
    ctrlc::set_handler(move || {
        info!("signal received, shutting down");
        let _ = shutdown_tx_signal.send(());
        std::process::exit(0);
    })
    .ok();

    // ── Main / tray event loop ────────────────────────────────────────────────
    loop {
        // Poll for menu events (non-blocking)
        if let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if event.id == quit_id {
                let _ = shutdown_tx.send(());
                break;
            } else if event.id == open_id {
                let url = tray_state.lock().unwrap().dashboard_url.clone();
                let _ = open::that(url);
            } else if event.id == restart_id {
                info!("Restarting orchestrator via systemctl");
                let _ = std::process::Command::new("systemctl")
                    .args(["--user", "restart", "claude-client"])
                    .spawn();
            }
        }

        // Refresh tray labels from shared state
        let snapshot = tray_state.lock().unwrap().clone();
        tray.update(&snapshot);

        // Drain pending GTK events (Linux only)
        #[cfg(target_os = "linux")]
        {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }

        std::thread::sleep(std::time::Duration::from_millis(100));
    }

    info!("daemon exiting");
    Ok(())
}

/// Verify that `claude` is installed and the user is logged in.
/// Prints a clear error message and exits if either check fails.
fn check_prerequisites() {
    // 1. Check the binary is on PATH.
    if which::which("claude").is_err() {
        eprintln!(
            "\x1b[1;31merror:\x1b[0m `claude` not found on PATH.\n\
             \n\
             Install Claude Code first:\n\
             \n\
             \x1b[1m  npm install -g @anthropic-ai/claude-code\x1b[0m\n\
             \n\
             Then log in:\n\
             \n\
             \x1b[1m  claude login\x1b[0m"
        );
        std::process::exit(1);
    }

    // 2. Check the helper binary exists (sibling of this binary, or on PATH).
    let helper_found = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("claude-orchestrator-helper")))
        .map(|p| p.exists())
        .unwrap_or(false)
        || which::which("claude-orchestrator-helper").is_ok();

    if !helper_found {
        eprintln!(
            "\x1b[1;31merror:\x1b[0m `claude-orchestrator-helper` not found.\n\
             \n\
             Build and install it alongside this binary:\n\
             \n\
             \x1b[1m  cargo install --path crates/helper\x1b[0m"
        );
        std::process::exit(1);
    }

    // 3. Check that credentials exist (~/.claude/.credentials.json).
    let creds = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("/"))
        .join(".claude")
        .join(".credentials.json");

    if !creds.exists() {
        eprintln!(
            "\x1b[1;31merror:\x1b[0m Claude credentials not found at {}.\n\
             \n\
             Log in to Claude Code before starting the client daemon:\n\
             \n\
             \x1b[1m  claude login\x1b[0m",
            creds.display()
        );
        std::process::exit(1);
    }
}

/// Reads an existing client-id from disk, or generates and persists a new one.
fn load_or_create_client_id() -> String {
    let dir = dirs::config_dir()
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
            std::path::PathBuf::from(home).join(".config")
        })
        .join("claude-client");

    std::fs::create_dir_all(&dir).ok();

    let path = dir.join("id");

    if let Ok(contents) = std::fs::read_to_string(&path) {
        let trimmed = contents.trim().to_string();
        if !trimmed.is_empty() {
            return trimmed;
        }
    }

    let new_id = uuid::Uuid::new_v4().to_string();

    if let Err(e) = std::fs::write(&path, &new_id) {
        tracing::warn!("could not persist client_id to {}: {e}", path.display());
    } else {
        tracing::info!("persisted new client_id to {}", path.display());
    }

    new_id
}
