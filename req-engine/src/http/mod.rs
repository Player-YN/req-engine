//! HTTP API (`req-engine serve`) — axum + tower-http CORS.

mod auth;
mod dto;
mod error;
mod routes;

#[cfg(test)]
mod tests;

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use axum::Router;
use axum::http::{HeaderValue, Method};
use rusqlite::Connection;
use tower_http::cors::{AllowOrigin, CorsLayer};

pub use routes::router;

/// Shared application state for handlers.
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Mutex<Connection>>,
    pub home: PathBuf,
}

impl AppState {
    pub fn new(conn: Connection, home: PathBuf) -> Self {
        Self {
            db: Arc::new(Mutex::new(conn)),
            home,
        }
    }
}

fn cors_layer() -> CorsLayer {
    // Localhost static HTML + file:// (Origin: null)
    let origins = [
        "http://127.0.0.1:5500",
        "http://localhost:5500",
        "http://127.0.0.1:3000",
        "http://localhost:3000",
        "http://127.0.0.1:5173",
        "http://localhost:5173",
        "http://127.0.0.1:8080",
        "http://localhost:8080",
        "null",
    ]
    .into_iter()
    .filter_map(|s| s.parse::<HeaderValue>().ok())
    .collect::<Vec<_>>();

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(origins))
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::OPTIONS,
            Method::PUT,
            Method::DELETE,
            Method::PATCH,
        ])
        .allow_headers(tower_http::cors::Any)
}

/// Build the full router with CORS (localhost origins for static HTML).
pub fn app(state: AppState) -> Router {
    router(state).layer(cors_layer())
}

/// API + optional static UI (`web/` directory). `/v1/*` routes take precedence.
pub fn app_with_static(state: AppState, web_dir: Option<std::path::PathBuf>) -> Router {
    use tower_http::services::ServeDir;

    let api = app(state);
    if let Some(dir) = web_dir.filter(|d| d.is_dir()) {
        println!("serving UI from {}", dir.display());
        api.fallback_service(ServeDir::new(dir).append_index_html_on_directories(true))
    } else {
        api
    }
}

/// Bind and serve until the process exits.
pub async fn serve(
    conn: Connection,
    host: &str,
    port: u16,
    home: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    serve_with_static(conn, host, port, None, home).await
}

/// Bind API (+ optional static UI) until the process exits.
pub async fn serve_with_static(
    conn: Connection,
    host: &str,
    port: u16,
    web_dir: Option<std::path::PathBuf>,
    home: PathBuf,
) -> Result<(), Box<dyn std::error::Error>> {
    let _ = crate::services::ensure_all_project_pair_codes(&conn, &home);
    let state = AppState::new(conn, home);
    let app = app_with_static(state, web_dir);
    let addr: SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(addr).await?;
    println!("req-engine listening on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
