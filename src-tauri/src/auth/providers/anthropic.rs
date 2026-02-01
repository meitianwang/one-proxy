// Anthropic/Claude OAuth implementation with PKCE

use anyhow::Result;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

const ANTHROPIC_AUTH_URL: &str = "https://claude.ai/oauth/authorize";
const ANTHROPIC_TOKEN_URL: &str = "https://console.anthropic.com/v1/oauth/token";
const ANTHROPIC_CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
const REDIRECT_URI: &str = "http://localhost:8417/anthropic/callback";

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
    pub token_type: String,
    pub expires_in: Option<u64>,
    pub organization: Option<Organization>,
    pub account: Option<Account>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Organization {
    pub uuid: String,
    pub name: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Account {
    pub uuid: String,
    pub email_address: Option<String>,
}

/// Generate a random string for state/verifier
fn generate_random_string(length: usize) -> String {
    use rand::Rng;
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

/// Generate PKCE code verifier (43-128 characters)
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

    // Clean up old sessions (older than 10 minutes)
    cleanup_old_sessions();

    let scopes = "org:create_api_key user:profile user:inference";

    let auth_url = format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        ANTHROPIC_AUTH_URL,
        ANTHROPIC_CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(scopes),
        code_challenge,
        state
    );

    tracing::info!("Generated Anthropic OAuth URL with state: {}", state);
    Ok(auth_url)
}

/// Parse code which may contain state fragment
fn parse_code_and_state(code: &str) -> (String, Option<String>) {
    let parts: Vec<&str> = code.split('#').collect();
    let parsed_code = parts[0].to_string();
    let parsed_state = if parts.len() > 1 {
        Some(parts[1].to_string())
    } else {
        None
    };
    (parsed_code, parsed_state)
}

/// Exchange authorization code for tokens
pub async fn exchange_code(code: &str, state: &str) -> Result<TokenResponse> {
    let session = PENDING_SESSIONS
        .lock()
        .remove(state)
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired OAuth state"))?;

    let (parsed_code, new_state) = parse_code_and_state(code);

    let client = reqwest::Client::new();

    let mut body = serde_json::json!({
        "code": parsed_code,
        "grant_type": "authorization_code",
        "client_id": ANTHROPIC_CLIENT_ID,
        "redirect_uri": REDIRECT_URI,
        "code_verifier": session.code_verifier
    });

    // Use new_state if present, otherwise use original state
    if let Some(ns) = new_state {
        body["state"] = serde_json::Value::String(ns);
    } else {
        body["state"] = serde_json::Value::String(state.to_string());
    }

    let response = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Token exchange failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;
    tracing::info!("Successfully exchanged code for Anthropic tokens");

    Ok(token_response)
}

/// Refresh access token using refresh token
pub async fn refresh_token(refresh_token: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "client_id": ANTHROPIC_CLIENT_ID,
        "grant_type": "refresh_token",
        "refresh_token": refresh_token
    });

    let response = client
        .post(ANTHROPIC_TOKEN_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Token refresh failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;
    Ok(token_response)
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
