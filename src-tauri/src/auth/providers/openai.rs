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
use std::time::Duration;
use tokio::sync::oneshot;
use uuid::Uuid;

const OPENAI_AUTH_URL: &str = "https://auth.openai.com/oauth/authorize";
const OPENAI_TOKEN_URL: &str = "https://auth.openai.com/oauth/token";
fn get_client_id() -> String {
    std::env::var("OPENAI_OAUTH_CLIENT_ID")
        .unwrap_or_else(|_| include_str!("../../../credentials/openai.txt").lines().next().unwrap_or_default().to_string())
}
const REDIRECT_URI: &str = "http://localhost:1455/auth/callback";
const OAUTH_CALLBACK_PORT: u16 = 1455;
const CODEX_DEVICE_USER_CODE_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/usercode";
const CODEX_DEVICE_TOKEN_URL: &str = "https://auth.openai.com/api/accounts/deviceauth/token";
const CODEX_DEVICE_VERIFICATION_URL: &str = "https://auth.openai.com/codex/device";
const CODEX_DEVICE_TOKEN_EXCHANGE_REDIRECT_URI: &str =
    "https://auth.openai.com/deviceauth/callback";
const CODEX_DEVICE_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const CODEX_DEVICE_DEFAULT_POLL_INTERVAL: Duration = Duration::from_secs(5);

/// Store pending OAuth sessions (state -> PKCE verifier)
static PENDING_OAUTH: Lazy<RwLock<HashMap<String, String>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
static PENDING_DEVICE_OAUTH: Lazy<RwLock<HashMap<String, PendingDeviceFlow>>> =
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
    pub auth_info: Option<CodexAuthInfo>,
}

