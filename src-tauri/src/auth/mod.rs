// Authentication module for OAuth providers

use crate::commands::{AuthAccount, OAuthProvider};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

pub mod providers;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    pub token_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthFile {
    pub provider: String,
    pub email: Option<String>,
    pub token: TokenInfo,
    #[serde(default)]
    pub project_id: Option<String>,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub prefix: Option<String>,
}

/// CLIProxyAPI-compatible Gemini auth file format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiAuthFile {
    pub token: Value,
    pub project_id: String,
    pub email: String,
    #[serde(default)]
    pub auto: bool,
    #[serde(default)]
    pub checked: bool,
    #[serde(rename = "type")]
    pub auth_type: String,
}

fn parse_auth_file(content: &str, filename: &str) -> Option<AuthAccount> {
    // Try new format first (AuthFile with provider field)
    if let Ok(auth_file) = serde_json::from_str::<AuthFile>(content) {
        return Some(AuthAccount {
            id: filename.to_string(),
            provider: auth_file.provider,
            email: auth_file.email,
            enabled: auth_file.enabled,
            prefix: auth_file.prefix,
        });
    }

    // Try CLIProxyAPI Gemini format (GeminiAuthFile with nested token object)
    if let Ok(gemini_auth) = serde_json::from_str::<GeminiAuthFile>(content) {
        // Check if token has access_token
        if gemini_auth.token.get("access_token").is_some() {
            return Some(AuthAccount {
                id: filename.to_string(),
                provider: "gemini".to_string(),
                email: Some(gemini_auth.email),
                enabled: true, // GeminiAuthFile doesn't have enabled field, default to true
                prefix: None,
            });
        }
    }

    // Try parsing as generic JSON for legacy formats
    if let Ok(json) = serde_json::from_str::<serde_json::Value>(content) {
        let obj = json.as_object()?;

        // Check for access_token at root level or in nested token object
        let has_access_token = obj.contains_key("access_token") ||
            obj.get("token")
                .and_then(|t| t.as_object())
                .map(|t| t.contains_key("access_token"))
                .unwrap_or(false);

        if !has_access_token {
            return None;
        }

        // Get provider from "type" or "provider" field, or from filename
        let provider = obj.get("type")
            .or_else(|| obj.get("provider"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                // Extract provider from filename like "antigravity-email.json" or "gemini-email.json"
                let parts: Vec<&str> = filename.split(|c| c == '-' || c == '_').collect();
                parts.first().map(|s| s.to_string())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let email = obj.get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let enabled = obj.get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let prefix = obj.get("prefix")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        return Some(AuthAccount {
            id: filename.to_string(),
            provider,
            email,
            enabled,
            prefix,
        });
    }

    None
}

pub async fn list_accounts() -> Result<Vec<AuthAccount>> {
    let auth_dir = crate::config::resolve_auth_dir();
    tracing::info!("Listing accounts from: {:?}", auth_dir);

    if !auth_dir.exists() {
        tracing::warn!("Auth dir does not exist: {:?}", auth_dir);
        return Ok(vec![]);
    }

    let mut accounts = Vec::new();

    let entries = std::fs::read_dir(&auth_dir)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().map(|e| e == "json").unwrap_or(false) {
            let filename = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown");

            // Skip config.yaml and other non-auth files
            if filename == "config" {
                continue;
            }

            if let Ok(content) = std::fs::read_to_string(&path) {
                match parse_auth_file(&content, filename) {
                    Some(account) => {
                        tracing::debug!("Parsed account: {} ({})", filename, account.provider);
                        accounts.push(account);
                    }
                    None => {
                        tracing::warn!("Failed to parse auth file: {}", filename);
                    }
                }
            }
        }
    }

    tracing::info!("Found {} accounts", accounts.len());
    Ok(accounts)
}

pub async fn start_oauth(provider: OAuthProvider, project_id: Option<String>) -> Result<String> {
    match provider {
        OAuthProvider::Google => {
            // Google uses a dedicated callback server on port 8085
            match providers::google::start_oauth_with_callback().await {
                Ok(result) => {
                    let email = result.email.clone().unwrap_or_else(|| "default".to_string());
                    let project_id = project_id.unwrap_or_default().trim().to_string();

                    let token = serde_json::json!({
                        "access_token": result.token_response.access_token,
                        "refresh_token": result.token_response.refresh_token,
                        "token_type": result.token_response.token_type,
                        "expires_in": result.token_response.expires_in,
                        "expiry": result.token_response.expires_in.map(|secs| {
                            (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
                        }),
                        "client_id": providers::google::GOOGLE_CLIENT_ID,
                        "client_secret": providers::google::GOOGLE_CLIENT_SECRET,
                        "scopes": providers::google::SCOPES,
                        "token_uri": "https://oauth2.googleapis.com/token",
                        "universe_domain": "googleapis.com"
                    });

                    let gemini_auth = GeminiAuthFile {
                        token,
                        project_id,
                        email: email.clone(),
                        auto: true,
                        checked: false,
                        auth_type: "gemini".to_string(),
                    };

                    let auth_dir = crate::config::resolve_auth_dir();
                    let path = auth_dir.join(format!("gemini-{}-all.json", email));

                    let content = serde_json::to_string_pretty(&gemini_auth)?;
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, content)?;

                    tracing::info!("Saved Gemini auth file to {:?}", path);
                    Ok("OAuth completed successfully".to_string())
                }
                Err(e) => Err(e),
            }
        }
        OAuthProvider::Anthropic => providers::anthropic::start_oauth().await,
        OAuthProvider::OpenAI => {
            // OpenAI uses a special flow with its own callback server on port 1455
            match providers::openai::start_oauth_with_callback().await {
                Ok(result) => {
                    // Save the auth file
                    let expires_at = result.token_response.expires_in.map(|secs| {
                        chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
                    });

                    let auth_file = AuthFile {
                        provider: "codex".to_string(),
                        email: result.email.clone(),
                        token: TokenInfo {
                            access_token: result.token_response.access_token,
                            refresh_token: result.token_response.refresh_token,
                            expires_at,
                            token_type: result.token_response.token_type,
                        },
                        project_id: None,
                        enabled: true,
                        prefix: None,
                    };

                    let identifier = result.email.as_deref().unwrap_or("default");
                    let path = get_auth_file_path("codex", identifier);
                    save_auth_file(&auth_file, &path)?;

                    tracing::info!("Saved Codex auth file to {:?}", path);
                    Ok("OAuth completed successfully".to_string())
                }
                Err(e) => Err(e),
            }
        }
        OAuthProvider::Qwen => providers::qwen::start_oauth().await,
        OAuthProvider::IFlow => providers::iflow::start_oauth().await,
        OAuthProvider::Antigravity => providers::antigravity::start_oauth().await,
    }
}

pub fn set_gemini_project_id(account_id: &str, project_id: &str) -> Result<()> {
    let account_id = account_id.trim();
    let project_id = project_id.trim();
    if account_id.is_empty() {
        return Err(anyhow::anyhow!("account_id is required"));
    }
    if project_id.is_empty() {
        return Err(anyhow::anyhow!("project_id is required"));
    }
    if !account_id.starts_with("gemini-") {
        return Err(anyhow::anyhow!("not a gemini auth file"));
    }

    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));
    if !path.exists() {
        return Err(anyhow::anyhow!("auth file not found: {:?}", path));
    }

    let content = std::fs::read_to_string(&path)?;
    let mut json: serde_json::Value = serde_json::from_str(&content)?;
    if !json.is_object() {
        return Err(anyhow::anyhow!("invalid auth file format"));
    }
    json["project_id"] = serde_json::Value::String(project_id.to_string());

    let updated = serde_json::to_string_pretty(&json)?;
    std::fs::write(&path, updated)?;
    Ok(())
}

