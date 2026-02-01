// HTTP API Server module

use anyhow::Result;
use axum::{
    http::Method,
    routing::{delete, get, patch, post, put},
    Router,
};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

mod handlers;
pub mod claude;
pub mod gemini;
pub mod management;
pub mod streaming;

static SERVER_HANDLE: OnceCell<RwLock<Option<oneshot::Sender<()>>>> = OnceCell::new();

#[derive(Clone)]
pub struct AppState {
    pub app_handle: tauri::AppHandle,
}

pub async fn start_server(app_handle: tauri::AppHandle) -> Result<()> {
    let config = crate::config::get_config().unwrap_or_default();

    let host = if config.host.is_empty() {
        "0.0.0.0"
    } else {
        &config.host
    };
    let addr = format!("{}:{}", host, config.port);

    let state = AppState { app_handle };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(handlers::root))
        .route("/v1/models", get(handlers::openai_models))
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/completions", post(handlers::completions))
        .route("/v1/messages", post(handlers::claude_messages))
        .route("/v1beta/models", get(handlers::gemini_models))
        .route("/v1beta/models/*action", post(handlers::gemini_handler))
        // OAuth callbacks
        .route("/oauth2callback", get(handlers::google_callback))
        .route("/google/callback", get(handlers::google_callback))
        .route("/anthropic/callback", get(handlers::anthropic_callback))
        .route("/codex/callback", get(handlers::codex_callback))
        .route("/antigravity/callback", get(handlers::antigravity_callback))
        // Management API
        .route("/management/auth-files", get(management::list_auth_files))
        .route("/management/auth-files", delete(management::delete_auth_file))
        .route("/management/auth-files/status", patch(management::patch_auth_status))
        .route("/management/auth-summary", get(management::get_auth_summary))
        .route("/management/accounts", get(management::list_accounts))
        .route("/management/config", get(management::get_config))
        .route("/management/config", put(management::update_config))
        .route("/management/status", get(management::get_server_status))
        .layer(cors)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("API server listening on {}", addr);

    let (tx, rx) = oneshot::channel::<()>();

    SERVER_HANDLE
        .get_or_init(|| RwLock::new(None))
        .write()
        .replace(tx);

    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            rx.await.ok();
        })
        .await?;

    Ok(())
}

pub async fn stop_server() -> Result<()> {
    if let Some(lock) = SERVER_HANDLE.get() {
        if let Some(tx) = lock.write().take() {
            let _ = tx.send(());
            tracing::info!("API server stopped");
        }
    }
    Ok(())
}

pub fn is_server_running() -> bool {
    SERVER_HANDLE
        .get()
        .map(|lock| lock.read().is_some())
        .unwrap_or(false)
}
