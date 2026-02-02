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
pub async fn save_api_key_account(
    provider: String,
    api_key: String,
    label: Option<String>,
) -> Result<AuthAccount, String> {
    crate::auth::save_api_key_account(&provider, &api_key, label.as_deref())
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

#[tauri::command]
pub async fn export_all_accounts() -> Result<String, String> {
    crate::auth::export_all_accounts().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_accounts(json_content: String) -> Result<i32, String> {
    crate::auth::import_accounts(&json_content).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn export_accounts_to_file(file_path: String) -> Result<(), String> {
    crate::auth::export_accounts_to_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn import_accounts_from_file(file_path: String) -> Result<i32, String> {
    crate::auth::import_accounts_from_file(&file_path).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_cached_quotas() -> Result<HashMap<String, crate::db::CachedQuota>, String> {
    crate::db::get_all_quota_cache().map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsData {
    pub quota_refresh_interval: u32,
}

#[tauri::command]
pub async fn get_settings() -> Result<SettingsData, String> {
    let config = config::get_config().ok_or_else(|| "Config not initialized".to_string())?;
    Ok(SettingsData {
        quota_refresh_interval: config.quota_refresh_interval,
    })
}

#[tauri::command]
pub async fn save_settings(settings: SettingsData) -> Result<(), String> {
    let mut config = config::get_config().ok_or_else(|| "Config not initialized".to_string())?;
    config.quota_refresh_interval = settings.quota_refresh_interval;
    config::update_config(config).map_err(|e| e.to_string())
}

// ============ Request Logs Commands ============

#[tauri::command]
pub async fn get_request_logs(
    limit: u32,
    offset: u32,
    filter: Option<crate::db::LogFilter>,
) -> Result<Vec<crate::db::RequestLogEntry>, String> {
    crate::db::get_request_logs(limit, offset, filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_request_logs_count(filter: Option<crate::db::LogFilter>) -> Result<i64, String> {
    crate::db::get_request_logs_count(filter).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_request_logs() -> Result<(), String> {
    crate::db::clear_request_logs().map_err(|e| e.to_string())
}

// ============ Claude Code Config Commands ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeConfig {
    pub opus_model: String,
    pub sonnet_model: String,
    pub haiku_model: String,
}

#[tauri::command]
pub async fn get_claude_code_config() -> Result<Option<ClaudeCodeConfig>, String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let settings_path = home.join(".claude").join("settings.json");

    if !settings_path.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&settings_path)
        .map_err(|e| format!("Failed to read settings.json: {}", e))?;

    let json: serde_json::Value = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse settings.json: {}", e))?;

    let env = json.get("env").and_then(|v| v.as_object());

    if let Some(env) = env {
        Ok(Some(ClaudeCodeConfig {
            opus_model: env.get("ANTHROPIC_DEFAULT_OPUS_MODEL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            sonnet_model: env.get("ANTHROPIC_DEFAULT_SONNET_MODEL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
            haiku_model: env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string(),
        }))
    } else {
        Ok(None)
    }
}

#[tauri::command]
pub async fn save_claude_code_config(claude_config: ClaudeCodeConfig) -> Result<(), String> {
    let home = dirs::home_dir().ok_or("Cannot find home directory")?;
    let claude_dir = home.join(".claude");
    let settings_path = claude_dir.join("settings.json");

    // Ensure .claude directory exists
    std::fs::create_dir_all(&claude_dir)
        .map_err(|e| format!("Failed to create .claude directory: {}", e))?;

    // Get current config for base URL and API key
    let app_config = config::get_config().unwrap_or_default();
    let base_url = format!("http://127.0.0.1:{}", app_config.port);
    let api_key = app_config.api_keys.first()
        .cloned()
        .unwrap_or_else(|| "sk-oneproxy".to_string());

    // Read existing settings to preserve other fields
    let mut settings: serde_json::Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).unwrap_or_default();
        serde_json::from_str(&content).unwrap_or(serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    // Update env section
    let env = serde_json::json!({
        "ANTHROPIC_AUTH_TOKEN": api_key,
        "ANTHROPIC_BASE_URL": base_url,
        "ANTHROPIC_DEFAULT_OPUS_MODEL": claude_config.opus_model,
        "ANTHROPIC_DEFAULT_SONNET_MODEL": claude_config.sonnet_model,
        "ANTHROPIC_DEFAULT_HAIKU_MODEL": claude_config.haiku_model,
        "API_TIMEOUT_MS": "3000000",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1"
    });

    settings["env"] = env;

    // Write back
    let content = serde_json::to_string_pretty(&settings)
        .map_err(|e| format!("Failed to serialize settings: {}", e))?;

    std::fs::write(&settings_path, content)
        .map_err(|e| format!("Failed to write settings.json: {}", e))?;

    tracing::info!("Claude Code config saved to {:?}", settings_path);
    Ok(())
}

// ============ Custom Provider Commands ============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProviderEntry {
    pub name: String,
    pub prefix: Option<String>,
    pub base_url: String,
    pub api_keys: Vec<String>,
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CustomProvidersData {
    pub openai_compatibility: Vec<CustomProviderEntry>,
    pub claude_code_compatibility: Vec<CustomProviderEntry>,
}

#[tauri::command]
pub async fn get_custom_providers() -> Result<CustomProvidersData, String> {
    let config = config::get_config().ok_or_else(|| "Config not initialized".to_string())?;

    let openai_compat = config.openai_compatibility.iter().map(|e| CustomProviderEntry {
        name: e.name.clone(),
        prefix: e.prefix.clone(),
        base_url: e.base_url.clone(),
        api_keys: e.api_key_entries.iter().map(|k| k.api_key.clone()).collect(),
        models: e.models.clone(),
    }).collect();

    let claude_compat = config.claude_code_compatibility.iter().map(|e| CustomProviderEntry {
        name: e.name.clone(),
        prefix: e.prefix.clone(),
        base_url: e.base_url.clone(),
        api_keys: e.api_key_entries.iter().map(|k| k.api_key.clone()).collect(),
        models: e.models.clone(),
    }).collect();

    Ok(CustomProvidersData {
        openai_compatibility: openai_compat,
        claude_code_compatibility: claude_compat,
    })
}

#[tauri::command]
pub async fn save_custom_providers(data: CustomProvidersData) -> Result<(), String> {
    // Reserved provider names that cannot be used
    const RESERVED_NAMES: &[&str] = &[
        "gemini", "codex", "openai", "claude", "antigravity", "kimi", "glm", "kiro"
    ];

    // Collect all prefixes for duplicate checking
    let mut all_prefixes: Vec<String> = Vec::new();

    // Validate OpenAI compatibility providers
    for entry in &data.openai_compatibility {
        let prefix = entry.prefix.as_ref().unwrap_or(&entry.name).to_lowercase();

        // Check reserved names
        if RESERVED_NAMES.contains(&prefix.as_str()) {
            return Err(format!(
                "供应商前缀 '{}' 是保留名称，不能使用。保留名称包括: {}",
                prefix,
                RESERVED_NAMES.join(", ")
            ));
        }

        // Check duplicates
        if all_prefixes.contains(&prefix) {
            return Err(format!(
                "供应商前缀 '{}' 已存在，不能重复添加",
                prefix
            ));
        }
        all_prefixes.push(prefix);
    }

    // Validate Claude Code compatibility providers
    for entry in &data.claude_code_compatibility {
        let prefix = entry.prefix.as_ref().unwrap_or(&entry.name).to_lowercase();

        // Check reserved names
        if RESERVED_NAMES.contains(&prefix.as_str()) {
            return Err(format!(
                "供应商前缀 '{}' 是保留名称，不能使用。保留名称包括: {}",
                prefix,
                RESERVED_NAMES.join(", ")
            ));
        }

        // Check duplicates
        if all_prefixes.contains(&prefix) {
            return Err(format!(
                "供应商前缀 '{}' 已存在，不能重复添加",
                prefix
            ));
        }
        all_prefixes.push(prefix);
    }

    let mut config = config::get_config().ok_or_else(|| "Config not initialized".to_string())?;

    config.openai_compatibility = data.openai_compatibility.iter().map(|e| {
        config::OpenAICompatEntry {
            name: e.name.clone(),
            prefix: e.prefix.clone(),
            base_url: e.base_url.clone(),
            api_key_entries: e.api_keys.iter().map(|k| config::ApiKeyEntry {
                api_key: k.clone(),
                prefix: None,
                base_url: None,
                proxy_url: None,
            }).collect(),
            models: e.models.clone(),
        }
    }).collect();

    config.claude_code_compatibility = data.claude_code_compatibility.iter().map(|e| {
        config::ClaudeCodeCompatEntry {
            name: e.name.clone(),
            prefix: e.prefix.clone(),
            base_url: e.base_url.clone(),
            api_key_entries: e.api_keys.iter().map(|k| config::ApiKeyEntry {
                api_key: k.clone(),
                prefix: None,
                base_url: None,
                proxy_url: None,
            }).collect(),
            models: e.models.clone(),
        }
    }).collect();

    config::update_config(config).map_err(|e| e.to_string())
}
