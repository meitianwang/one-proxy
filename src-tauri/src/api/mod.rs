// HTTP API Server module

use anyhow::Result;
use axum::{
    body::{Body, Bytes},
    http::{header, Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post, put},
    Router,
};
use futures::StreamExt;
use http_body_util::BodyExt;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use serde_json::Value;
use tokio::sync::oneshot;
use tower_http::cors::{Any, CorsLayer};

mod handlers;
pub mod antigravity;
pub mod kiro;
mod schema_cleaner;
pub mod claude;
pub mod codex;
pub mod gemini;
mod mime_types;
pub mod management;
pub mod streaming;
pub mod model_router;
pub mod mappers;
pub mod common;
pub mod signature_cache;
pub mod config;

static SERVER_HANDLE: OnceCell<RwLock<Option<oneshot::Sender<()>>>> = OnceCell::new();

#[derive(Clone)]
pub struct AppState {
    pub app_handle: tauri::AppHandle,
}

/// Determine protocol from request path
fn protocol_from_path(path: &str) -> Option<String> {
    if path.starts_with("/v1/chat/completions") || path.starts_with("/v1/completions") || path.starts_with("/v1/models") {
        Some("openai".to_string())
    } else if path.starts_with("/v1/messages") {
        Some("anthropic".to_string())
    } else if path.starts_with("/v1beta/models") || path.starts_with("/gemini/v1beta/models") {
        Some("gemini".to_string())
    } else {
        None
    }
}


/// Internal header name for passing account_id from handlers to logging middleware
/// This header will be stripped before sending response to client
pub const X_ONEPROXY_ACCOUNT_ID: &str = "x-oneproxy-account-id";

/// Internal header name for passing provider from handlers to logging middleware
/// This header will be stripped before sending response to client
pub const X_ONEPROXY_PROVIDER: &str = "x-oneproxy-provider";

/// Internal header name for passing actual model from handlers to logging middleware
/// This header will be stripped before sending response to client
pub const X_ONEPROXY_MODEL: &str = "x-oneproxy-model";


/// Extract model name from request body JSON
fn extract_model_from_body(body: &[u8]) -> Option<String> {
    let json: serde_json::Value = serde_json::from_slice(body).ok()?;
    json.get("model").and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Normalize model name by removing provider prefix (e.g., "antigravity/claude-3.5" -> "claude-3.5")
/// This ensures consistent model names in logs regardless of how the request was made
fn normalize_model_name(model: &str) -> String {
    // Known provider prefixes that should be stripped
    const PREFIXES: &[&str] = &[
        "antigravity/",
        "gemini/",
        "claude/",
        "codex/",
        "kiro/",
        "kimi/",
        "glm/",
        "deepseek/",
    ];
    
    let lower = model.to_lowercase();
    for prefix in PREFIXES {
        if lower.starts_with(prefix) {
            return model[prefix.len()..].to_string();
        }
    }
    model.to_string()
}

/// Extract model name from Gemini URL path
/// e.g., "/gemini/v1beta/models/gemini-3-flash-preview:generateContent" -> "gemini-3-flash-preview"
fn extract_model_from_gemini_path(path: &str) -> Option<String> {
    // Match patterns like /gemini/v1beta/models/{model}:action or /v1beta/models/{model}:action
    let models_prefix = if path.contains("/gemini/v1beta/models/") {
        "/gemini/v1beta/models/"
    } else if path.contains("/v1beta/models/") {
        "/v1beta/models/"
    } else {
        return None;
    };
    
    let after_models = path.split(models_prefix).nth(1)?;
    // Remove the action suffix like ":generateContent" or ":streamGenerateContent"
    let model = after_models.split(':').next()?;
    if model.is_empty() {
        None
    } else {
        Some(model.to_string())
    }
}


fn should_verbose_log() -> bool {
    if let Some(config) = crate::config::get_config() {
        if config.debug {
            return true;
        }
    }
    std::env::var("CLIPROXY_VERBOSE_LOGS")
        .map(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
        .unwrap_or(false)
}

fn is_sensitive_key(key: &str) -> bool {
    matches!(
        key.trim().to_lowercase().as_str(),
        "authorization"
            | "api_key"
            | "apikey"
            | "access_token"
            | "refresh_token"
            | "client_secret"
            | "token"
            | "bearer"
            | "anthropic_api_key"
            | "openai_api_key"
    )
}

fn redact_json_value(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, val) in map.iter_mut() {
                if is_sensitive_key(key) {
                    *val = Value::String("***".to_string());
                } else {
                    redact_json_value(val);
                }
            }
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                redact_json_value(item);
            }
        }
        _ => {}
    }
}

fn format_body_for_log(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "".to_string();
    }
    if let Ok(mut json) = serde_json::from_slice::<Value>(bytes) {
        redact_json_value(&mut json);
        return json.to_string();
    }
    String::from_utf8_lossy(bytes).to_string()
}

fn log_request_body(method: &str, path: &str, bytes: &[u8]) {
    let body_text = format_body_for_log(bytes);
    if body_text.is_empty() {
        tracing::info!("REQ {} {}", method, path);
    } else {
        tracing::info!("REQ {} {} {}", method, path, body_text);
    }
}

fn log_response_body(method: &str, path: &str, status: u16, bytes: &[u8]) {
    let body_text = format_body_for_log(bytes);
    if body_text.is_empty() {
        tracing::info!("RESP {} {} {}", status, method, path);
    } else {
        tracing::info!("RESP {} {} {} {}", status, method, path, body_text);
    }
}