pub fn get_auth_file_path(provider: &str, identifier: &str) -> PathBuf {
    let auth_dir = crate::config::resolve_auth_dir();
    auth_dir.join(format!("{}_{}.json", provider, identifier))
}

pub fn save_auth_file(auth_file: &AuthFile, path: &PathBuf) -> Result<()> {
    let content = serde_json::to_string_pretty(auth_file)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, content)?;
    Ok(())
}

pub fn delete_account(account_id: &str) -> Result<()> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if path.exists() {
        std::fs::remove_file(&path)?;
        tracing::info!("Deleted account file: {:?}", path);
        Ok(())
    } else {
        Err(anyhow::anyhow!("Account file not found: {}", account_id))
    }
}

pub fn set_account_enabled(account_id: &str, enabled: bool) -> Result<()> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if !path.exists() {
        return Err(anyhow::anyhow!("Account file not found: {}", account_id));
    }

    let content = std::fs::read_to_string(&path)?;
    let mut json: serde_json::Value = serde_json::from_str(&content)?;

    json["enabled"] = serde_json::json!(enabled);
    json["disabled"] = serde_json::json!(!enabled);

    let content = serde_json::to_string_pretty(&json)?;
    std::fs::write(&path, content)?;
    tracing::info!("Set account {} enabled={}", account_id, enabled);
    Ok(())
}
