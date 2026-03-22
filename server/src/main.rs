mod client_ws;
mod dashboard_ws;
mod ntfy;
mod persist;
mod protocol;
mod state;

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
    let dashboard_count = {
        let guard = state.dashboards.read().await;
        guard.len()
    };

    HttpResponse::Ok().json(serde_json::json!({
        "status": "ok",
        "client_connected": client_connected,
        "session_count": session_count,
        "dashboard_count": dashboard_count,
    }))
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    // Initialise tracing with RUST_LOG env filter (default: info).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
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

    info!("claude-server starting on {bind_addr}");

    HttpServer::new(move || {
        let state = app_state.clone();

        App::new()
            .app_data(web::Data::new(state))
            // WebSocket endpoints
            .route("/ws/client", web::get().to(client_ws::handler))
            .route("/ws/dashboard", web::get().to(dashboard_ws::handler))
            // Health check
            .route("/health", web::get().to(health))
            // Static files (dashboard UI) — served last so routes above take priority
            .service(
                Files::new("/", "./static")
                    .index_file("index.html")
                    .default_handler(
                        web::get().to(|req: actix_web::HttpRequest, _state: web::Data<Arc<AppState>>| async move {
                            // SPA fallback - serve index.html for any unknown route
                            actix_files::NamedFile::open("./static/index.html")
                                .map(|f| f.into_response(&req))
                                .unwrap_or_else(|_| HttpResponse::NotFound().finish())
                        })
                    )
            )
    })
    .bind(&bind_addr)?
    .run()
    .await
}
