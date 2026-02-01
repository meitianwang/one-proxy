// Google/Gemini OAuth implementation - CLIProxyAPI compatible
// This implementation matches CLIProxyAPI exactly for compatibility

use anyhow::Result;
use axum::{extract::Query, response::{Html, IntoResponse}, Router, routing::get};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;

// Google OAuth endpoints - same as golang.org/x/oauth2/google
const GOOGLE_AUTH_URL: &str = "https://accounts.google.com/o/oauth2/auth";
const GOOGLE_TOKEN_URL: &str = "https://oauth2.googleapis.com/token";
// Use v1 API like CLIProxyAPI
const GOOGLE_USERINFO_URL: &str = "https://www.googleapis.com/oauth2/v1/userinfo?alt=json";

// OAuth configuration constants - same as CLIProxyAPI
pub const GOOGLE_CLIENT_ID: &str = "681255809395-oo8ft2oprdrnp9e3aqf6av3hmdib135j.apps.googleusercontent.com";
pub const GOOGLE_CLIENT_SECRET: &str = "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";
const OAUTH_CALLBACK_PORT: u16 = 8085;
const REDIRECT_URI: &str = "http://localhost:8085/oauth2callback";

// OAuth scopes - same as CLIProxyAPI
pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
];

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

pub struct OAuthFlowHandle {
    pub auth_url: String,
    pub result_rx: oneshot::Receiver<Result<OAuthResult>>,
    pub shutdown_tx: oneshot::Sender<()>,
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

/// Start OAuth flow with dedicated callback server - CLIProxyAPI compatible
/// This matches the getTokenFromWeb function in CLIProxyAPI
pub async fn start_oauth_with_callback() -> Result<OAuthResult> {
    // CLIProxyAPI uses fixed "state-token" string
    let state = "state-token".to_string();

    tracing::info!("Starting Google OAuth flow (CLIProxyAPI compatible)");

    let scopes = SCOPES.join(" ");

    // Build auth URL exactly like CLIProxyAPI:
    // config.AuthCodeURL("state-token", oauth2.AccessTypeOffline, oauth2.SetAuthURLParam("prompt", "consent"))
    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}",
        GOOGLE_AUTH_URL,
        GOOGLE_CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&scopes),
        state
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

    // Wait for result with timeout (5 minutes) - same as CLIProxyAPI
    let result = tokio::time::timeout(std::time::Duration::from_secs(300), result_rx).await;

    // Shutdown server
    let _ = shutdown_tx.send(());

    match result {
        Ok(Ok(res)) => res,
        Ok(Err(_)) => Err(anyhow::anyhow!("OAuth callback channel closed")),
        Err(_) => Err(anyhow::anyhow!("OAuth flow timed out after 5 minutes")),
    }
}

