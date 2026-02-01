// Kiro account import implementation
// Reads credentials from local kiro-cli SQLite database

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// SQLite token keys (searched in priority order)
const SQLITE_TOKEN_KEYS: &[&str] = &[
    "kirocli:social:token",      // Social login (Google, GitHub, Microsoft, etc.)
    "kirocli:odic:token",        // AWS SSO OIDC (kiro-cli corporate)
    "codewhisperer:odic:token",  // Legacy AWS SSO OIDC
];

// Device registration keys (for AWS SSO OIDC only)
const SQLITE_REGISTRATION_KEYS: &[&str] = &[
    "kirocli:odic:device-registration",
    "codewhisperer:odic:device-registration",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroCredentials {
    pub access_token: Option<String>,
    pub refresh_token: Option<String>,
    pub expires_at: Option<String>,
    pub region: Option<String>,
    pub profile_arn: Option<String>,
    pub scopes: Option<Vec<String>>,
    // AWS SSO OIDC specific
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    // Auth type detection
    pub auth_type: String,
    // Which key was used
    pub token_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KiroImportResult {
    pub email: Option<String>,
    pub credentials: KiroCredentials,
}

/// Get the default kiro-cli SQLite database path
pub fn get_default_db_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

    // Try kiro-cli path first
    let kiro_path = home.join(".local/share/kiro-cli/data.sqlite3");
    if kiro_path.exists() {
        return kiro_path;
    }

    // Fallback to amazon-q path
    let amazon_q_path = home.join(".local/share/amazon-q/data.sqlite3");
    if amazon_q_path.exists() {
        return amazon_q_path;
    }

    // Return kiro path as default even if it doesn't exist
    kiro_path
}

/// Import Kiro credentials from local SQLite database
pub async fn import_local_credentials() -> Result<KiroImportResult> {
    let db_path = get_default_db_path();

    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "Kiro CLI database not found. Please login with Kiro CLI first.\nExpected path: {:?}",
            db_path
        ));
    }

    tracing::info!("Reading Kiro credentials from: {:?}", db_path);

    let conn = rusqlite::Connection::open(&db_path)?;

    // Try all possible token keys in priority order
    let mut token_data: Option<(String, serde_json::Value)> = None;
    for key in SQLITE_TOKEN_KEYS {
        let result: Result<String, _> = conn.query_row(
            "SELECT value FROM auth_kv WHERE key = ?",
            [key],
            |row| row.get(0),
        );

        if let Ok(value) = result {
            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&value) {
                tracing::debug!("Found credentials with key: {}", key);
                token_data = Some((key.to_string(), json));
                break;
            }
        }
    }

    let (token_key, token_json) = token_data.ok_or_else(|| {
        anyhow::anyhow!("No valid Kiro credentials found in database. Please login with Kiro CLI first.")
    })?;

    // Extract token fields
    let access_token = token_json.get("access_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let refresh_token = token_json.get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let expires_at = token_json.get("expires_at")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let region = token_json.get("region")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let profile_arn = token_json.get("profile_arn")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let scopes = token_json.get("scopes")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        });

    // Try to load device registration for AWS SSO OIDC
    let mut client_id: Option<String> = None;
    let mut client_secret: Option<String> = None;

    for key in SQLITE_REGISTRATION_KEYS {
        let result: Result<String, _> = conn.query_row(
            "SELECT value FROM auth_kv WHERE key = ?",
            [key],
            |row| row.get(0),
        );

        if let Ok(value) = result {
            if let Ok(reg_json) = serde_json::from_str::<serde_json::Value>(&value) {
                client_id = reg_json.get("client_id")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                client_secret = reg_json.get("client_secret")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                if client_id.is_some() {
                    tracing::debug!("Found device registration with key: {}", key);
                    break;
                }
            }
        }
    }

    // Determine auth type
    let auth_type = if client_id.is_some() && client_secret.is_some() {
        "aws_sso_oidc".to_string()
    } else {
        "kiro_desktop".to_string()
    };

    tracing::info!("Detected Kiro auth type: {}", auth_type);

    // Try to extract email from access token (JWT)
    let email = access_token.as_ref().and_then(|token| extract_email_from_jwt(token));

    Ok(KiroImportResult {
        email,
        credentials: KiroCredentials {
            access_token,
            refresh_token,
            expires_at,
            region,
            profile_arn,
            scopes,
            client_id,
            client_secret,
            auth_type,
            token_key,
        },
    })
}

/// Extract email from JWT token (if present)
fn extract_email_from_jwt(token: &str) -> Option<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    // Decode the payload (second part)
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    let payload_str = String::from_utf8(payload).ok()?;
    let payload_json: serde_json::Value = serde_json::from_str(&payload_str).ok()?;

    // Try common email claim names
    payload_json.get("email")
        .or_else(|| payload_json.get("preferred_username"))
        .or_else(|| payload_json.get("sub"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Start OAuth flow - for Kiro, this imports local credentials
pub async fn start_oauth() -> Result<String> {
    // Import credentials from local database
    let result = import_local_credentials().await?;

    // Save to auth file
    let auth_dir = crate::config::resolve_auth_dir();
    let identifier = result.email.as_deref().unwrap_or("default");
    let path = auth_dir.join(format!("kiro_{}.json", identifier));

    let auth_data = serde_json::json!({
        "provider": "kiro",
        "type": "kiro",
        "email": result.email,
        "access_token": result.credentials.access_token,
        "refresh_token": result.credentials.refresh_token,
        "expires_at": result.credentials.expires_at,
        "region": result.credentials.region.unwrap_or_else(|| "us-east-1".to_string()),
        "profile_arn": result.credentials.profile_arn,
        "scopes": result.credentials.scopes,
        "client_id": result.credentials.client_id,
        "client_secret": result.credentials.client_secret,
        "auth_type": result.credentials.auth_type,
        "token_key": result.credentials.token_key,
        "enabled": true,
        "imported_at": chrono::Utc::now().to_rfc3339(),
    });

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let content = serde_json::to_string_pretty(&auth_data)?;
    std::fs::write(&path, content)?;

    tracing::info!("Saved Kiro auth file to {:?}", path);

    Ok(format!("Kiro account imported successfully: {}", identifier))
}
