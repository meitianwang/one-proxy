// Configuration module for CLI Proxy API

use anyhow::Result;
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

static CONFIG: OnceCell<RwLock<AppConfig>> = OnceCell::new();
static CONFIG_PATH: OnceCell<PathBuf> = OnceCell::new();

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct AppConfig {
    #[serde(default)]
    pub host: String,

    #[serde(default = "default_port")]
    pub port: u16,

    #[serde(default)]
    pub tls: TlsConfig,

    #[serde(default)]
    pub remote_management: RemoteManagementConfig,

    #[serde(default = "default_auth_dir")]
    pub auth_dir: String,

    #[serde(default)]
    pub api_keys: Vec<String>,

    #[serde(default)]
    pub debug: bool,

    #[serde(default)]
    pub proxy_url: String,

    #[serde(default = "default_request_retry")]
    pub request_retry: u32,

    #[serde(default = "default_max_retry_interval")]
    pub max_retry_interval: u32,

    #[serde(default)]
    pub quota_exceeded: QuotaExceededConfig,

    #[serde(default)]
    pub routing: RoutingConfig,

    #[serde(default)]
    pub gemini_api_key: Vec<ApiKeyEntry>,

    #[serde(default)]
    pub claude_api_key: Vec<ApiKeyEntry>,

    #[serde(default)]
    pub codex_api_key: Vec<ApiKeyEntry>,

    #[serde(default)]
    pub openai_compatibility: Vec<OpenAICompatEntry>,

    #[serde(default)]
    pub claude_code_compatibility: Vec<ClaudeCodeCompatEntry>,

    #[serde(default = "default_quota_refresh_interval")]
    pub quota_refresh_interval: u32,

    #[serde(default)]
    pub model_routing: ModelRoutingConfig,
}

fn default_port() -> u16 {
    8417
}

fn default_auth_dir() -> String {
    "~/.cli-proxy-api".to_string()
}

fn default_request_retry() -> u32 {
    3
}

fn default_max_retry_interval() -> u32 {
    30
}

fn default_quota_refresh_interval() -> u32 {
    5
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct TlsConfig {
    #[serde(default)]
    pub enable: bool,
    #[serde(default)]
    pub cert: String,
    #[serde(default)]
    pub key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RemoteManagementConfig {
    #[serde(default)]
    pub allow_remote: bool,
    #[serde(default)]
    pub secret_key: String,
    #[serde(default)]
    pub disable_control_panel: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct QuotaExceededConfig {
    #[serde(default = "default_true")]
    pub switch_project: bool,
    #[serde(default = "default_true")]
    pub switch_preview_model: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct RoutingConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,
}

fn default_strategy() -> String {
    "round-robin".to_string()
}

/// Model routing configuration for aggregation mode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ModelRoutingConfig {
    /// Routing mode: "provider" (default, require provider prefix) or "model" (aggregate by model name)
    #[serde(default = "default_routing_mode")]
    pub mode: String,

    /// Provider priority order for model aggregation mode
    /// Higher priority providers are tried first
    #[serde(default = "default_provider_priorities")]
    pub provider_priorities: Vec<ProviderPriority>,
}

impl Default for ModelRoutingConfig {
    fn default() -> Self {
        Self {
            mode: default_routing_mode(),
            provider_priorities: default_provider_priorities(),
        }
    }
}

/// Provider priority configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct ProviderPriority {
    /// Provider name: "kiro", "antigravity", "gemini", "codex", "claude"
    pub provider: String,
    /// Priority weight (higher = tried first)
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Whether this provider is enabled for aggregation
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_routing_mode() -> String {
    "provider".to_string()
}

fn default_priority() -> u32 {
    100
}

fn default_provider_priorities() -> Vec<ProviderPriority> {
    vec![
        ProviderPriority {
            provider: "kiro".to_string(),
            priority: 100,
            enabled: true,
        },
        ProviderPriority {
            provider: "antigravity".to_string(),
            priority: 90,
            enabled: true,
        },
        ProviderPriority {
            provider: "gemini".to_string(),
            priority: 80,
            enabled: true,
        },
        ProviderPriority {
            provider: "codex".to_string(),
            priority: 70,
            enabled: true,
        },
        ProviderPriority {
            provider: "claude".to_string(),
            priority: 60,
            enabled: true,
        },
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ApiKeyEntry {
    pub api_key: String,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub proxy_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct OpenAICompatEntry {
    pub name: String,
    #[serde(default)]
    pub prefix: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub api_key_entries: Vec<ApiKeyEntry>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub struct ClaudeCodeCompatEntry {
    pub name: String,
    #[serde(default)]
    pub prefix: Option<String>,
    pub base_url: String,
    #[serde(default)]
    pub api_key_entries: Vec<ApiKeyEntry>,
    #[serde(default)]
    pub models: Vec<String>,
}

pub async fn init_config(app: &AppHandle) -> Result<()> {
    let config_dir = app
        .path()
        .app_config_dir()
        .map_err(|e| anyhow::anyhow!("Failed to get config dir: {}", e))?;

    std::fs::create_dir_all(&config_dir)?;

    let config_path = config_dir.join("config.yaml");
    CONFIG_PATH.set(config_path.clone()).ok();

    let config = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        serde_yaml::from_str(&content)?
    } else {
        let default_config = AppConfig::default();
        let content = serde_yaml::to_string(&default_config)?;
        std::fs::write(&config_path, content)?;
        default_config
    };

    CONFIG.set(RwLock::new(config)).ok();

    tracing::info!("Config initialized from {:?}", config_path);
    Ok(())
}

pub fn get_config() -> Option<AppConfig> {
    CONFIG.get().map(|c| c.read().clone())
}

pub fn update_config(config: AppConfig) -> Result<()> {
    if let Some(lock) = CONFIG.get() {
        *lock.write() = config.clone();
    }

    if let Some(path) = CONFIG_PATH.get() {
        let content = serde_yaml::to_string(&config)?;
        std::fs::write(path, content)?;
    }

    Ok(())
}

pub fn get_config_path() -> Option<PathBuf> {
    CONFIG_PATH.get().cloned()
}

pub fn resolve_auth_dir() -> PathBuf {
    let auth_dir = get_config()
        .map(|c| c.auth_dir)
        .unwrap_or_else(default_auth_dir);

    let path = if auth_dir.starts_with("~") {
        let trimmed = auth_dir.trim_start_matches("~/").trim_start_matches('~');
        if let Some(home) = dirs::home_dir() {
            return home.join(trimmed);
        }
        tracing::warn!("Home dir not found; falling back to app config dir for auth storage.");
        PathBuf::from(trimmed)
    } else {
        PathBuf::from(&auth_dir)
    };

    if path.is_absolute() {
        return path;
    }

    if let Some(base) = get_config_path().and_then(|p| p.parent().map(|p| p.to_path_buf())) {
        return base.join(&path);
    }

    if let Some(data_dir) = dirs::data_dir() {
        return data_dir.join("cli-proxy-api").join(&path);
    }

    path
}
