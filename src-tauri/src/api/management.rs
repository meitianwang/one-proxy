// Management API handlers

use axum::{
    extract::State,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;

use super::AppState;
use crate::auth::{self, AuthFile};
use crate::config;

/// Parse auth file content (supports both new and legacy formats)
fn parse_auth_info(content: &str) -> Option<(String, Option<String>, bool)> {
    // Try new format first
    if let Ok(auth_file) = serde_json::from_str::<AuthFile>(content) {
        return Some((auth_file.provider, auth_file.email, auth_file.enabled));
    }

    // Try parsing as generic JSON for legacy formats
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        let obj = json.as_object()?;

        // Must have access_token to be valid
        if !obj.contains_key("access_token") {
            return None;
        }

        let provider = obj.get("type")
            .or_else(|| obj.get("provider"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let email = obj.get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let enabled = obj.get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        return Some((provider, email, enabled));
    }

    None
}

/// List all auth files
pub async fn list_auth_files(State(_state): State<AppState>) -> impl IntoResponse {
    let auth_dir = config::resolve_auth_dir();

    if !auth_dir.exists() {
        return Json(json!({ "files": [] }));
    }

    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".json") && !name.starts_with(".") {
                    if let Ok(metadata) = entry.metadata() {
                        let mut file_info = json!({
                            "name": name,
                            "size": metadata.len(),
                        });

                        if let Ok(modified) = metadata.modified() {
                            if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                                file_info["modtime"] = json!(duration.as_secs());
                            }
                        }

                        // Read file to get provider and email
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            if let Some((provider, email, enabled)) = parse_auth_info(&content) {
                                file_info["provider"] = json!(provider);
                                file_info["type"] = json!(provider);
                                file_info["enabled"] = json!(enabled);
                                if let Some(e) = email {
                                    file_info["email"] = json!(e);
                                }
                            }
                        }

                        files.push(file_info);
                    }
                }
            }
        }
    }

    // Sort by name
    files.sort_by(|a, b| {
        let name_a = a["name"].as_str().unwrap_or("");
        let name_b = b["name"].as_str().unwrap_or("");
        name_a.to_lowercase().cmp(&name_b.to_lowercase())
    });

    Json(json!({ "files": files }))
}

#[derive(Debug, Deserialize)]
pub struct DeleteAuthFileQuery {
    pub name: Option<String>,
    pub all: Option<String>,
}

/// Delete auth file(s)
pub async fn delete_auth_file(
    State(_state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<DeleteAuthFileQuery>,
) -> impl IntoResponse {
    let auth_dir = config::resolve_auth_dir();

    // Delete all files
    if let Some(all) = &params.all {
        if all == "true" || all == "1" || all == "*" {
            let mut deleted = 0;
            if let Ok(entries) = std::fs::read_dir(&auth_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.ends_with(".json") && !name.starts_with(".") {
                            if std::fs::remove_file(&path).is_ok() {
                                deleted += 1;
                            }
                        }
                    }
                }
            }
            return Json(json!({ "status": "ok", "deleted": deleted }));
        }
    }

    // Delete single file
    let name = match &params.name {
        Some(n) => n,
        None => return Json(json!({ "error": "name is required" })),
    };

    if name.contains(std::path::MAIN_SEPARATOR) || name.contains('/') || name.contains('\\') {
        return Json(json!({ "error": "invalid name" }));
    }

    let path = auth_dir.join(name);

    match std::fs::remove_file(&path) {
        Ok(_) => Json(json!({ "status": "ok" })),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            Json(json!({ "error": "file not found" }))
        }
        Err(e) => Json(json!({ "error": format!("failed to delete: {}", e) })),
    }
}

#[derive(Debug, Deserialize)]
pub struct PatchAuthStatusRequest {
    pub name: String,
    pub enabled: bool,
}

