mod client_registry;
mod client_ws;
mod commands;
mod config;
mod idle_watchdog;
mod orchestrator;
mod persistence;
mod task_manager;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::{extract::ws::WebSocketUpgrade, routing::get, Router};
use clap::{Parser, Subcommand};
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;

use claude_events::EventBus;

use client_registry::ClientRegistry;
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
    if !config_path.exists() {
        anyhow::bail!(
            "config file not found at {}\n\
             Create one or pass a path with --config.",
            config_path.display()
        );
    }
    let config = OrchestratorConfig::load(&config_path)?;

    let state_dir = OrchestratorConfig::expand_path(&config.server.state_dir);
    let store = Arc::new(StateStore::new(&state_dir));

    let mut bus = EventBus::new();
    let backend_rx = bus.take_backend_receiver();
    let bus = Arc::new(bus);
    let registry = Arc::new(TaskRegistry::new());
    let client_registry = Arc::new(ClientRegistry::new());

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
            use backend_telegram::backend::TelegramConfig as TgConfig;
            let tg_backend = backend_telegram::TelegramBackend::new(TgConfig {
                bot_token: tg_cfg.bot_token.clone(),
                supergroup_id: tg_cfg.supergroup_id,
                scratchpad_topic_name: tg_cfg.scratchpad_topic_name.clone(),
                allowed_users: tg_cfg.allowed_users.clone(),
                voice_stt_api_key: tg_cfg.voice_stt_api_key.clone(),
                show_thinking: config.display.show_thinking,
                state_dir: state_dir.clone(),
            });
            let orch_rx = bus.subscribe_orchestrator();
            let tx = backend_sender.clone();
            tokio::spawn(async move {
                use backend_traits::MessagingBackend;
                if let Err(e) = tg_backend.run(orch_rx, tx).await {
                    tracing::error!("telegram backend error: {e}");
                }
            });
            info!("telegram backend started");
        }
    }

    // Discord backend (if configured).
    if let Some(ref dc_cfg) = config.backends.discord {
        if dc_cfg.enabled {
            use backend_discord::DiscordConfig;
            let discord_backend = backend_discord::DiscordBackend::new(DiscordConfig {
                bot_token: dc_cfg.bot_token.clone(),
                channel_id: dc_cfg.guild_id, // reuse guild_id as channel_id for now
                guild_id: Some(dc_cfg.guild_id),
                allowed_user_ids: vec![],
                show_thinking: config.display.show_thinking,
            });
            let orch_rx = bus.subscribe_orchestrator();
            let tx = backend_sender.clone();
            tokio::spawn(async move {
                use backend_traits::MessagingBackend;
                if let Err(e) = discord_backend.run(orch_rx, tx).await {
                    tracing::error!("discord backend error: {e}");
                }
            });
            info!("discord backend started");
        }
    }

    // Web backend (if configured).
    if let Some(ref web_cfg) = config.backends.web {
        if web_cfg.enabled {
            let bind = web_cfg.bind.clone().unwrap_or_else(|| "0.0.0.0:8080".to_string());
            let orch_rx = bus.subscribe_orchestrator();
            let tx = backend_sender.clone();
            tokio::spawn(async move {
                use backend_traits::MessagingBackend;
                let backend = backend_web::WebBackend::with_bind(bind);
                if let Err(e) = backend.run(orch_rx, tx).await {
                    tracing::error!("web backend error: {e}");
                }
            });
        }
    }

    // Client-daemon WebSocket server.
    {
        let bind = config.server.client_bind.clone();
        let reg = Arc::clone(&client_registry);
        let task_reg = Arc::clone(&registry);
        let b = Arc::clone(&bus);
        let _token = config.server.client_token.clone();
        tokio::spawn(async move {
            let app = Router::new().route(
                "/ws/client",
                get({
                    let reg = Arc::clone(&reg);
                    let task_reg = Arc::clone(&task_reg);
                    let b = Arc::clone(&b);
                    move |ws: WebSocketUpgrade| {
                        let reg = Arc::clone(&reg);
                        let task_reg = Arc::clone(&task_reg);
                        let b = Arc::clone(&b);
                        async move {
                            ws.on_upgrade(move |socket| {
                                client_ws::handle_client_ws(socket, reg, task_reg, b)
                            })
                        }
                    }
                }),
            );

            let listener = match tokio::net::TcpListener::bind(&bind).await {
                Ok(l) => {
                    info!("client WebSocket server listening on {bind}");
                    l
                }
                Err(e) => {
                    tracing::error!("failed to bind client WebSocket server on {bind}: {e}");
                    return;
                }
            };

            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("client WebSocket server error: {e}");
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
        Arc::clone(&client_registry),
        config,
    ));

    // Handle Ctrl-C / SIGTERM.
    let orch_clone = Arc::clone(&orchestrator);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            info!("shutdown signal received");
            let ids = orch_clone.registry.all_ids();
            for id in ids {
                orch_clone.hibernate_task(&id).await;
            }
            info!("shutdown complete");
            std::process::exit(0);
        }
    });

    orchestrator.run(backend_rx).await;
    Ok(())
}

async fn setup(_config_path: PathBuf) -> Result<()> {
    eprintln!(
        "Authentication is handled by the Claude Code CLI on each client machine.\n\
         Run the following on every machine that will run claude-client:\n\
         \n\
         \x1b[1m  claude login\x1b[0m\n\
         \n\
         Then start the client daemon.  The server requires no authentication setup."
    );
    Ok(())
}
