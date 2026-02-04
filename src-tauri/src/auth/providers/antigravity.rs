// Antigravity OAuth implementation (Google-based)

use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

// Antigravity uses Google OAuth with specific client credentials
const AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";
const CLIENT_ID: &str = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep.apps.googleusercontent.com";
const CLIENT_SECRET: &str = "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";
fn get_redirect_uri() -> String {
    let config = crate::config::get_config().unwrap_or_default();
    format!("http://localhost:{}/antigravity/callback", config.port)
}
const API_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
const API_VERSION: &str = "v1internal";
const API_USER_AGENT: &str = "google-api-nodejs-client/9.15.1";
const API_CLIENT: &str = "google-cloud-sdk vscode_cloudshelleditor/0.1";
const CLIENT_METADATA: &str =
    r#"{"ideType":"IDE_UNSPECIFIED","platform":"PLATFORM_UNSPECIFIED","pluginType":"GEMINI"}"#;

// Store pending OAuth sessions
static PENDING_SESSIONS: Lazy<Mutex<HashMap<String, OAuthSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
pub struct OAuthSession {
    pub state: String,
    pub code_verifier: String,
    pub created_at: std::time::Instant,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: Option<String>,
}

/// Generate a random string for state/verifier
fn generate_random_string(length: usize) -> String {
    let mut rng = rand::rng();
    let chars: Vec<char> = (0..length)
        .map(|_| {
            let idx = rng.random_range(0..62);
            match idx {
                0..=25 => (b'a' + idx) as char,
                26..=51 => (b'A' + idx - 26) as char,
                _ => (b'0' + idx - 52) as char,
            }
        })
        .collect();
    chars.into_iter().collect()
}

/// Generate PKCE code verifier
fn generate_code_verifier() -> String {
    generate_random_string(64)
}

/// Generate PKCE code challenge from verifier (S256 method)
fn generate_code_challenge(verifier: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    URL_SAFE_NO_PAD.encode(hash)
}

/// Start OAuth flow and return the authorization URL
pub async fn start_oauth() -> Result<String> {
    let state = generate_random_string(32);
    let code_verifier = generate_code_verifier();
    let code_challenge = generate_code_challenge(&code_verifier);

    // Store session for later verification
    let session = OAuthSession {
        state: state.clone(),
        code_verifier,
        created_at: std::time::Instant::now(),
    };

    PENDING_SESSIONS.lock().insert(state.clone(), session);

    // Clean up old sessions
    cleanup_old_sessions();

    // Antigravity-specific scopes
    let scopes = [
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
        "https://www.googleapis.com/auth/cclog",
        "https://www.googleapis.com/auth/experimentsandconfigs",
    ]
    .join(" ");

    let redirect_uri = get_redirect_uri();
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        AUTH_URL,
        CLIENT_ID,
        urlencoding::encode(&redirect_uri),
        urlencoding::encode(&scopes),
        state,
        code_challenge
    );

    tracing::info!("Generated Antigravity OAuth URL with state: {}", state);
    Ok(auth_url)
}

/// Exchange authorization code for tokens
pub async fn exchange_code(code: &str, state: &str) -> Result<TokenResponse> {
    let session = PENDING_SESSIONS
        .lock()
        .remove(state)
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired OAuth state"))?;

    let client = reqwest::Client::new();

    let redirect_uri = get_redirect_uri();
    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("code_verifier", &session.code_verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", redirect_uri.as_str()),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Token exchange failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;
    tracing::info!("Successfully exchanged code for Antigravity tokens");

    Ok(token_response)
}

/// Get user info using access token
pub async fn get_user_info(access_token: &str) -> Result<UserInfo> {
    let client = reqwest::Client::new();

    let response = client
        .get(USERINFO_URL)
        .bearer_auth(access_token)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Failed to get user info: {}", error_text));
    }

    let user_info: UserInfo = response.json().await?;
    Ok(user_info)
}

/// Refresh access token using refresh token
pub async fn refresh_token(refresh_token: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
    ];

    let response = client
        .post(TOKEN_URL)
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Token refresh failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;
    Ok(token_response)
}

