mod cleanup;
mod client_ws;
mod dashboard_api;
mod ntfy;
mod persist;
mod protocol;
mod state;
mod telegram;

use std::sync::Arc;

use actix_files::Files;
use actix_web::{web, App, HttpResponse, HttpServer};
use tracing::info;
use tracing_subscriber::EnvFilter;

use state::AppState;

// ---------------------------------------------------------------------------
// Health check endpoint
// ---------------------------------------------------------------------------

async fn health(state: web::Data<Arc<AppState>>) -> HttpResponse {
    let client_connected = {
        let guard = state.client.read().await;
        guard.is_some()
    };
    let session_count = {
        let guard = state.sessions.read().await;
        guard.len()
    };
    let sse_subscribers = state.sse_tx.receiver_count();

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "client_connected": client_connected,
        "session_count": session_count,
        "sse_subscribers": sse_subscribers,
    }))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();

    let host = std::env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8080);
    let data_dir = std::env::var("DATA_DIR").unwrap_or_else(|_| "./data".to_string());

    let bind_addr = format!("{host}:{port}");

    let app_state = AppState::new();

    info!("claude-server: data_dir={data_dir}");
    app_state.load_from_disk().await;

    // Background session cleanup
    {
        let cleanup_state = app_state.clone();
        tokio::spawn(async move {
            cleanup::run(cleanup_state).await;
        });
    }

    // Start Telegram bot if token is configured
    if let Ok(token) = std::env::var("TELEGRAM_BOT_TOKEN") {
        let tg_state = app_state.clone();
        tokio::spawn(async move {
            telegram::start(tg_state, token).await;
        });
    }

    info!("claude-server starting on {bind_addr}");

    HttpServer::new(move || {
        let state = app_state.clone();

        App::new()
            .app_data(web::Data::new(state))
            // Increase WebSocket payload limit to 128 MB so large Claude events
            // (file reads, big outputs) don't drop the client connection.
            .app_data(web::PayloadConfig::default().limit(128 * 1024 * 1024))
            // Client daemon WebSocket
            .route("/ws/client", web::get().to(client_ws::handler))
            // Dashboard REST + SSE API
            .route("/api/events", web::get().to(dashboard_api::sse_events))
            .route("/api/status", web::get().to(dashboard_api::get_status))
            .route("/api/sessions", web::get().to(dashboard_api::list_sessions))
            .route(
                "/api/sessions",
                web::post().to(dashboard_api::create_session),
            )
            .route(
                "/api/sessions/{id}/history",
                web::get().to(dashboard_api::get_history),
            )
            .route(
                "/api/sessions/{id}/input",
                web::post().to(dashboard_api::send_input),
            )
            .route(
                "/api/sessions/{id}",
                web::delete().to(dashboard_api::kill_session),
            )
            // Health check
            .route("/health", web::get().to(health))
            // Static files (dashboard UI)
            .service(
                Files::new("/", "./static")
                    .index_file("index.html")
                    .default_handler(web::get().to(
                        |req: actix_web::HttpRequest, _state: web::Data<Arc<AppState>>| async move {
                            actix_files::NamedFile::open("./static/index.html")
                                .map(|f| f.into_response(&req))
                                .unwrap_or_else(|_| HttpResponse::NotFound().finish())
                        },
                    )),
            )
    })
    .bind(&bind_addr)?
    .run()
    .await
}
