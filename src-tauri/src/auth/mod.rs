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
        OAuthProvider::Kiro => providers::kiro::start_oauth().await,
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

/// Fetch quota for an Antigravity account
pub async fn fetch_antigravity_quota(account_id: &str) -> Result<providers::antigravity::QuotaData> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if !path.exists() {
        return Err(anyhow::anyhow!("Account file not found: {}", account_id));
    }

    let content = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    // Check if it's an antigravity account
    let provider = json.get("type")
        .or_else(|| json.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider != "antigravity" {
        return Err(anyhow::anyhow!("Not an antigravity account"));
    }

    // Get access token - try to refresh if needed
    // Check both root level and nested in token object
    let refresh_token = json.get("refresh_token")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("token")
                .and_then(|t| t.get("refresh_token"))
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("No refresh token found"))?;

    // Refresh the token to get a fresh access token
    let token_resp = providers::antigravity::refresh_token(refresh_token).await?;
    let access_token = token_resp.access_token;

    // Get cached project_id and subscription_tier if available
    let cached_project_id = json.get("project_id")
        .and_then(|v| v.as_str());
    let cached_subscription_tier = json.get("subscription_tier")
        .and_then(|v| v.as_str());

    // Fetch quota
    let quota = providers::antigravity::fetch_quota(&access_token, cached_project_id, cached_subscription_tier).await?;

    // Update the auth file with new token and quota info
    let mut updated_json = json.clone();
    updated_json["access_token"] = serde_json::json!(access_token);
    if let Some(new_refresh) = token_resp.refresh_token {
        updated_json["refresh_token"] = serde_json::json!(new_refresh);
    }
    if let Some(expires_in) = token_resp.expires_in {
        updated_json["expires_in"] = serde_json::json!(expires_in);
        let expired = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);
        updated_json["expired"] = serde_json::json!(expired.to_rfc3339());
    }
    if let Some(ref pid) = quota.project_id {
        updated_json["project_id"] = serde_json::json!(pid);
    }
    if let Some(ref tier) = quota.subscription_tier {
        updated_json["subscription_tier"] = serde_json::json!(tier);
    }
    updated_json["quota_last_updated"] = serde_json::json!(quota.last_updated);
    updated_json["quota_is_forbidden"] = serde_json::json!(quota.is_forbidden);

    let updated_content = serde_json::to_string_pretty(&updated_json)?;
    std::fs::write(&path, updated_content)?;

    Ok(quota)
}

/// Fetch quota for a Codex (OpenAI) account
pub async fn fetch_codex_quota(account_id: &str) -> Result<providers::openai::CodexQuotaData> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if !path.exists() {
        return Err(anyhow::anyhow!("Account file not found: {}", account_id));
    }

    let content = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    // Check if it's an openai/codex account
    let provider = json.get("type")
        .or_else(|| json.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider != "openai" && provider != "codex" {
        return Err(anyhow::anyhow!("Not an OpenAI/Codex account"));
    }

    // Get refresh token - check both root level and nested in token object
    let refresh_token = json.get("refresh_token")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("token")
                .and_then(|t| t.get("refresh_token"))
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("No refresh token found"))?;

    // Refresh the token to get a fresh access token
    let token_resp = providers::openai::refresh_token(refresh_token).await?;
    let access_token = token_resp.access_token;

    // Get account_id for the API call (different from our internal account_id)
    let openai_account_id = json.get("account_id")
        .and_then(|v| v.as_str());

    // Fetch quota
    let quota = providers::openai::fetch_codex_quota(&access_token, openai_account_id).await?;

    // Update the auth file with new token
    let mut updated_json = json.clone();
    updated_json["access_token"] = serde_json::json!(access_token);
    if let Some(new_refresh) = token_resp.refresh_token {
        updated_json["refresh_token"] = serde_json::json!(new_refresh);
    }
    if let Some(expires_in) = token_resp.expires_in {
        updated_json["expires_in"] = serde_json::json!(expires_in);
        let expired = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);
        updated_json["expired"] = serde_json::json!(expired.to_rfc3339());
    }
    updated_json["codex_quota_last_updated"] = serde_json::json!(quota.last_updated);
    updated_json["codex_plan_type"] = serde_json::json!(&quota.plan_type);

    let updated_content = serde_json::to_string_pretty(&updated_json)?;
    std::fs::write(&path, updated_content)?;

    Ok(quota)
}

