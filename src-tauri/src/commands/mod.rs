// Tauri IPC commands

use crate::config::{self, AppConfig};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub running: bool,
    pub port: u16,
    pub host: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthSummary {
    pub total: i32,
    pub enabled: i32,
    pub by_provider: HashMap<String, i32>,
}

#[tauri::command]
pub async fn get_config() -> Result<AppConfig, String> {
    config::get_config().ok_or_else(|| "Config not initialized".to_string())
}

#[tauri::command]
pub async fn save_config(config: AppConfig) -> Result<(), String> {
    config::update_config(config).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_auth_accounts() -> Result<Vec<AuthAccount>, String> {
    crate::auth::list_accounts().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_auth_summary() -> Result<AuthSummary, String> {
    let accounts = crate::auth::list_accounts().await.map_err(|e| e.to_string())?;

    let mut by_provider: HashMap<String, i32> = HashMap::new();
    let mut enabled = 0;

    for account in &accounts {
        *by_provider.entry(account.provider.clone()).or_insert(0) += 1;
        if account.enabled {
            enabled += 1;
        }
    }

    Ok(AuthSummary {
        total: accounts.len() as i32,
        enabled,
        by_provider,
    })
}

#[tauri::command]
pub async fn start_server(app: tauri::AppHandle) -> Result<(), String> {
    crate::api::start_server(app).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn stop_server() -> Result<(), String> {
    crate::api::stop_server().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_server_status() -> Result<ServerStatus, String> {
    let running = crate::api::is_server_running();
    let config = config::get_config().unwrap_or_default();

    Ok(ServerStatus {
        running,
        port: config.port,
        host: if config.host.is_empty() {
            "0.0.0.0".to_string()
        } else {
            config.host
        },
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthAccount {
    pub id: String,
    pub provider: String,
    pub email: Option<String>,
    pub enabled: bool,
    pub prefix: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OAuthProvider {
    Google,
    Anthropic,
    OpenAI,
    Qwen,
    IFlow,
    Antigravity,
    Kiro,
}

#[tauri::command]
pub async fn start_oauth_login(
    provider: String,
    project_id: Option<String>,
) -> Result<String, String> {
    let provider = match provider.to_lowercase().as_str() {
        "google" | "gemini" => OAuthProvider::Google,
        "anthropic" | "claude" => OAuthProvider::Anthropic,
        "openai" | "codex" => OAuthProvider::OpenAI,
        "qwen" => OAuthProvider::Qwen,
        "iflow" => OAuthProvider::IFlow,
        "antigravity" => OAuthProvider::Antigravity,
        "kiro" => OAuthProvider::Kiro,
        _ => return Err(format!("Unknown provider: {}", provider)),
    };

    crate::auth::start_oauth(provider, project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_account(account_id: String) -> Result<(), String> {
    crate::auth::delete_account(&account_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_account_enabled(account_id: String, enabled: bool) -> Result<(), String> {
    crate::auth::set_account_enabled(&account_id, enabled).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_gemini_project_id(account_id: String, project_id: String) -> Result<(), String> {
    crate::auth::set_gemini_project_id(&account_id, &project_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_antigravity_quota(account_id: String) -> Result<crate::auth::providers::antigravity::QuotaData, String> {
    crate::auth::fetch_antigravity_quota(&account_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_codex_quota(account_id: String) -> Result<crate::auth::providers::openai::CodexQuotaData, String> {
    crate::auth::fetch_codex_quota(&account_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_gemini_quota(account_id: String) -> Result<crate::auth::providers::google::GeminiQuotaData, String> {
    crate::auth::fetch_gemini_quota(&account_id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn fetch_kiro_quota(account_id: String) -> Result<crate::auth::providers::kiro::KiroQuotaData, String> {
    crate::auth::fetch_kiro_quota(&account_id).await.map_err(|e| e.to_string())
}