/// Fetch Antigravity project id (loadCodeAssist + onboardUser fallback)
pub async fn fetch_project_id(access_token: &str) -> Result<String> {
    let access_token = access_token.trim();
    if access_token.is_empty() {
        return Err(anyhow::anyhow!("missing access token"));
    }

    let load_resp = load_code_assist(access_token).await?;
    if let Some(project_id) = extract_project_id(&load_resp) {
        return Ok(project_id);
    }

    let tier_id = extract_default_tier(&load_resp).unwrap_or_else(|| "legacy-tier".to_string());
    onboard_user(access_token, &tier_id).await
}

async fn load_code_assist(access_token: &str) -> Result<serde_json::Value> {
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    let endpoint = format!("{}/{}:loadCodeAssist", API_ENDPOINT, API_VERSION);
    let client = reqwest::Client::new();
    let response = client
        .post(&endpoint)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("User-Agent", API_USER_AGENT)
        .header("X-Goog-Api-Client", API_CLIENT)
        .header("Client-Metadata", CLIENT_METADATA)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "loadCodeAssist failed: {} {}",
            status,
            text
        ));
    }

    let value: serde_json::Value = response.json().await?;
    Ok(value)
}

async fn onboard_user(access_token: &str, tier_id: &str) -> Result<String> {
    let body = serde_json::json!({
        "tierId": tier_id,
        "metadata": {
            "ideType": "ANTIGRAVITY",
            "platform": "PLATFORM_UNSPECIFIED",
            "pluginType": "GEMINI"
        }
    });

    let endpoint = format!("{}/{}:onboardUser", API_ENDPOINT, API_VERSION);
    let client = reqwest::Client::new();

    for _ in 0..5 {
        let response = client
            .post(&endpoint)
            .bearer_auth(access_token)
            .header("Content-Type", "application/json")
            .header("User-Agent", API_USER_AGENT)
            .header("X-Goog-Api-Client", API_CLIENT)
            .header("Client-Metadata", CLIENT_METADATA)
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if status.is_success() {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                let done = value.get("done").and_then(|v| v.as_bool()).unwrap_or(false);
                if done {
                    if let Some(resp) = value.get("response") {
                        if let Some(project_id) = extract_project_id(resp) {
                            return Ok(project_id);
                        }
                    }
                    return Err(anyhow::anyhow!("no project_id in onboard response"));
                }
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            continue;
        }
        return Err(anyhow::anyhow!(
            "onboardUser failed: {} {}",
            status,
            text
        ));
    }

    Err(anyhow::anyhow!("onboardUser did not complete"))
}