fn log_stream_chunk(method: &str, path: &str, status: u16, bytes: &Bytes) {
    if bytes.is_empty() {
        return;
    }
    let text = String::from_utf8_lossy(bytes);
    tracing::info!("RESP-STREAM {} {} {} {}", status, method, path, text);
}

async fn log_response_if_needed(
    method: &str,
    path: &str,
    response: Response,
    verbose: bool,
) -> Response {
    if !verbose {
        return response;
    }

    let (parts, body) = response.into_parts();
    let status = parts.status.as_u16();
    let content_type = parts
        .headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if content_type.starts_with("text/event-stream") {
        let method = method.to_string();
        let path = path.to_string();
        let stream = body.into_data_stream().map(move |chunk| {
            if let Ok(ref bytes) = chunk {
                log_stream_chunk(&method, &path, status, bytes);
            }
            chunk
        });
        let new_body = Body::from_stream(stream);
        return Response::from_parts(parts, new_body);
    }

    let bytes = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(_) => Bytes::new(),
    };
    log_response_body(method, path, status, &bytes);
    Response::from_parts(parts, Body::from(bytes))
}

/// Request logging middleware
async fn logging_middleware(request: Request<Body>, next: Next) -> Response {
    let start = std::time::Instant::now();
    let method = request.method().to_string();
    let path = request.uri().path().to_string();
    let verbose = should_verbose_log();

    // Skip logging for model list requests early
    if path == "/v1/models" || (path.starts_with("/v1beta/models") && method == "GET") {
        let response = next.run(request).await;
        return log_response_if_needed(&method, &path, response, verbose).await;
    }

    // Extract model from request body for POST requests
    if method == "POST" {
        // Buffer the body to extract model
        let (parts, body) = request.into_parts();
        let bytes = match axum::body::to_bytes(body, 10 * 1024 * 1024).await {
            Ok(b) => b,
            Err(_) => {
                // If we can't read the body, just continue without model info
                let request = Request::from_parts(parts, Body::empty());
                let response = next.run(request).await;
                return log_response_if_needed(&method, &path, response, verbose).await;
            }
        };

        let model = extract_model_from_body(&bytes)
            .or_else(|| extract_model_from_gemini_path(&path));

        if verbose {
            log_request_body(&method, &path, &bytes);
        }

        // Reconstruct the request with the buffered body
        let request = Request::from_parts(parts, Body::from(bytes.to_vec()));
        let mut response = next.run(request).await;
        
        // Extract and remove internal account_id header
        let account_id = response.headers()
            .get(X_ONEPROXY_ACCOUNT_ID)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        response.headers_mut().remove(X_ONEPROXY_ACCOUNT_ID);
        
        // Extract and remove internal provider header
        let provider = response.headers()
            .get(X_ONEPROXY_PROVIDER)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        response.headers_mut().remove(X_ONEPROXY_PROVIDER);
        
        // Extract and remove internal model header (prefer this over request body model)
        let handler_model = response.headers()
            .get(X_ONEPROXY_MODEL)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string());
        response.headers_mut().remove(X_ONEPROXY_MODEL);
        
        // Use handler-provided model if available, otherwise fall back to request body
        let final_model = handler_model.or(model);
        // Normalize model name (remove provider prefix) for consistent logging
        let normalized_model = final_model.map(|m| normalize_model_name(&m));
        
        let response = log_response_if_needed(&method, &path, response, verbose).await;

        let protocol = protocol_from_path(&path);
        let duration_ms = start.elapsed().as_millis() as i64;
        let status = response.status().as_u16() as i32;
        
        let error_message = if status >= 400 {
            Some(format!("HTTP {}", status))
        } else {
            None
        };
        
        let _ = crate::db::save_request_log(
            status,
            &method,
            normalized_model.as_deref(),
            protocol.as_deref(),
            provider.as_deref(),
            account_id.as_deref(),
            &path,
            0,
            0,
            duration_ms,
            error_message.as_deref(),
        );
        
        return response;
    }

    if verbose {
        log_request_body(&method, &path, &[]);
    }
    let mut response = next.run(request).await;
    
    // Extract and remove internal account_id header
    let account_id = response.headers()
        .get(X_ONEPROXY_ACCOUNT_ID)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    response.headers_mut().remove(X_ONEPROXY_ACCOUNT_ID);
    
    // Extract and remove internal provider header
    let provider = response.headers()
        .get(X_ONEPROXY_PROVIDER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    response.headers_mut().remove(X_ONEPROXY_PROVIDER);
    
    let response = log_response_if_needed(&method, &path, response, verbose).await;

    let protocol = protocol_from_path(&path);
    let duration_ms = start.elapsed().as_millis() as i64;
    let status = response.status().as_u16() as i32;

    let error_message = if status >= 400 {
        Some(format!("HTTP {}", status))
    } else {
        None
    };

    let _ = crate::db::save_request_log(
        status,
        &method,
        None,
        protocol.as_deref(),
        provider.as_deref(),
        account_id.as_deref(),
        &path,
        0,
        0,
        duration_ms,
        error_message.as_deref(),
    );

    response
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
        // Gemini protocol routes (both with and without /gemini prefix)
        .route("/v1beta/models", get(handlers::gemini_models))
        .route("/v1beta/models/*action", post(handlers::gemini_handler))
        .route("/v1beta/models/*action", get(handlers::gemini_get_handler))
        .route("/gemini/v1beta/models", get(handlers::gemini_models))
        .route("/gemini/v1beta/models/*action", post(handlers::gemini_handler))
        .route("/gemini/v1beta/models/*action", get(handlers::gemini_get_handler))
        .layer(middleware::from_fn(auth_middleware))
        .layer(middleware::from_fn(logging_middleware));

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
