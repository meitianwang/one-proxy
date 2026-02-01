// Google/Gemini OAuth implementation with PKCE

use anyhow::Result;
use axum::{extract::Query, response::{Html, IntoResponse}, Router, routing::get};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use once_cell::sync::Lazy;
use parking_lot::{Mutex, RwLock};
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v2/userinfo";
const OAUTH_CALLBACK_PORT: u16 = 8085;
const REDIRECT_URI: &str = "http://localhost:8085/oauth2callback";

fn get_google_client_id() -> String {
    std::env::var("GOOGLE_CLIENT_ID").unwrap_or_else(|_| "YOUR_GOOGLE_CLIENT_ID".to_string())
}

fn get_google_client_secret() -> String {
    std::env::var("GOOGLE_CLIENT_SECRET").unwrap_or_else(|_| "YOUR_GOOGLE_CLIENT_SECRET".to_string())
}

// Store pending OAuth sessions
static PENDING_SESSIONS: Lazy<Mutex<HashMap<String, OAuthSession>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

#[derive(Debug, Clone)]
pub struct OAuthSession {
    pub state: String,
    pub code_verifier: String,
    pub created_at: std::time::Instant,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<u64>,
    pub token_type: String,
    pub scope: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub email: Option<String>,
    pub name: Option<String>,
}

// OAuth callback query parameters
#[derive(Debug, Deserialize)]
pub struct OAuthCallbackParams {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

// OAuth result
#[derive(Debug, Clone)]
pub struct OAuthResult {
    pub token_response: TokenResponse,
    pub email: Option<String>,
}

// Shared state for the callback server
struct CallbackState {
    result_tx: Option<oneshot::Sender<Result<OAuthResult>>>,
}

const SUCCESS_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Authentication Successful</title>
    <script>setTimeout(function(){window.close();}, 3000);</script>
    <style>
        body { font-family: system-ui, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { text-align: center; padding: 2rem; background: white; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #22c55e; }
    </style>
</head>
<body>
    <div class="container">
        <h1>✓ Authentication Successful!</h1>
        <p>You can close this window.</p>
    </div>
</body>
</html>
"#;

const ERROR_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Authentication Failed</title>
    <style>
        body { font-family: system-ui, sans-serif; display: flex; justify-content: center; align-items: center; height: 100vh; margin: 0; background: #f5f5f5; }
        .container { text-align: center; padding: 2rem; background: white; border-radius: 8px; box-shadow: 0 2px 10px rgba(0,0,0,0.1); }
        h1 { color: #ef4444; }
    </style>
</head>
<body>
    <div class="container">
        <h1>✗ Authentication Failed</h1>
        <p>{{ERROR}}</p>
        <p>Please close this window and try again.</p>
    </div>
</body>
</html>
"#;

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

/// Kill any process using the OAuth callback port
fn kill_process_on_port(port: u16) {
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
        {
            let pids = String::from_utf8_lossy(&output.stdout);
            for pid in pids.lines() {
                if let Ok(pid_num) = pid.trim().parse::<i32>() {
                    tracing::info!("Killing process {} on port {}", pid_num, port);
                    let _ = std::process::Command::new("kill")
                        .args(["-9", &pid_num.to_string()])
                        .output();
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(output) = std::process::Command::new("fuser")
            .args(["-k", &format!("{}/tcp", port)])
            .output()
        {
            tracing::info!("Killed processes on port {}: {:?}", port, output);
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("cmd")
            .args(["/c", &format!("for /f \"tokens=5\" %a in ('netstat -aon ^| findstr :{} ^| findstr LISTENING') do taskkill /F /PID %a", port)])
            .output()
        {
            tracing::info!("Killed processes on port {}: {:?}", port, output);
        }
    }

    // Give the OS a moment to release the port
    std::thread::sleep(std::time::Duration::from_millis(100));
}

/// Start OAuth flow and return the authorization URL (legacy, for use with external callback handler)
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

    let scopes = [
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
    ]
    .join(" ");

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        GOOGLE_AUTH_URL,
        get_google_client_id(),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&scopes),
        state,
        code_challenge
    );

    tracing::info!("Generated Google OAuth URL with state: {}", state);
    Ok(auth_url)
}

/// Start OAuth flow with dedicated callback server
pub async fn start_oauth_with_callback() -> Result<OAuthResult> {
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

    tracing::info!("Starting Google OAuth flow with state: {}", state);

    let scopes = [
        "https://www.googleapis.com/auth/cloud-platform",
        "https://www.googleapis.com/auth/userinfo.email",
        "https://www.googleapis.com/auth/userinfo.profile",
    ]
    .join(" ");

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}&code_challenge={}&code_challenge_method=S256",
        GOOGLE_AUTH_URL,
        get_google_client_id(),
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&scopes),
        state,
        code_challenge
    );

    // Create channel for result
    let (result_tx, result_rx) = oneshot::channel();

    // Create shared state
    let callback_state = Arc::new(RwLock::new(CallbackState {
        result_tx: Some(result_tx),
    }));

    // Create callback handler
    let state_clone = callback_state.clone();
    let callback_handler = move |Query(params): Query<OAuthCallbackParams>| {
        let state = state_clone.clone();
        async move {
            handle_callback(params, state).await
        }
    };

    // Build router
    let app = Router::new().route("/oauth2callback", get(callback_handler));

    // Kill any existing process on the port
    kill_process_on_port(OAUTH_CALLBACK_PORT);

    // Try to bind to the port
    let addr = format!("127.0.0.1:{}", OAUTH_CALLBACK_PORT);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            return Err(anyhow::anyhow!(
                "Failed to start OAuth callback server on port {}: {}",
                OAUTH_CALLBACK_PORT,
                e
            ));
        }
    };