/// Fetch quota for a Gemini account
pub async fn fetch_gemini_quota(account_id: &str) -> Result<providers::google::GeminiQuotaData> {
    let auth_dir = crate::config::resolve_auth_dir();
    let path = auth_dir.join(format!("{}.json", account_id));

    if !path.exists() {
        return Err(anyhow::anyhow!("Account file not found: {}", account_id));
    }

    let content = std::fs::read_to_string(&path)?;
    let json: serde_json::Value = serde_json::from_str(&content)?;

    // Check if it's a gemini/google account
    let provider = json.get("type")
        .or_else(|| json.get("provider"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if provider != "gemini" && provider != "google" {
        return Err(anyhow::anyhow!("Not a Gemini account"));
    }

    // Get refresh token - check both root level and nested in token object
    let refresh_token = json.get("refresh_token")
        .and_then(|v| v.as_str())
        .or_else(|| {
            json.get("token")
                .and_then(|t| t.get("refresh_token"))
                .and_then(|v| v.as_str())
        })
        .ok_or_else(|| anyhow::anyhow!("No refresh token found"))?;

    // Refresh the token to get a fresh access token
    let token_resp = providers::google::refresh_token(refresh_token).await?;
    let access_token = token_resp.access_token;

    // Get project_id if available
    let project_id = json.get("project_id")
        .and_then(|v| v.as_str());

    // Fetch quota
    let quota = providers::google::fetch_gemini_quota(&access_token, project_id).await?;

    // Update the auth file with new token
    let mut updated_json = json.clone();
    // Update token in nested object if it exists
    if updated_json.get("token").is_some() {
        updated_json["token"]["access_token"] = serde_json::json!(access_token);
        if let Some(expires_in) = token_resp.expires_in {
            updated_json["token"]["expires_in"] = serde_json::json!(expires_in);
            let expiry = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);
            updated_json["token"]["expiry"] = serde_json::json!(expiry.to_rfc3339());
        }
    } else {
        updated_json["access_token"] = serde_json::json!(access_token);
        if let Some(expires_in) = token_resp.expires_in {
            updated_json["expires_in"] = serde_json::json!(expires_in);
            let expired = chrono::Utc::now() + chrono::Duration::seconds(expires_in as i64);
            updated_json["expired"] = serde_json::json!(expired.to_rfc3339());
        }
    }
    updated_json["gemini_quota_last_updated"] = serde_json::json!(quota.last_updated);

    let updated_content = serde_json::to_string_pretty(&updated_json)?;
    std::fs::write(&path, updated_content)?;

    Ok(quota)
}

pub async fn fetch_kiro_quota(account_id: &str) -> Result<providers::kiro::KiroQuotaData> {
    providers::kiro::fetch_quota(account_id).await
}

/// Export all accounts to a single JSON string
pub fn export_all_accounts() -> Result<String> {
    let auth_dir = crate::config::resolve_auth_dir();

    if !auth_dir.exists() {
        return Ok("[]".to_string());
    }

    let mut accounts: Vec<serde_json::Value> = Vec::new();

    for entry in std::fs::read_dir(&auth_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.extension().map_or(false, |ext| ext == "json") {
            match std::fs::read_to_string(&path) {
                Ok(content) => {
                    match serde_json::from_str::<serde_json::Value>(&content) {
                        Ok(json) => {
                            accounts.push(json);
                        }
                        Err(e) => {
                            tracing::warn!("Failed to parse {:?}: {}", path, e);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to read {:?}: {}", path, e);
                }
            }
        }
    }

    serde_json::to_string_pretty(&accounts).map_err(|e| anyhow::anyhow!("Failed to serialize: {}", e))
}

/// Import accounts from a JSON string containing an array of account objects
pub fn import_accounts(json_content: &str) -> Result<i32> {
    let accounts: Vec<serde_json::Value> = serde_json::from_str(json_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {}", e))?;

    let auth_dir = crate::config::resolve_auth_dir();
    std::fs::create_dir_all(&auth_dir)?;

    let mut imported = 0;

    for account in accounts {
        // Determine filename based on provider and email/id
        let provider = account.get("provider")
            .or_else(|| account.get("type"))
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let email = account.get("email")
            .and_then(|v| v.as_str())
            .map(|s| s.replace(['@', '.'], "_"));

        let filename = if let Some(email) = email {
            format!("{}_{}.json", provider, email)
        } else {
            // Generate a unique filename
            let id = uuid::Uuid::new_v4().to_string().replace("-", "")[..8].to_string();
            format!("{}_{}.json", provider, id)
        };

        let path = auth_dir.join(&filename);

        // Write the account file
        let content = serde_json::to_string_pretty(&account)?;
        std::fs::write(&path, content)?;

        tracing::info!("Imported account to {:?}", path);
        imported += 1;
    }

    Ok(imported)
}

/// Export all accounts directly to a file
pub fn export_accounts_to_file(file_path: &str) -> Result<()> {
    let content = export_all_accounts()?;
    std::fs::write(file_path, content)?;
    tracing::info!("Exported accounts to {}", file_path);
    Ok(())
}

/// Import accounts from a file
pub fn import_accounts_from_file(file_path: &str) -> Result<i32> {
    let content = std::fs::read_to_string(file_path)?;
    import_accounts(&content)
}
