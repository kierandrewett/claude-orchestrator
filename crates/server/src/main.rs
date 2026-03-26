mod client_registry;
mod client_ws;
mod commands;
mod config;
mod idle_watchdog;
mod mcp;
mod mcp_registry;
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

use claude_events::{EventBus, OrchestratorEvent};

use client_registry::ClientRegistry;
use config::OrchestratorConfig;
use mcp_registry::McpServerRegistry;
use orchestrator::Orchestrator;
use persistence::StateStore;
use task_manager::TaskRegistry;

// ── Session action API ────────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SessionActionRequest {
    action: String,
    #[serde(default)]
    title: Option<String>,
}

#[derive(Clone)]
struct SessionApiState {
    registry: Arc<TaskRegistry>,
    bus: Arc<EventBus>,
}

async fn session_action_handler(
    axum::extract::State(state): axum::extract::State<SessionApiState>,
    axum::extract::Path(session_id): axum::extract::Path<String>,
    axum::Json(req): axum::Json<SessionActionRequest>,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    match req.action.as_str() {
        "rename_conversation" => {
            let title = match req.title {
                Some(t) if !t.is_empty() => t,
                _ => {
                    return (
                        StatusCode::BAD_REQUEST,
                        axum::Json(serde_json::json!({"error": "title required"})),
                    )
                        .into_response()
                }
            };
            match state.registry.find_by_session_id(&session_id) {
                Some(task_id) => {
                    state
                        .bus
                        .emit(OrchestratorEvent::ConversationRenamed { task_id, title });
                    axum::Json(serde_json::json!({"ok": true})).into_response()
                }
                None => (
                    StatusCode::NOT_FOUND,
                    axum::Json(serde_json::json!({"error": "session not found"})),
                )
                    .into_response(),
            }
        }
        _ => (
            StatusCode::BAD_REQUEST,
            axum::Json(serde_json::json!({"error": "unknown action"})),
        )
            .into_response(),
    }
}

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