#[derive(Debug, Deserialize)]
pub struct CodexAuthInfo {
    #[serde(default)]
    pub chatgpt_account_id: Option<String>,
    #[serde(default)]
    pub chatgpt_plan_type: Option<String>,
    #[serde(default)]
    pub user_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CodexIdentity {
    pub email: Option<String>,
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
}

#[derive(Debug, Clone)]
struct PendingDeviceFlow {
    device_auth_id: String,
    user_code: String,
    poll_interval: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceOAuthStart {
    pub session_id: String,
    pub verification_url: String,
    pub user_code: String,
    pub interval_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DeviceUserCodeRequest {
    client_id: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceUserCodeResponse {
    device_auth_id: String,
    #[serde(default)]
    user_code: Option<String>,
    #[serde(default)]
    usercode: Option<String>,
    #[serde(default)]
    interval: Option<ValueOrStringNumber>,
}

#[derive(Debug, Clone, Serialize)]
struct DeviceTokenRequest {
    device_auth_id: String,
    user_code: String,
}

#[derive(Debug, Clone, Deserialize)]
struct DeviceTokenResponse {
    authorization_code: String,
    code_verifier: String,
    #[allow(dead_code)]
    code_challenge: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ValueOrStringNumber {
    String(String),
    Number(u64),
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

pub fn extract_codex_identity_from_id_token(id_token: Option<&str>) -> CodexIdentity {
    let Some(id_token) = id_token else {
        return CodexIdentity {
            email: None,
            account_id: None,
            plan_type: None,
        };
    };
    let Some(claims) = parse_jwt_claims(id_token) else {
        return CodexIdentity {
            email: None,
            account_id: None,
            plan_type: None,
        };
    };

    let account_id = claims
        .auth_info
        .as_ref()
        .and_then(|info| {
            info.chatgpt_account_id
                .clone()
                .or_else(|| info.user_id.clone())
        })
        .or(claims.sub);
    let plan_type = claims
        .auth_info
        .as_ref()
        .and_then(|info| info.chatgpt_plan_type.clone());

    CodexIdentity {
        email: claims.email,
        account_id,
        plan_type,
    }
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

    let client_id = get_client_id();
    let params = [
        ("client_id", client_id.as_str()),
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
    exchange_code_with_redirect(code, code_verifier, REDIRECT_URI).await
}

async fn exchange_code_with_redirect(
    code: &str,
    code_verifier: &str,
    redirect_uri: &str,
) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let client_id = get_client_id();
    let params = [
        ("grant_type", "authorization_code"),
        ("client_id", client_id.as_str()),
        ("code", code),
        ("redirect_uri", redirect_uri),
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

/// Refresh access token using refresh_token
pub async fn refresh_token(refresh_token: &str) -> Result<TokenResponse> {
    let client = reqwest::Client::new();

    let client_id = get_client_id();
    let params = [
        ("grant_type", "refresh_token"),
        ("client_id", client_id.as_str()),
        ("refresh_token", refresh_token),
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
            "Token refresh failed with status {}: {}",
            status,
            body
        ));
    }

    let text = response.text().await?;
    let token_response: TokenResponse = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Failed to parse token response: {} - body: {}", e, text))?;
    Ok(token_response)
}

/// Extract email from token response
pub fn extract_email(token_response: &TokenResponse) -> Option<String> {
    extract_codex_identity_from_id_token(token_response.id_token.as_deref()).email
}

pub fn extract_codex_identity(token_response: &TokenResponse) -> CodexIdentity {
    extract_codex_identity_from_id_token(token_response.id_token.as_deref())
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
    pub account_id: Option<String>,
    pub plan_type: Option<String>,
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

fn open_system_browser(url: &str) {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(target_os = "windows")]
    {
        let _ = std::process::Command::new("cmd")
            .args(["/c", "start", "", url])
            .spawn();
    }
    #[cfg(target_os = "linux")]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
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
    let client_id = get_client_id();
    let params = [
        ("client_id", client_id.as_str()),
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
        async move { handle_callback(params, state).await }
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
    open_system_browser(&auth_url);

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

pub async fn start_device_oauth() -> Result<DeviceOAuthStart> {
    let user_code = request_device_user_code().await?;
    let session_id = Uuid::new_v4().to_string();
    let interval = parse_device_poll_interval(user_code.interval.as_ref());
    let device_auth_id = user_code.device_auth_id.trim().to_string();
    let code = user_code
        .user_code
        .clone()
        .or(user_code.usercode.clone())
        .unwrap_or_default()
        .trim()
        .to_string();

    if device_auth_id.is_empty() || code.is_empty() {
        return Err(anyhow::anyhow!(
            "codex device flow did not return required fields"
        ));
    }

    PENDING_DEVICE_OAUTH.write().insert(
        session_id.clone(),
        PendingDeviceFlow {
            device_auth_id,
            user_code: code.clone(),
            poll_interval: interval,
        },
    );

    open_system_browser(CODEX_DEVICE_VERIFICATION_URL);

    Ok(DeviceOAuthStart {
        session_id,
        verification_url: CODEX_DEVICE_VERIFICATION_URL.to_string(),
        user_code: code,
        interval_seconds: interval.as_secs(),
    })
}

pub async fn finish_device_oauth(session_id: &str) -> Result<OAuthResult> {
    let pending = PENDING_DEVICE_OAUTH
        .read()
        .get(session_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("Invalid or expired device login session"))?;

    let token = poll_device_token(
        &pending.device_auth_id,
        &pending.user_code,
        pending.poll_interval,
    )
    .await?;

    let token_response = exchange_code_with_redirect(
        &token.authorization_code,
        &token.code_verifier,
        CODEX_DEVICE_TOKEN_EXCHANGE_REDIRECT_URI,
    )
    .await?;

    let identity = extract_codex_identity(&token_response);
    PENDING_DEVICE_OAUTH.write().remove(session_id);

    Ok(OAuthResult {
        token_response,
        email: identity.email,
        account_id: identity.account_id,
        plan_type: identity.plan_type,
    })
}

async fn request_device_user_code() -> Result<DeviceUserCodeResponse> {
    let client = reqwest::Client::new();
    let response = client
        .post(CODEX_DEVICE_USER_CODE_URL)
        .header("Content-Type", "application/json")
        .header("Accept", "application/json")
        .json(&DeviceUserCodeRequest {
            client_id: get_client_id(),
        })
        .send()
        .await?;

    let status = response.status();
    let text = response.text().await?;
    if !status.is_success() {
        return Err(anyhow::anyhow!(
            "codex device code request failed with status {}: {}",
            status,
            text
        ));
    }

    serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("failed to decode codex device code response: {}", e))
}

async fn poll_device_token(
    device_auth_id: &str,
    user_code: &str,
    interval: Duration,
) -> Result<DeviceTokenResponse> {
    let client = reqwest::Client::new();
    let deadline = tokio::time::Instant::now() + CODEX_DEVICE_TIMEOUT;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow::anyhow!(
                "codex device authentication timed out after 15 minutes"
            ));
        }

        let response = client
            .post(CODEX_DEVICE_TOKEN_URL)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .json(&DeviceTokenRequest {
                device_auth_id: device_auth_id.to_string(),
                user_code: user_code.to_string(),
            })
            .send()
            .await?;

        let status = response.status();
        let text = response.text().await?;
        if status.is_success() {
            return serde_json::from_str(&text).map_err(|e| {
                anyhow::anyhow!("failed to decode codex device token response: {}", e)
            });
        }

        if status == reqwest::StatusCode::FORBIDDEN || status == reqwest::StatusCode::NOT_FOUND {
            tokio::time::sleep(interval).await;
            continue;
        }

        return Err(anyhow::anyhow!(
            "codex device token polling failed with status {}: {}",
            status,
            text
        ));
    }
}

fn parse_device_poll_interval(interval: Option<&ValueOrStringNumber>) -> Duration {
    match interval {
        Some(ValueOrStringNumber::Number(seconds)) if *seconds > 0 => Duration::from_secs(*seconds),
        Some(ValueOrStringNumber::String(value)) => value
            .trim()
            .parse::<u64>()
            .ok()
            .filter(|seconds| *seconds > 0)
            .map(Duration::from_secs)
            .unwrap_or(CODEX_DEVICE_DEFAULT_POLL_INTERVAL),
        _ => CODEX_DEVICE_DEFAULT_POLL_INTERVAL,
    }
}

// Codex Quota API
const CODEX_USAGE_URL: &str = "https://chatgpt.com/backend-api/wham/usage";
const CODEX_USER_AGENT: &str = "codex-cli/1.0.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodexQuotaData {
    pub plan_type: String,
    pub primary_used: f64,
    pub primary_resets_at: Option<String>,
    pub secondary_used: f64,
    pub secondary_resets_at: Option<String>,
    pub has_credits: bool,
    pub unlimited_credits: bool,
    pub credits_balance: Option<f64>,
    pub last_updated: i64,
    #[serde(default)]
    pub is_error: bool,
    #[serde(default)]
    pub error_message: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CodexUsageResponse {
    plan_type: Option<String>,
    rate_limit: Option<RateLimitInfo>,
    credits: Option<CreditsInfo>,
}

#[derive(Debug, Deserialize)]
struct RateLimitInfo {
    primary_window: Option<WindowInfo>,
    secondary_window: Option<WindowInfo>,
}

#[derive(Debug, Deserialize)]
struct WindowInfo {
    #[serde(deserialize_with = "deserialize_f64_or_string", default)]
    used_percent: Option<f64>,
    reset_at: Option<i64>,
    limit_window_seconds: Option<i64>,
}

/// Deserialize f64 from either a number or a string
fn deserialize_f64_or_string<'de, D>(deserializer: D) -> Result<Option<f64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct F64OrStringVisitor;

    impl<'de> Visitor<'de> for F64OrStringVisitor {
        type Value = Option<f64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a number or a string containing a number")
        }

        fn visit_f64<E>(self, v: f64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(v))
        }

        fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(v as f64))
        }

        fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(v as f64))
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            v.parse::<f64>().map(Some).map_err(de::Error::custom)
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(F64OrStringVisitor)
}

#[derive(Debug, Deserialize)]
struct CreditsInfo {
    has_credits: Option<bool>,
    unlimited: Option<bool>,
    balance: Option<f64>,
}

/// Fetch Codex usage/quota data
pub async fn fetch_codex_quota(
    access_token: &str,
    account_id: Option<&str>,
) -> Result<CodexQuotaData> {
    let client = reqwest::Client::new();

    let mut request = client
        .get(CODEX_USAGE_URL)
        .header("User-Agent", CODEX_USER_AGENT)
        .bearer_auth(access_token);

    if let Some(aid) = account_id {
        request = request.header("chatgpt-account-id", aid);
    }

    let response = request.send().await?;

    if !response.status().is_success() {
        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        tracing::warn!("Codex quota fetch failed: {} {}", status, text);
        return Ok(CodexQuotaData {
            plan_type: "unknown".to_string(),
            primary_used: 0.0,
            primary_resets_at: None,
            secondary_used: 0.0,
            secondary_resets_at: None,
            has_credits: false,
            unlimited_credits: false,
            credits_balance: None,
            last_updated: chrono::Utc::now().timestamp(),
            is_error: true,
            error_message: Some(format!("API Error: {} {}", status, text)),
        });
    }

    let text = response.text().await?;

    // Parse as generic JSON first (like JS does), then extract values flexibly
    let data: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| anyhow::anyhow!("Failed to parse quota response: {} - body: {}", e, text))?;

    let rate_limit = data.get("rate_limit");
    let primary = rate_limit.and_then(|r| r.get("primary_window"));
    let secondary = rate_limit.and_then(|r| r.get("secondary_window"));
    let credits = data.get("credits");

    // Helper to extract f64 from either number or string
    let get_f64 = |v: Option<&serde_json::Value>| -> f64 {
        v.and_then(|val| {
            val.as_f64()
                .or_else(|| val.as_i64().map(|i| i as f64))
                .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
        })
        .unwrap_or(0.0)
    };

    let get_i64 = |v: Option<&serde_json::Value>| -> Option<i64> {
        v.and_then(|val| {
            val.as_i64()
                .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
        })
    };

    let get_bool = |v: Option<&serde_json::Value>| -> bool {
        v.and_then(|val| val.as_bool()).unwrap_or(false)
    };

    Ok(CodexQuotaData {
        plan_type: data
            .get("plan_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown")
            .to_string(),
        primary_used: get_f64(primary.and_then(|p| p.get("used_percent"))),
        primary_resets_at: get_i64(primary.and_then(|p| p.get("reset_at"))).map(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }),
        secondary_used: get_f64(secondary.and_then(|s| s.get("used_percent"))),
        secondary_resets_at: get_i64(secondary.and_then(|s| s.get("reset_at"))).map(|ts| {
            chrono::DateTime::from_timestamp(ts, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_default()
        }),
        has_credits: get_bool(credits.and_then(|c| c.get("has_credits"))),
        unlimited_credits: get_bool(credits.and_then(|c| c.get("unlimited"))),
        credits_balance: credits
            .and_then(|c| c.get("balance"))
            .and_then(|v| v.as_f64()),
        last_updated: chrono::Utc::now().timestamp(),
        is_error: false,
        error_message: None,
    })
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
            let identity = extract_codex_identity(&token_response);
            let result = OAuthResult {
                token_response,
                email: identity.email,
                account_id: identity.account_id,
                plan_type: identity.plan_type,
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
