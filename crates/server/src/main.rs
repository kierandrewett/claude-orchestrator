mod commands;
mod config;
mod idle_watchdog;
mod orchestrator;
mod persistence;
mod task_manager;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use claude_containers::{AuthManager, ContainerManager};
use claude_events::EventBus;

use config::OrchestratorConfig;
use orchestrator::Orchestrator;
use persistence::StateStore;
use task_manager::TaskRegistry;

#[derive(Parser)]
#[command(name = "claude-orchestrator", about = "Claude Code Orchestrator")]
struct Cli {
    /// Path to the config file.
    #[arg(short, long, default_value = "config/orchestrator.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the orchestrator (default).
    Run,
    /// Interactive first-time setup: build images + authenticate.
    Setup,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let cli = Cli::parse();

    match cli.command.unwrap_or(Commands::Run) {
        Commands::Run => run(cli.config).await,
        Commands::Setup => setup(cli.config).await,
    }
}

async fn run(config_path: PathBuf) -> Result<()> {
    let config = if config_path.exists() {
        OrchestratorConfig::load(&config_path)?
    } else {
        warn!("no config file at {}, using defaults", config_path.display());
        OrchestratorConfig::default()
    };

    let state_dir = OrchestratorConfig::expand_path(&config.server.state_dir);
    let auth_dir = OrchestratorConfig::expand_path(&config.auth.credentials_dir);

    let auth = AuthManager::new(auth_dir);
    if !auth.has_credentials() {
        warn!("no credentials found — run `claude-orchestrator setup` to authenticate");
    }

    let containers = ContainerManager::new(
        &config.docker.socket,
        auth,
        PathBuf::from("docker/profiles"),
    )
    .context("connecting to Docker")?;

    let mut bus = EventBus::new();
    let backend_rx = bus.take_backend_receiver();
    let bus = Arc::new(bus);
    let registry = Arc::new(TaskRegistry::new());
    let store = Arc::new(StateStore::new(&state_dir));
    let containers = Arc::new(containers);

    // Load persisted state.
    match store.load() {
        Ok(state) => info!("loaded {} persisted tasks", state.tasks.len()),
        Err(e) => warn!("failed to load state: {e}"),
    }

    let backend_sender = bus.backend_sender();

    // Stdio backend is always active for development.
    {
        let orch_rx = bus.subscribe_orchestrator();
        let tx = backend_sender.clone();
        tokio::spawn(async move {
            let backend = backend_stdio::StdioBackend;
            use backend_traits::MessagingBackend;
            if let Err(e) = backend.run(orch_rx, tx).await {
                tracing::error!("stdio backend error: {e}");
            }
        });
    }

    // Telegram backend (if configured).
    if let Some(ref tg_cfg) = config.backends.telegram {
        if tg_cfg.enabled {
            info!("telegram backend enabled");
            // backend-telegram::start() would go here
        }
    }

    // Web backend (if configured).
    if config
        .backends
        .web
        .as_ref()
        .map(|w| w.enabled)
        .unwrap_or(false)
    {
        let orch_rx = bus.subscribe_orchestrator();
        let tx = backend_sender.clone();
        tokio::spawn(async move {
            use backend_traits::MessagingBackend;
            let backend = backend_web::WebBackend::new();
            if let Err(e) = backend.run(orch_rx, tx).await {
                tracing::error!("web backend error: {e}");
            }
        });
    }

    // Start idle watchdog.
    {
        let reg = Arc::clone(&registry);
        let b = Arc::clone(&bus);
        let idle_hours = config.docker.idle_timeout_hours;
        tokio::spawn(async move {
            idle_watchdog::run(reg, b, idle_hours).await;
        });
    }

    // Run the orchestrator main loop.
    let orchestrator = Arc::new(Orchestrator::new(
        Arc::clone(&bus),
        Arc::clone(&registry),
        Arc::clone(&containers),
        config,
    ));

    // Handle Ctrl-C / SIGTERM.
    let orch_clone = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("shutdown signal received");
            // Hibernate all running tasks.
            let ids = orch_clone.registry.all_ids();
            for id in ids {
                orch_clone.hibernate_task(&id).await;
            }
        }
    });

    orchestrator.run(backend_rx).await;
    Ok(())
}

async fn setup(config_path: PathBuf) -> Result<()> {
    let config = if config_path.exists() {
        OrchestratorConfig::load(&config_path)?
    } else {
        OrchestratorConfig::default()
    };

    info!("setup: building Docker images...");
    let status = tokio::process::Command::new("bash")
        .args(["scripts/build-images.sh"])
        .status()
        .await
        .context("running build-images.sh")?;

    if !status.success() {
        anyhow::bail!("image build failed");
    }

    info!("setup: running Claude authentication...");
    let auth_dir = OrchestratorConfig::expand_path(&config.auth.credentials_dir);
    let auth = AuthManager::new(auth_dir.clone());
    let containers = ContainerManager::new(
        &config.docker.socket,
        AuthManager::new(auth_dir),
        PathBuf::from("docker/profiles"),
    )?;

    auth.login(&containers.docker, "orchestrator/claude-code:base")
        .await
        .context("authentication flow")?;

    info!("setup: complete!");
    Ok(())
}
