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
const REDIRECT_URI: &str = "http://localhost:8417/antigravity/callback";

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

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        AUTH_URL,
        CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
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

    let params = [
        ("client_id", CLIENT_ID),
        ("client_secret", CLIENT_SECRET),
        ("code", code),
        ("code_verifier", &session.code_verifier),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
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