fn extract_project_id(value: &serde_json::Value) -> Option<String> {
    if let Some(project) = value.get("cloudaicompanionProject") {
        if let Some(id) = project.as_str() {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        if let Some(id) = project.get("id").and_then(|v| v.as_str()) {
            let id = id.trim();
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
    }
    None
}

fn extract_default_tier(value: &serde_json::Value) -> Option<String> {
    value.get("allowedTiers").and_then(|tiers| tiers.as_array()).and_then(|tiers| {
        for tier in tiers {
            let is_default = tier.get("isDefault").and_then(|v| v.as_bool()).unwrap_or(false);
            if !is_default {
                continue;
            }
            if let Some(id) = tier.get("id").and_then(|v| v.as_str()) {
                let id = id.trim();
                if !id.is_empty() {
                    return Some(id.to_string());
                }
            }
        }
        None
    })
}

/// Clean up OAuth sessions older than 10 minutes
fn cleanup_old_sessions() {
    let mut sessions = PENDING_SESSIONS.lock();
    let now = std::time::Instant::now();
    sessions.retain(|_, session| now.duration_since(session.created_at).as_secs() < 600);
}

/// Get pending session by state (for verification)
pub fn get_pending_session(state: &str) -> Option<OAuthSession> {
    PENDING_SESSIONS.lock().get(state).cloned()
}

// Quota API
const QUOTA_API_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels";
const LOAD_CODE_ASSIST_URL: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:loadCodeAssist";
const QUOTA_USER_AGENT: &str = "antigravity/1.0.0 macos/aarch64";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelQuota {
    pub name: String,
    pub percentage: i32,
    pub reset_time: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaData {
    pub models: Vec<ModelQuota>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_forbidden: bool,
    #[serde(default)]
    pub subscription_tier: Option<String>,
    #[serde(default)]
    pub project_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QuotaResponse {
    models: Option<HashMap<String, QuotaModelInfo>>,
}

#[derive(Debug, Deserialize)]
struct QuotaModelInfo {
    #[serde(rename = "quotaInfo")]
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Deserialize)]
struct QuotaInfo {
    #[serde(rename = "remainingFraction")]
    remaining_fraction: Option<f64>,
    #[serde(rename = "resetTime")]
    reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct LoadCodeAssistResponse {
    #[serde(rename = "cloudaicompanionProject")]
    project_id: Option<String>,
    #[serde(rename = "currentTier")]
    current_tier: Option<TierInfo>,
    #[serde(rename = "paidTier")]
    paid_tier: Option<TierInfo>,
}

#[derive(Debug, Deserialize)]
struct TierInfo {
    id: Option<String>,
}

/// Fetch project ID and subscription tier
pub async fn fetch_project_and_tier(access_token: &str) -> Result<(Option<String>, Option<String>)> {
    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "metadata": {
            "ideType": "ANTIGRAVITY"
        }
    });

    let response = client
        .post(LOAD_CODE_ASSIST_URL)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("User-Agent", QUOTA_USER_AGENT)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        tracing::warn!("loadCodeAssist failed: {} {}", status, text);
        return Ok((None, None));
    }

    let load_resp: LoadCodeAssistResponse = response.json().await?;

    let project_id = load_resp.project_id;

    // Priority: paid_tier > current_tier
    let subscription_tier = load_resp.paid_tier
        .and_then(|t| t.id)
        .or_else(|| load_resp.current_tier.and_then(|t| t.id));

    Ok((project_id, subscription_tier))
}

/// Fetch quota for an Antigravity account
pub async fn fetch_quota(access_token: &str, cached_project_id: Option<&str>, _cached_subscription_tier: Option<&str>) -> Result<QuotaData> {
    // Always fetch fresh project_id and subscription_tier from API
    // This ensures we get the latest subscription status (FREE/PRO) when user upgrades
    // Note: We ignore cached_subscription_tier and always fetch fresh data
    let (api_project_id, subscription_tier) = fetch_project_and_tier(access_token).await?;
    
    // Use cached project_id if available (for performance), but always use fresh subscription_tier
    let project_id = if cached_project_id.is_some() {
        cached_project_id.map(|s| s.to_string())
    } else {
        api_project_id.clone()
    };

    let final_project_id = project_id.clone().unwrap_or_else(|| "bamboo-precept-lgxtn".to_string());

    let client = reqwest::Client::new();
    let body = serde_json::json!({
        "project": final_project_id
    });

    let response = client
        .post(QUOTA_API_URL)
        .bearer_auth(access_token)
        .header("Content-Type", "application/json")
        .header("User-Agent", QUOTA_USER_AGENT)
        .json(&body)
        .send()
        .await?;

    // Handle 403 Forbidden
    if response.status() == reqwest::StatusCode::FORBIDDEN {
        tracing::warn!("Antigravity account forbidden (403)");
        return Ok(QuotaData {
            models: vec![],
            last_updated: chrono::Utc::now().timestamp(),
            is_forbidden: true,
            subscription_tier,
            project_id,
        });
    }

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("fetchQuota failed: {} {}", status, text));
    }

    let quota_resp: QuotaResponse = response.json().await?;

    let mut models = Vec::new();
    if let Some(model_map) = quota_resp.models {
        for (name, info) in model_map {
            // Only keep gemini and claude models
            if !name.contains("gemini") && !name.contains("claude") {
                continue;
            }

            if let Some(quota_info) = info.quota_info {
                let percentage = quota_info.remaining_fraction
                    .map(|f| (f * 100.0) as i32)
                    .unwrap_or(0);

                let reset_time = quota_info.reset_time.unwrap_or_default();

                models.push(ModelQuota {
                    name,
                    percentage,
                    reset_time,
                });
            }
        }
    }

    Ok(QuotaData {
        models,
        last_updated: chrono::Utc::now().timestamp(),
        is_forbidden: false,
        subscription_tier,
        project_id,
    })
}