/// Enable/disable an auth file
pub async fn patch_auth_status(
    State(_state): State<AppState>,
    Json(request): Json<PatchAuthStatusRequest>,
) -> impl IntoResponse {
    let auth_dir = config::resolve_auth_dir();
    let name = request.name.trim();

    if name.is_empty() {
        return Json(json!({ "error": "name is required" }));
    }

    if name.contains(std::path::MAIN_SEPARATOR) || name.contains('/') || name.contains('\\') {
        return Json(json!({ "error": "invalid name" }));
    }

    let path = auth_dir.join(name);

    // Read existing file
    let content = match std::fs::read_to_string(&path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Json(json!({ "error": "file not found" }));
        }
        Err(e) => {
            return Json(json!({ "error": format!("failed to read: {}", e) }));
        }
    };

    let mut auth_file: AuthFile = match serde_json::from_str(&content) {
        Ok(f) => f,
        Err(e) => {
            return Json(json!({ "error": format!("invalid auth file: {}", e) }));
        }
    };

    // Update enabled status
    auth_file.enabled = request.enabled;

    // Save back
    match auth::save_auth_file(&auth_file, &path) {
        Ok(_) => Json(json!({ "status": "ok", "enabled": request.enabled })),
        Err(e) => Json(json!({ "error": format!("failed to save: {}", e) })),
    }
}

/// Get current configuration
pub async fn get_config(State(_state): State<AppState>) -> impl IntoResponse {
    match config::get_config() {
        Some(cfg) => Json(json!(cfg)),
        None => Json(json!({})),
    }
}

#[derive(Debug, Deserialize)]
pub struct UpdateConfigRequest {
    pub host: Option<String>,
    pub port: Option<u16>,
    pub auth_dir: Option<String>,
    pub api_keys: Option<Vec<String>>,
}

/// Update configuration
pub async fn update_config(
    State(_state): State<AppState>,
    Json(request): Json<UpdateConfigRequest>,
) -> impl IntoResponse {
    let mut cfg = config::get_config().unwrap_or_default();

    if let Some(host) = request.host {
        cfg.host = host;
    }
    if let Some(port) = request.port {
        cfg.port = port;
    }
    if let Some(auth_dir) = request.auth_dir {
        cfg.auth_dir = auth_dir;
    }
    if let Some(api_keys) = request.api_keys {
        cfg.api_keys = api_keys;
    }

    match config::update_config(cfg) {
        Ok(_) => Json(json!({ "status": "ok" })),
        Err(e) => Json(json!({ "error": format!("failed to save config: {}", e) })),
    }
}

/// Get server status
pub async fn get_server_status(State(_state): State<AppState>) -> impl IntoResponse {
    let running = crate::api::is_server_running();
    let config = config::get_config();

    let mut status = json!({
        "running": running,
    });

    if let Some(cfg) = config {
        status["host"] = json!(cfg.host);
        status["port"] = json!(cfg.port);
        if running {
            let addr = if cfg.host.is_empty() {
                format!("0.0.0.0:{}", cfg.port)
            } else {
                format!("{}:{}", cfg.host, cfg.port)
            };
            status["address"] = json!(addr);
        }
    }

    Json(status)
}

/// List accounts (same as Tauri command)
pub async fn list_accounts(State(_state): State<AppState>) -> impl IntoResponse {
    match crate::auth::list_accounts().await {
        Ok(accounts) => Json(json!(accounts)),
        Err(e) => Json(json!({ "error": format!("{}", e) })),
    }
}

/// Get auth file count by provider
pub async fn get_auth_summary(State(_state): State<AppState>) -> impl IntoResponse {
    let auth_dir = config::resolve_auth_dir();

    let mut summary: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
    let mut total = 0;
    let mut enabled = 0;

    if let Ok(entries) = std::fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.ends_with(".json") && !name.starts_with(".") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Some((provider, _email, is_enabled)) = parse_auth_info(&content) {
                            total += 1;
                            if is_enabled {
                                enabled += 1;
                            }
                            *summary.entry(provider).or_insert(0) += 1;
                        }
                    }
                }
            }
        }
    }

    Json(json!({
        "total": total,
        "enabled": enabled,
        "by_provider": summary,
    }))
}
