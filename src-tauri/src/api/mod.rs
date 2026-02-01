// HTTP API Server module

use anyhow::Result;
use axum::{
    body::Body,
    http::{header, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

mod handlers;
pub mod antigravity;
mod schema_cleaner;
pub mod claude;
pub mod codex;
pub mod gemini;
mod mime_types;
pub mod management;
pub mod streaming;

static SERVER_HANDLE: OnceCell<RwLock<Option<oneshot::Sender<()>>>> = OnceCell::new();

#[derive(Clone)]
pub struct AppState {
    pub app_handle: tauri::AppHandle,
}

/// API Key authentication middleware
async fn auth_middleware(request: Request<Body>, next: Next) -> Response {
    let config = crate::config::get_config().unwrap_or_default();

    // If no API keys configured, allow all requests
    if config.api_keys.is_empty() {
        return next.run(request).await;
    }

    // Extract Authorization header
    let auth_header = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok());

    let is_valid = match auth_header {
        Some(auth) => {
            // Support both "Bearer <key>" and raw key
            let key = auth.strip_prefix("Bearer ").unwrap_or(auth);
            config.api_keys.contains(&key.to_string())
        }
        None => false,
    };

    if is_valid {
        next.run(request).await
    } else {
        (
            StatusCode::UNAUTHORIZED,
            [("Content-Type", "application/json")],
            r#"{"error":{"message":"Invalid API key","type":"invalid_request_error","code":"invalid_api_key"}}"#,
        )
            .into_response()
    }
}

/// Kill any process using the specified port
fn kill_process_on_port(port: u16) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
        {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid in pids.lines() {
                if let Ok(pid_num) = pid.trim().parse::<i32>() {
                    tracing::info!("Killing process {} on port {}", pid_num, port);
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &pid_num.to_string()])
                        .output();
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fuser")
            .args(["-k", &format!("{}/tcp", port)])
            .output()
        {
            tracing::info!("Killed processes on port {}: {:?}", port, output);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("cmd")
            .args(["/c", &format!("for /f \"tokens=5\" %a in ('netstat -aon ^| findstr :{} ^| findstr LISTENING') do taskkill /F /PID %a", port)])
            .output()
        {
            tracing::info!("Killed processes on port {}: {:?}", port, output);
        }
    }

    // Give the OS a moment to release the port
    std::thread::sleep(std::time::Duration::from_millis(100));
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

    // Routes that require API key authentication
    let protected_routes = Router::new()
        .route("/v1/models", get(handlers::openai_models))
        .route("/v1/chat/completions", post(handlers::chat_completions))
        .route("/v1/completions", post(handlers::completions))
        .route("/v1/messages", post(handlers::claude_messages))
        .route("/v1/messages/count_tokens", post(handlers::claude_count_tokens))
        .route("/v1beta/models", get(handlers::gemini_models))
        .route("/v1beta/models/*action", post(handlers::gemini_handler))
        .route("/v1beta/models/*action", get(handlers::gemini_get_handler))
        .layer(middleware::from_fn(auth_middleware));

    // Routes that don't require authentication
    let public_routes = Router::new()
        .route("/", get(handlers::root))
        // OAuth callbacks
        .route("/oauth2callback", get(handlers::google_callback))
        .route("/google/callback", get(handlers::google_callback))
        .route("/anthropic/callback", get(handlers::anthropic_callback))
        .route("/codex/callback", get(handlers::codex_callback))
        .route("/antigravity/callback", get(handlers::antigravity_callback))
        // Management API (internal use)
        .route("/management/auth-files", get(management::list_auth_files))
        .route("/management/auth-files", delete(management::delete_auth_file))
        .route("/management/auth-files/status", patch(management::patch_auth_status))
        .route("/management/auth-summary", get(management::get_auth_summary))
        .route("/management/accounts", get(management::list_accounts))
        .route("/management/config", get(management::get_config))
        .route("/management/config", put(management::update_config))
        .route("/management/status", get(management::get_server_status));

    let app = Router::new()
        .merge(protected_routes)
        .merge(public_routes)
        .layer(cors)
        .with_state(state);

    // Try to bind, if port is in use, kill the process and retry
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            tracing::warn!("Port {} is in use, killing existing process...", config.port);
            kill_process_on_port(config.port);
            tokio::net::TcpListener::bind(&addr).await?
        }
        Err(e) => return Err(e.into()),
    };

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