/// Reverse-proxy a request to the dashboard Node server.
async fn proxy_to_dashboard(
    base_url: String,
    req: axum::extract::Request,
) -> axum::response::Response {
    use axum::response::IntoResponse;

    let path_and_query = req.uri()
        .path_and_query()
        .map(|pq| pq.as_str())
        .unwrap_or("/");
    let url = format!("{}{}", base_url, path_and_query);

    let method = reqwest::Method::from_bytes(req.method().as_str().as_bytes())
        .unwrap_or(reqwest::Method::GET);

    let body_bytes = match axum::body::to_bytes(req.into_body(), 32 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return axum::http::StatusCode::BAD_REQUEST.into_response(),
    };

    let client = reqwest::Client::new();
    match client.request(method, &url)
        .body(body_bytes)
        .send()
        .await
    {
        Ok(resp) => {
            let status = axum::http::StatusCode::from_u16(resp.status().as_u16())
                .unwrap_or(axum::http::StatusCode::INTERNAL_SERVER_ERROR);
            let mut builder = axum::response::Response::builder().status(status);
            for (k, v) in resp.headers() {
                if !matches!(k.as_str(), "connection" | "transfer-encoding") {
                    builder = builder.header(k, v);
                }
            }
            let bytes = resp.bytes().await.unwrap_or_default();
            builder.body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(e) => {
            warn!("dashboard proxy error: {e}");
            axum::http::StatusCode::BAD_GATEWAY.into_response()
        }
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
    std::fs::create_dir_all(&state_dir).ok();
    let store = Arc::new(StateStore::new(&state_dir));
    let mcp_registry = Arc::new(McpServerRegistry::load(&state_dir));
    let db = claude_db::Db::open(&state_dir).expect("failed to open db");

    let mut bus = EventBus::new();
    let backend_rx = bus.take_backend_receiver();
    let bus = Arc::new(bus);
    let registry = Arc::new(TaskRegistry::new());
    let client_registry = Arc::new(ClientRegistry::new());

    // Load persisted state and sync to DB so the scheduler can find all tasks.
    match store.load() {
        Ok(state) => {
            info!("loaded {} persisted tasks", state.tasks.len());
            for pt in state.tasks {
                use crate::persistence::PersistedTaskState;
                if matches!(pt.state, PersistedTaskState::Dead) {
                    continue;
                }
                let mut task = task_manager::Task::new(
                    pt.id.clone(),
                    pt.name.clone(),
                    pt.profile,
                    task_manager::TaskState::Hibernated,
                    pt.kind,
                );
                task.usage = pt.usage;
                task.last_activity = pt.last_activity;
                task.created_at = pt.created_at;
                task.claude_session_id = pt.claude_session_id.clone();
                // Sync into SQLite so the scheduler can query tasks by id.
                db.upsert_task(&claude_db::TaskRow {
                    task_id: pt.id.0.clone(),
                    task_name: pt.name.clone(),
                    session_id: pt.claude_session_id,
                    session_status: "hibernated".to_string(),
                    created_at: task.created_at.to_rfc3339(),
                    last_activity: Some(task.last_activity.to_rfc3339()),
                });
                registry.insert(task);
            }
        }
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
                hidden_tools: tg_cfg.hidden_tools.clone(),
                dashboard_url: config.backends.web
                    .as_ref()
                    .and_then(|w| w.dashboard_url.clone()),
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

    // Spawn Node.js dashboard server alongside the web API.
    // Only set dashboard_proxy_url if we actually find and spawn the server.
    let dashboard_proxy_url: Option<String> = if let Some(ref web_cfg) = config.backends.web {
        if web_cfg.enabled {
            let api_bind = web_cfg.bind.clone().unwrap_or_else(|| "0.0.0.0:8080".to_string());
            let orch_api = format!("http://{}", api_bind.replace("0.0.0.0", "localhost"));
            let dashboard_port = web_cfg.dashboard_bind.as_deref()
                .unwrap_or("0.0.0.0:3001")
                .split(':').last().unwrap_or("3001").to_string();
            let dashboard_token = web_cfg.dashboard_token.clone().unwrap_or_default();
            let config_path_str = config_path.to_string_lossy().to_string();
            let state_dir_str = state_dir.to_string_lossy().to_string();

            let candidates: Vec<std::path::PathBuf> = vec![
                std::path::PathBuf::from("dashboard/dist-server/index.cjs"),
            ];

            if let Some(server_js) = candidates.into_iter().find(|p| p.exists()) {
                let proxy_url = format!("http://localhost:{}", dashboard_port);
                tokio::spawn(async move {
                    let mut cmd = tokio::process::Command::new("node");
                    cmd.arg(&server_js)
                        .env("PORT", &dashboard_port)
                        .env("ORCHESTRATOR_API", &orch_api)
                        .env("DASHBOARD_TOKEN", &dashboard_token)
                        .env("CONFIG_PATH", &config_path_str)
                        .env("STATE_DIR", &state_dir_str);
                    info!("starting dashboard server on :{dashboard_port}");
                    match cmd.spawn() {
                        Ok(mut child) => { let _ = child.wait().await; }
                        Err(e) => tracing::error!("failed to start dashboard server: {e}"),
                    }
                });
                Some(proxy_url)
            } else {
                info!("dashboard/dist-server/index.cjs not found - skipping dashboard spawn (run: cd dashboard && npm run build)");
                None
            }
        } else {
            None
        }
    } else {
        None
    };

    // Client-daemon WebSocket server.
    {
        let bind = config.server.client_bind.clone();
        let reg = Arc::clone(&client_registry);
        let task_reg = Arc::clone(&registry);
        let b = Arc::clone(&bus);
        let mcp_token = config.server.client_token.clone();
        let db_for_mcp = db.clone();
        tokio::spawn(async move {
            let session_api_state = SessionApiState {
                registry: Arc::clone(&task_reg),
                bus: Arc::clone(&b),
            };

            let mcp_state = mcp::McpState {
                registry: Arc::clone(&task_reg),
                bus: Arc::clone(&b),
                connections: std::sync::Arc::new(dashmap::DashMap::new()),
                db: db_for_mcp,
                token: mcp_token,
            };

            let app = Router::new()
            .route(
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
            )
            .merge(
                Router::new()
                    .route("/api/session/:session_id/action", axum::routing::post(session_action_handler))
                    .with_state(session_api_state),
            )
            .merge(
                Router::new()
                    .route("/mcp", get(mcp::mcp_sse_handler).post(mcp::mcp_post_handler))
                    .with_state(mcp_state),
            );

            let app = if let Some(proxy_url) = dashboard_proxy_url {
                info!("proxying dashboard requests to {proxy_url}");
                app.fallback(move |req: axum::extract::Request| {
                    proxy_to_dashboard(proxy_url.clone(), req)
                })
            } else {
                app
            };

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

    // Start the scheduler.
    claude_scheduler::start(db.clone(), Arc::clone(&bus), backend_sender.clone());

    // Run the orchestrator main loop.
    let orchestrator = Arc::new(Orchestrator::new(
        Arc::clone(&bus),
        Arc::clone(&registry),
        Arc::clone(&client_registry),
        config,
        Arc::clone(&store),
        Arc::clone(&mcp_registry),
        db,
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