/// Start OAuth flow and return the auth URL plus a handle to await the result.
/// The caller is responsible for opening the auth URL and handling the result.
pub async fn start_oauth_with_callback_url() -> Result<OAuthFlowHandle> {
    // CLIProxyAPI uses fixed "state-token" string
    let state = "state-token".to_string();

    tracing::info!("Starting Google OAuth flow (CLIProxyAPI compatible)");

    let scopes = SCOPES.join(" ");

    let auth_url = format!(
        "{}?client_id={}&redirect_uri={}&response_type=code&scope={}&access_type=offline&prompt=consent&state={}",
        GOOGLE_AUTH_URL,
        GOOGLE_CLIENT_ID,
        urlencoding::encode(REDIRECT_URI),
        urlencoding::encode(&scopes),
        state
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
        async move { handle_callback(params, state).await }
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

    Ok(OAuthFlowHandle {
        auth_url,
        result_rx,
        shutdown_tx,
    })
}

async fn handle_callback(
    params: OAuthCallbackParams,
    state: Arc<RwLock<CallbackState>>,
) -> impl IntoResponse {
    // Check for errors - same as CLIProxyAPI
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Google OAuth error: {}", error_msg);

        if let Some(tx) = state.write().result_tx.take() {
            let _ = tx.send(Err(anyhow::anyhow!("Authentication failed: {}", error_msg)));
        }

        return Html(ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            if let Some(tx) = state.write().result_tx.take() {
                let _ = tx.send(Err(anyhow::anyhow!("Authentication failed: code not found")));
            }
            return Html(ERROR_HTML.replace("{{ERROR}}", "Authentication failed: code not found"));
        }
    };

    // Verify state - CLIProxyAPI uses fixed "state-token"
    if params.state.as_deref() != Some("state-token") {
        if let Some(tx) = state.write().result_tx.take() {
            let _ = tx.send(Err(anyhow::anyhow!("Invalid state parameter")));
        }
        return Html(ERROR_HTML.replace("{{ERROR}}", "Invalid state parameter"));
    }

    // Exchange code for tokens - same as CLIProxyAPI config.Exchange()
    match exchange_code_internal(&code).await {
        Ok(token_response) => {
            // Get user info using v1 API like CLIProxyAPI
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

/// Exchange authorization code for tokens - matches CLIProxyAPI config.Exchange()
async fn exchange_code_internal(code: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let params = [
        ("client_id", GOOGLE_CLIENT_ID),
        ("client_secret", GOOGLE_CLIENT_SECRET),
        ("code", code),
        ("grant_type", "authorization_code"),
        ("redirect_uri", REDIRECT_URI),
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

/// Get user info using access token - uses v1 API like CLIProxyAPI
pub async fn get_user_info(access_token: &str) -> Result<UserInfo> {
    let client = reqwest::Client::new();

    let response = client
        .get(GOOGLE_USERINFO_URL)
        .header("Content-Type", "application/json")
        .header("Authorization", format!("Bearer {}", access_token))
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
        ("client_id", GOOGLE_CLIENT_ID),
        ("client_secret", GOOGLE_CLIENT_SECRET),
        ("refresh_token", refresh_token),
        ("grant_type", "refresh_token"),
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

/// Exchange authorization code for tokens - public API for external callback handlers
/// CLIProxyAPI uses fixed "state-token" so we just verify that
pub async fn exchange_code(code: &str, state: &str) -> Result<TokenResponse> {
    // Verify state matches CLIProxyAPI's fixed value
    if state != "state-token" {
        return Err(anyhow::anyhow!("Invalid state parameter"));
    }
    exchange_code_internal(code).await
}

// Gemini Quota API
const GEMINI_QUOTA_URL: &str = "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiQuotaData {
    pub models: Vec<GeminiModelQuota>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeminiModelQuota {
    pub model_id: String,
    pub remaining_fraction: f64,
    pub reset_time: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GeminiQuotaResponse {
    buckets: Option<Vec<BucketInfo>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BucketInfo {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
    model_id: Option<String>,
    token_type: Option<String>,
}

/// Fetch Gemini quota data
pub async fn fetch_gemini_quota(access_token: &str, project_id: Option<&str>) -> Result<GeminiQuotaData> {
    let client = reqwest::Client::new();

    let body = serde_json::json!({
        "project": project_id.unwrap_or("")
    });

    let response = client
        .post(GEMINI_QUOTA_URL)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        tracing::warn!("Gemini quota fetch failed: {} {}", status, text);
        return Ok(GeminiQuotaData {
            models: vec![],
            last_updated: chrono::Utc::now().timestamp(),
            is_error: true,
            error_message: Some(format!("API Error: {} {}", status, text)),
        });
    }

    let data: GeminiQuotaResponse = response.json().await?;

    let models = data.buckets
        .unwrap_or_default()
        .into_iter()
        .filter(|b| b.token_type.as_deref() == Some("REQUESTS"))
        .map(|b| GeminiModelQuota {
            model_id: b.model_id.unwrap_or_default(),
            remaining_fraction: b.remaining_fraction.unwrap_or(0.0),
            reset_time: b.reset_time,
        })
        .collect();

    Ok(GeminiQuotaData {
        models,
        last_updated: chrono::Utc::now().timestamp(),
        is_error: false,
        error_message: None,
    })
}
