// OpenAI/Codex OAuth implementation

use anyhow::Result;
use axum::{
    extract::Query,
    response::{Html, IntoResponse},
    routing::get,
    Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use once_cell::sync::Lazy;
use parking_lot::RwLock;
use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::oneshot;

const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
const CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OAUTH_CALLBACK_PORT: u16 = 1455;

/// Store pending OAuth sessions (state -> PKCE verifier)
static PENDING_OAUTH: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

/// PKCE codes for OAuth flow
#[derive(Debug, Clone)]
pub struct PKCECodes {
    pub code_verifier: String,
    pub code_challenge: String,
}

impl PKCECodes {
    /// Generate new PKCE codes
    pub fn new() -> Self {
        // Generate a random 32-byte code verifier
        let mut rng = rand::thread_rng();
        let random_bytes: Vec<u8> = (0..32).map(|_| rng.gen()).collect();
        let code_verifier = URL_SAFE_NO_PAD.encode(&random_bytes);

        // Generate code challenge using SHA256
        let mut hasher = Sha256::new();
        hasher.update(code_verifier.as_bytes());
        let hash = hasher.finalize();
        let code_challenge = URL_SAFE_NO_PAD.encode(&hash);

        Self {
            code_verifier,
            code_challenge,
        }
    }
}

/// Generate a random state string for CSRF protection
fn generate_state() -> String {
    let mut rng = rand::thread_rng();
    let random_bytes: Vec<u8> = (0..16).map(|_| rng.gen()).collect();
    URL_SAFE_NO_PAD.encode(&random_bytes)
}

/// Token response from OpenAI
#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub id_token: Option<String>,
    pub token_type: String,
    pub expires_in: Option<u64>,
}

/// JWT claims for extracting user info
#[derive(Debug, Deserialize)]
pub struct JwtClaims {
    pub sub: Option<String>,
    pub email: Option<String>,
    #[serde(rename = "https://api.openai.com/auth", default)]
    pub auth_info: Option<AuthInfo>,
}

#[derive(Debug, Deserialize)]
pub struct AuthInfo {
    pub user_id: Option<String>,
}

/// Parse JWT token to extract claims (without verification)
fn parse_jwt_claims(token: &str) -> Option<JwtClaims> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }

    let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;
    serde_json::from_slice(&payload).ok()
}

/// Get the PKCE verifier for a given state
pub fn get_pkce_verifier(state: &str) -> Option<String> {
    PENDING_OAUTH.write().remove(state)
}

pub async fn start_oauth() -> Result<String> {
    let pkce = PKCECodes::new();
    let state = generate_state();

    // Store PKCE verifier for later use in token exchange
    PENDING_OAUTH
        .write()
        .insert(state.clone(), pkce.code_verifier.clone());

    tracing::info!("Generated OAuth state: {}", state);

    let params = [
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid email profile offline_access"),
        ("state", &state),
        ("code_challenge", &pkce.code_challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "login"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
    ];

    let query_string = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let auth_url = format!("{}?{}", OPENAI_AUTH_URL, query_string);

    Ok(auth_url)
}

/// Exchange authorization code for tokens
pub async fn exchange_code(code: &str, code_verifier: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", CLIENT_ID),
        ("code", code),
        ("redirect_uri", REDIRECT_URI),
        ("code_verifier", code_verifier),
    ];

    let response = client
        .post(OPENAI_TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Accept", "application/json")
        .form(&params)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!(
            "Token exchange failed with status {}: {}",
            status,
            body
        ));
    }

    let token_response: TokenResponse = response.json().await?;
    Ok(token_response)
}

/// Extract email from token response
pub fn extract_email(token_response: &TokenResponse) -> Option<String> {
    if let Some(id_token) = &token_response.id_token {
        if let Some(claims) = parse_jwt_claims(id_token) {
            return claims.email;
        }
    }
    None
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

/// Kill any process using the OAuth callback port
fn kill_process_on_port(port: u16) {
    #[cfg(target_os = "macos")]
    {
        // Use lsof to find and kill process on the port
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
        // On Windows, use netstat to find PID and taskkill to kill it
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

/// Start OAuth flow and wait for callback
pub async fn start_oauth_with_callback() -> Result<OAuthResult> {
    let pkce = PKCECodes::new();
    let state = generate_state();

    // Store PKCE verifier
    PENDING_OAUTH
        .write()
        .insert(state.clone(), pkce.code_verifier.clone());

    tracing::info!("Starting OAuth flow with state: {}", state);

    // Build auth URL
    let params = [
        ("client_id", CLIENT_ID),
        ("response_type", "code"),
        ("redirect_uri", REDIRECT_URI),
        ("scope", "openid email profile offline_access"),
        ("state", &state),
        ("code_challenge", &pkce.code_challenge),
        ("code_challenge_method", "S256"),
        ("prompt", "login"),
        ("id_token_add_organizations", "true"),
        ("codex_cli_simplified_flow", "true"),
    ];

    let query_string = params
        .iter()
        .map(|(k, v)| format!("{}={}", k, urlencoding::encode(v)))
        .collect::<Vec<_>>()
        .join("&");

    let auth_url = format!("{}?{}", OPENAI_AUTH_URL, query_string);

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
    let app = Router::new().route("/auth/callback", get(callback_handler));

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

    tracing::info!("OAuth callback server listening on {}", addr);

    // Return the auth URL - the caller should open it in the browser
    // We don't open the browser here to avoid Tauri shell permission issues
    tracing::info!("Please open this URL in your browser: {}", auth_url);

    // Try to open browser using system command directly
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

    // Wait for result with timeout
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
        tracing::error!("OAuth error: {}", error_msg);

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

    // Get PKCE verifier
    let code_verifier = match get_pkce_verifier(&oauth_state) {
        Some(v) => v,
        None => {
            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Err(anyhow::anyhow!("Invalid or expired OAuth session")));
            }
            return Html(ERROR_HTML.replace("{{ERROR}}", "Invalid or expired OAuth session"));
        }
    };

    // Exchange code for tokens
    match exchange_code(&code, &code_verifier).await {
        Ok(token_response) => {
            let email = extract_email(&token_response);
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