    tracing::info!("Google OAuth callback server listening on {}", addr);

    // Open browser using system command
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open")
            .arg(&auth_url)
            .spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", &auth_url])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open")
            .arg(&auth_url)
            .spawn();
    }

    // Spawn server with graceful shutdown
    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

    tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                shutdown_rx.await.ok();
            })
            .await
            .ok();
    });

    // Wait for result with timeout (5 minutes)
    let result = tokio::time::timeout(std::time::Duration::from_secs(300), result_rx).await;

    // Shutdown server
    let _ = shutdown_tx.send(());

    match result {
        Ok(Ok(res)) => res,
        Ok(Err(_)) => Err(anyhow::anyhow!("OAuth callback channel closed")),
        Err(_) => Err(anyhow::anyhow!("OAuth flow timed out after 5 minutes")),
    }
}

async fn handle_callback(
    params: OAuthCallbackParams,
    state: Arc<RwLock<CallbackState>>,
) -> impl IntoResponse {
    // Check for errors
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Google OAuth error: {}", error_msg);

        if let Some(tx) = state.write().result_tx.take() {
            let _ = tx.send(Err(anyhow::anyhow!("OAuth error: {}", error_msg)));
        }

        return Html(ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Err(anyhow::anyhow!("No authorization code received")));
            }
            return Html(ERROR_HTML.replace("{{ERROR}}", "No authorization code received"));
        }
    };

    let oauth_state = match params.state {
        Some(s) => s,
        None => {
            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Err(anyhow::anyhow!("No state parameter received")));
            }
            return Html(ERROR_HTML.replace("{{ERROR}}", "No state parameter received"));
        }
    };

    // Get session and exchange code
    match exchange_code(&code, &oauth_state).await {
        Ok(token_response) => {
            // Get user info
            let email = match get_user_info(&token_response.access_token).await {
                Ok(user_info) => user_info.email,
                Err(e) => {
                    tracing::warn!("Failed to get user info: {}", e);
                    None
                }
            };

            let result = OAuthResult {
                token_response,
                email,
            };

            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Ok(result));
            }

            Html(SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Token exchange failed: {}", e);

            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Err(e));
            }

            Html(ERROR_HTML.replace("{{ERROR}}", "Token exchange failed"))
        }
    }
}

/// Exchange authorization code for tokens
pub async fn exchange_code(code: &str, state: &str) -> Result<TokenResponse> {
    let session = PENDING_SESSIONS
        .lock()
        .remove(state)
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired OAuth state"))?;

    let client = reqwest::Client::new();

    let params = [
        ("client_id", get_google_client_id()),
        ("client_secret", get_google_client_secret()),
        ("code", code.to_string()),
        ("code_verifier", session.code_verifier),
        ("grant_type", "authorization_code".to_string()),
        ("redirect_uri", REDIRECT_URI.to_string()),
    ];

    let response = client
        .post(GOOGLE_TOKEN_URL)
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        let error_text = response.text().await?;
        return Err(anyhow::anyhow!("Token exchange failed: {}", error_text));
    }

    let token_response: TokenResponse = response.json().await?;
    tracing::info!("Successfully exchanged code for Google tokens");

    Ok(token_response)
}

/// Get user info using access token
pub async fn get_user_info(access_token: &str) -> Result<UserInfo> {
    let client = reqwest::Client::new();

    let response = client
        .get(GOOGLE_USERINFO_URL)
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
        ("client_id", get_google_client_id()),
        ("client_secret", get_google_client_secret()),
        ("refresh_token", refresh_token.to_string()),
        ("grant_type", "refresh_token".to_string()),
    ];

    let response = client
        .post(GOOGLE_TOKEN_URL)
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
