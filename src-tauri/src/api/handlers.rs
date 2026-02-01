// API request handlers

use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Json},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::claude::{self, ClaudeClient, ClaudeRequest};
use super::gemini::{self, GeminiClient};
use super::AppState;
use crate::auth::{self, providers::{google, anthropic, antigravity, openai}, AuthFile, TokenInfo};

#[derive(Debug, Clone)]
struct GeminiAuth {
    access_token: String,
    project_id: Option<String>,
}

// Root endpoint
pub async fn root() -> Json<Value> {
    Json(json!({
        "message": "CLI Proxy API Server (Tauri)",
        "endpoints": [
            "POST /v1/chat/completions",
            "POST /v1/completions",
            "GET /v1/models",
            "POST /v1/messages",
            "GET /v1beta/models",
            "POST /v1beta/models/*action"
        ]
    }))
}

// OpenAI compatible endpoints
#[derive(Debug, Serialize)]
pub struct ModelInfo {
    pub id: String,
    pub object: String,
    pub created: i64,
    pub owned_by: String,
}

#[derive(Debug, Serialize)]
pub struct ModelsResponse {
    pub object: String,
    pub data: Vec<ModelInfo>,
}

pub async fn openai_models(State(_state): State<AppState>) -> Json<ModelsResponse> {
    let mut models = Vec::new();
    let mut has_gemini = false;
    let mut has_codex = false;
    let mut has_antigravity = false;
    let mut has_claude = false;

    // Check which providers have valid auth files
    let auth_dir = crate::config::resolve_auth_dir();
    if auth_dir.exists() {
        if let Ok(entries) = std::fs::read_dir(&auth_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(auth_file) = serde_json::from_str::<AuthFile>(&content) {
                            if auth_file.enabled {
                                match auth_file.provider.as_str() {
                                    "gemini" => has_gemini = true,
                                    "codex" => has_codex = true,
                                    "antigravity" => has_antigravity = true,
                                    "claude" => has_claude = true,
                                    _ => {}
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Add Gemini models if available
    if has_gemini {
        models.extend(get_gemini_models());
    }

    // Add Codex/OpenAI models if available
    if has_codex {
        models.extend(get_codex_models());
    }

    // Add Antigravity models if available
    if has_antigravity {
        models.extend(get_antigravity_models());
    }

    // Add Claude models if available
    if has_claude {
        models.extend(get_claude_models());
    }

    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
}

/// Get static Gemini model definitions
fn get_gemini_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-2.5-pro".to_string(),
            object: "model".to_string(),
            created: 1750118400,
            owned_by: "google".to_string(),
        },
        ModelInfo {
            id: "gemini-2.5-flash".to_string(),
            object: "model".to_string(),
            created: 1750118400,
            owned_by: "google".to_string(),
        },
        ModelInfo {
            id: "gemini-2.5-flash-lite".to_string(),
            object: "model".to_string(),
            created: 1753142400,
            owned_by: "google".to_string(),
        },
        ModelInfo {
            id: "gemini-3-pro-preview".to_string(),
            object: "model".to_string(),
            created: 1737158400,
            owned_by: "google".to_string(),
        },
        ModelInfo {
            id: "gemini-3-flash-preview".to_string(),
            object: "model".to_string(),
            created: 1765929600,
            owned_by: "google".to_string(),
        },
    ]
}

/// Get static Codex/OpenAI model definitions
fn get_codex_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gpt-5".to_string(),
            object: "model".to_string(),
            created: 1754524800,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5-codex".to_string(),
            object: "model".to_string(),
            created: 1757894400,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5-codex-mini".to_string(),
            object: "model".to_string(),
            created: 1762473600,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5.1".to_string(),
            object: "model".to_string(),
            created: 1762905600,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5.1-codex".to_string(),
            object: "model".to_string(),
            created: 1762905600,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5.1-codex-mini".to_string(),
            object: "model".to_string(),
            created: 1762905600,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5.2".to_string(),
            object: "model".to_string(),
            created: 1765440000,
            owned_by: "openai".to_string(),
        },
        ModelInfo {
            id: "gpt-5.2-codex".to_string(),
            object: "model".to_string(),
            created: 1765440000,
            owned_by: "openai".to_string(),
        },
    ]
}

/// Get static Antigravity model definitions
fn get_antigravity_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "gemini-2.5-flash".to_string(),
            object: "model".to_string(),
            created: 1750118400,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "gemini-2.5-flash-lite".to_string(),
            object: "model".to_string(),
            created: 1753142400,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "gemini-3-pro-high".to_string(),
            object: "model".to_string(),
            created: 1737158400,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "gemini-3-flash".to_string(),
            object: "model".to_string(),
            created: 1765929600,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "claude-sonnet-4-5-thinking".to_string(),
            object: "model".to_string(),
            created: 1759104000,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "claude-opus-4-5-thinking".to_string(),
            object: "model".to_string(),
            created: 1761955200,
            owned_by: "antigravity".to_string(),
        },
        ModelInfo {
            id: "claude-sonnet-4-5".to_string(),
            object: "model".to_string(),
            created: 1759104000,
            owned_by: "antigravity".to_string(),
        },
    ]
}

/// Get static Claude model definitions
fn get_claude_models() -> Vec<ModelInfo> {
    vec![
        ModelInfo {
            id: "claude-haiku-4-5-20251001".to_string(),
            object: "model".to_string(),
            created: 1759276800,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-sonnet-4-5-20250929".to_string(),
            object: "model".to_string(),
            created: 1759104000,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-opus-4-5-20251101".to_string(),
            object: "model".to_string(),
            created: 1761955200,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-opus-4-20250514".to_string(),
            object: "model".to_string(),
            created: 1715644800,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-sonnet-4-20250514".to_string(),
            object: "model".to_string(),
            created: 1715644800,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-3-7-sonnet-20250219".to_string(),
            object: "model".to_string(),
            created: 1708300800,
            owned_by: "anthropic".to_string(),
        },
        ModelInfo {
            id: "claude-3-5-haiku-20241022".to_string(),
            object: "model".to_string(),
            created: 1729555200,
            owned_by: "anthropic".to_string(),
        },
    ]
}

#[derive(Debug, Deserialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(default)]
    pub stream: bool,
    #[serde(default)]
    pub temperature: Option<f32>,
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

/// Get a valid Gemini access token from stored credentials
/// Supports CLIProxyAPI format (gemini-*.json)
async fn get_gemini_auth() -> Option<GeminiAuth> {
    let auth_dir = crate::config::resolve_auth_dir();
    if !auth_dir.exists() {
        return None;
    }

    // Look for gemini auth files
    if let Ok(entries) = std::fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                // Match gemini-*.json (CLIProxyAPI format)
                let is_gemini_file = name.starts_with("gemini-") && name.ends_with(".json");

                if is_gemini_file {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&content) {
                            // Check if enabled (default true)
                            let enabled = json.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);
                            if !enabled {
                                continue;
                            }

                            // Extract token info from token object
                            if let Some(token_obj) = json.get("token") {
                                let access_token = token_obj.get("access_token").and_then(|v| v.as_str()).unwrap_or("");
                                let refresh_token = token_obj.get("refresh_token").and_then(|v| v.as_str());
                                let expiry = token_obj.get("expiry").and_then(|v| v.as_str());
                                let project_id = json
                                    .get("project_id")
                                    .and_then(|v| v.as_str())
                                    .map(|v| v.trim().to_string())
                                    .and_then(|v| {
                                        if v.is_empty() {
                                            return None;
                                        }
                                        if v.eq_ignore_ascii_case("all") {
                                            return None;
                                        }
                                        if let Some(first) = v.split(',').map(|s| s.trim()).find(|s| !s.is_empty()) {
                                            return Some(first.to_string());
                                        }
                                        None
                                    });

                                if access_token.is_empty() {
                                    continue;
                                }

                                // Check if token is expired
                                let is_expired = if let Some(expiry_str) = expiry {
                                    if let Ok(expiry_time) = chrono::DateTime::parse_from_rfc3339(expiry_str) {
                                        expiry_time < chrono::Utc::now()
                                    } else {
                                        false // Can't parse, assume not expired
                                    }
                                } else {
                                    false // No expiry, assume not expired
                                };

                                if !is_expired {
                                    return Some(GeminiAuth {
                                        access_token: access_token.to_string(),
                                        project_id,
                                    });
                                }

                                // Token expired, try to refresh
                                if let Some(refresh_tok) = refresh_token {
                                    if let Ok(new_tokens) = google::refresh_token(refresh_tok).await {
                                        // Update the token in the JSON
                                        let new_expiry = new_tokens.expires_in.map(|secs| {
                                            (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
                                        });

                                        if let Some(token_obj) = json.get_mut("token") {
                                            if let Some(obj) = token_obj.as_object_mut() {
                                                obj.insert("access_token".to_string(), serde_json::json!(new_tokens.access_token));
                                                if let Some(new_refresh) = &new_tokens.refresh_token {
                                                    obj.insert("refresh_token".to_string(), serde_json::json!(new_refresh));
                                                }
                                                if let Some(exp) = new_expiry {
                                                    obj.insert("expiry".to_string(), serde_json::json!(exp));
                                                }
                                            }
                                        }

                                        // Save updated file
                                        if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                                            let _ = std::fs::write(&path, updated_content);
                                        }

                                        return Some(GeminiAuth {
                                            access_token: new_tokens.access_token,
                                            project_id,
                                        });
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Get a valid Claude access token from stored credentials
async fn get_claude_token() -> Option<String> {
    let auth_dir = crate::config::resolve_auth_dir();
    if !auth_dir.exists() {
        return None;
    }

    // Look for claude auth files
    if let Ok(entries) = std::fs::read_dir(&auth_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                if name.starts_with("claude_") && name.ends_with(".json") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if let Ok(auth_file) = serde_json::from_str::<AuthFile>(&content) {
                            if auth_file.enabled {
                                // Check if token is expired
                                if let Some(expires_at) = auth_file.token.expires_at {
                                    if expires_at > chrono::Utc::now() {
                                        return Some(auth_file.token.access_token);
                                    }
                                    // Token expired, try to refresh
                                    if let Some(refresh_token) = auth_file.token.refresh_token {
                                        if let Ok(new_tokens) = anthropic::refresh_token(&refresh_token).await {
                                            // Update the auth file with new tokens
                                            let new_expires_at = new_tokens.expires_in.map(|secs| {
                                                chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
                                            });
                                            let updated_auth = AuthFile {
                                                token: TokenInfo {
                                                    access_token: new_tokens.access_token.clone(),
                                                    refresh_token: new_tokens.refresh_token.or(Some(refresh_token)),
                                                    expires_at: new_expires_at,
                                                    token_type: new_tokens.token_type,
                                                },
                                                ..auth_file
                                            };
                                            let _ = auth::save_auth_file(&updated_auth, &path);
                                            return Some(new_tokens.access_token);
                                        }
                                    }
                                } else {
                                    // No expiry set, assume valid
                                    return Some(auth_file.token.access_token);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

pub async fn chat_completions(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> impl IntoResponse {
    let request_id = uuid::Uuid::new_v4().to_string();
    let model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();

    // Check if this is a Gemini model
    if model.starts_with("gemini") {
        // Get Gemini token
        let auth = match get_gemini_auth().await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Gemini credentials found. Please login with Google first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }));
            }
        };

        let client = GeminiClient::new(auth.access_token);

        let mut gemini_request = gemini::openai_to_gemini_cli_request(&raw, &model);
        if let Some(project_id) = auth.project_id {
            gemini_request["project"] = json!(project_id);
        }

        match client.generate_content(&gemini_request).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                return Json(openai_response);
            }
            Err(e) => {
                tracing::error!("Gemini API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }));
            }
        }
    }

    // Check if this is a Claude model
    let request: ChatCompletionRequest = match serde_json::from_value(raw.clone()) {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({
                "error": {
                    "message": format!("Invalid request: {}", e),
                    "type": "invalid_request_error",
                    "code": 400
                }
            }));
        }
    };

    if request.model.starts_with("claude") {
        // Get Claude token
        let token = match get_claude_token().await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Claude credentials found. Please login with Anthropic first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }));
            }
        };

        let client = ClaudeClient::new(token);

        // Convert messages to Claude format
        let (messages, system) = claude::openai_to_claude_messages(&request.messages);

        let claude_request = ClaudeRequest {
            model: request.model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            system,
        };

        match client.create_message(claude_request).await {
            Ok(response) => {
                let openai_response = claude::claude_to_openai_response(
                    &response,
                    &request.model,
                    &request_id,
                );
                return Json(openai_response);
            }
            Err(e) => {
                tracing::error!("Claude API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Claude API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }));
            }
        }
    }

    // Default placeholder response for unsupported models
    Json(json!({
        "error": {
            "message": format!("Model '{}' is not supported. Please use a Gemini or Claude model and add appropriate credentials.", request.model),
            "type": "invalid_request_error",
            "code": 400
        }
    }))
}

pub async fn completions(
    State(_state): State<AppState>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement completions endpoint
    Json(json!({
        "error": "Completions endpoint not yet implemented"
    }))
}

// Claude compatible endpoint
pub async fn claude_messages(
    State(_state): State<AppState>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement Claude messages endpoint
    Json(json!({
        "error": "Claude messages endpoint not yet implemented"
    }))
}

// Gemini compatible endpoints
pub async fn gemini_models(State(_state): State<AppState>) -> Json<Value> {
    // TODO: Fetch actual Gemini models
    Json(json!({
        "models": [
            {
                "name": "models/gemini-2.5-pro",
                "displayName": "Gemini 2.5 Pro",
                "description": "Gemini 2.5 Pro model"
            },
            {
                "name": "models/gemini-2.5-flash",
                "displayName": "Gemini 2.5 Flash",
                "description": "Gemini 2.5 Flash model"
            }
        ]
    }))
}

pub async fn gemini_handler(
    State(_state): State<AppState>,
    Path(action): Path<String>,
    Json(_request): Json<Value>,
) -> impl IntoResponse {
    // TODO: Implement Gemini API handler
    Json(json!({
        "error": format!("Gemini action '{}' not yet implemented", action)
    }))
}

// OAuth callback handlers
const OAUTH_SUCCESS_HTML: &str = r#"
<!DOCTYPE html>
<html>
<head>
    <meta charset="utf-8">
    <title>Authentication Successful</title>
    <script>setTimeout(function(){window.close();}, 5000);</script>
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
        <p><small>This window will close automatically in 5 seconds.</small></p>
    </div>
</body>
</html>
"#;

const OAUTH_ERROR_HTML: &str = r#"
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

#[derive(Debug, Deserialize)]
pub struct OAuthCallback {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

pub async fn google_callback(
    State(_state): State<AppState>,
    Query(params): Query<OAuthCallback>,
) -> impl IntoResponse {
    // Check for errors first
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Google OAuth error: {}", error_msg);
        return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No authorization code received"));
        }
    };

    let state = match params.state {
        Some(s) => s,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No state parameter received"));
        }
    };

    tracing::info!("Received Google OAuth callback with code and state");

    // Exchange code for tokens
    match google::exchange_code(&code, &state).await {
        Ok(token_response) => {
            tracing::info!("Successfully exchanged code for tokens");

            // Get user info using v1 API like CLIProxyAPI
            let email = match google::get_user_info(&token_response.access_token).await {
                Ok(user_info) => user_info.email.unwrap_or_else(|| "default".to_string()),
                Err(e) => {
                    tracing::warn!("Failed to get user info: {}", e);
                    "default".to_string()
                }
            };

            // Save in CLIProxyAPI-compatible format (GeminiAuthFile)
            let token = serde_json::json!({
                "access_token": token_response.access_token,
                "refresh_token": token_response.refresh_token,
                "token_type": token_response.token_type,
                "expires_in": token_response.expires_in,
                "expiry": token_response.expires_in.map(|secs| {
                    (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
                }),
                "client_id": google::GOOGLE_CLIENT_ID,
                "client_secret": google::GOOGLE_CLIENT_SECRET,
                "scopes": google::SCOPES,
                "token_uri": "https://oauth2.googleapis.com/token",
                "universe_domain": "googleapis.com"
            });

            let gemini_auth = auth::GeminiAuthFile {
                token,
                project_id: "".to_string(),
                email: email.clone(),
                auto: true,
                checked: false,
                auth_type: "gemini".to_string(),
            };

            // Save with CLIProxyAPI naming convention: gemini-email-all.json
            let auth_dir = crate::config::resolve_auth_dir();
            let path = auth_dir.join(format!("gemini-{}-all.json", email));

            let content = match serde_json::to_string_pretty(&gemini_auth) {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("Failed to serialize auth file: {}", e);
                    return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
                }
            };

            if let Some(parent) = path.parent() {
                if let Err(e) = std::fs::create_dir_all(parent) {
                    tracing::error!("Failed to create auth dir: {}", e);
                    return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
                }
            }

            if let Err(e) = std::fs::write(&path, content) {
                tracing::error!("Failed to save auth file: {}", e);
                return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
            }

            tracing::info!("Saved Google auth file to {:?}", path);
            Html(OAUTH_SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Token exchange failed: {}", e)))
        }
    }
}

pub async fn anthropic_callback(
    State(_state): State<AppState>,
    Query(params): Query<OAuthCallback>,
) -> impl IntoResponse {
    // Check for errors first
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Anthropic OAuth error: {}", error_msg);
        return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No authorization code received"));
        }
    };

    let state = match params.state {
        Some(s) => s,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No state parameter received"));
        }
    };

    tracing::info!("Received Anthropic OAuth callback with code and state");

    // Exchange code for tokens
    match anthropic::exchange_code(&code, &state).await {
        Ok(token_response) => {
            tracing::info!("Successfully exchanged code for Anthropic tokens");

            // Get email from account info
            let email = token_response
                .account
                .as_ref()
                .and_then(|a| a.email_address.clone());

            // Calculate expiry time
            let expires_at = token_response.expires_in.map(|secs| {
                chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
            });

            // Create auth file
            let auth_file = AuthFile {
                provider: "claude".to_string(),
                email: email.clone(),
                token: TokenInfo {
                    access_token: token_response.access_token,
                    refresh_token: token_response.refresh_token,
                    expires_at,
                    token_type: token_response.token_type,
                },
                enabled: true,
                prefix: None,
            };

            // Save auth file
            let identifier = email.as_deref().unwrap_or("default");
            let path = auth::get_auth_file_path("claude", identifier);

            if let Err(e) = auth::save_auth_file(&auth_file, &path) {
                tracing::error!("Failed to save auth file: {}", e);
                return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
            }

            tracing::info!("Saved Anthropic auth file to {:?}", path);
            Html(OAUTH_SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Token exchange failed: {}", e)))
        }
    }
}

pub async fn codex_callback(
    State(_state): State<AppState>,
    Query(params): Query<OAuthCallback>,
) -> impl IntoResponse {
    // Check for errors first
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Codex OAuth error: {}", error_msg);
        return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No authorization code received"));
        }
    };

    let state = match params.state {
        Some(s) => s,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No state parameter received"));
        }
    };

    tracing::info!("Received Codex OAuth callback with code and state");

    // Get the PKCE verifier for this state
    let code_verifier = match openai::get_pkce_verifier(&state) {
        Some(v) => v,
        None => {
            tracing::error!("No PKCE verifier found for state: {}", state);
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "Invalid or expired OAuth session. Please try again."));
        }
    };

    // Exchange code for tokens
    match openai::exchange_code(&code, &code_verifier).await {
        Ok(token_response) => {
            tracing::info!("Successfully exchanged code for Codex tokens");

            // Extract email from ID token
            let email = openai::extract_email(&token_response);

            // Calculate expiry time
            let expires_at = token_response.expires_in.map(|secs| {
                chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
            });

            // Create auth file
            let auth_file = AuthFile {
                provider: "codex".to_string(),
                email: email.clone(),
                token: TokenInfo {
                    access_token: token_response.access_token,
                    refresh_token: token_response.refresh_token,
                    expires_at,
                    token_type: token_response.token_type,
                },
                enabled: true,
                prefix: None,
            };

            // Save auth file
            let identifier = email.as_deref().unwrap_or("default");
            let path = auth::get_auth_file_path("codex", identifier);

            if let Err(e) = auth::save_auth_file(&auth_file, &path) {
                tracing::error!("Failed to save auth file: {}", e);
                return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
            }

            tracing::info!("Saved Codex auth file to {:?}", path);
            Html(OAUTH_SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Token exchange failed: {}", e)))
        }
    }
}

pub async fn antigravity_callback(
    State(_state): State<AppState>,
    Query(params): Query<OAuthCallback>,
) -> impl IntoResponse {
    // Check for errors first
    if let Some(error) = params.error {
        let error_msg = params.error_description.unwrap_or(error);
        tracing::error!("Antigravity OAuth error: {}", error_msg);
        return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &error_msg));
    }

    let code = match params.code {
        Some(c) => c,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No authorization code received"));
        }
    };

    let state = match params.state {
        Some(s) => s,
        None => {
            return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", "No state parameter received"));
        }
    };

    tracing::info!("Received Antigravity OAuth callback with code and state");

    // Exchange code for tokens
    match antigravity::exchange_code(&code, &state).await {
        Ok(token_response) => {
            tracing::info!("Successfully exchanged code for Antigravity tokens");

            // Get user info
            let email = match antigravity::get_user_info(&token_response.access_token).await {
                Ok(user_info) => user_info.email,
                Err(e) => {
                    tracing::warn!("Failed to get user info: {}", e);
                    None
                }
            };

            // Calculate expiry time
            let expires_at = token_response.expires_in.map(|secs| {
                chrono::Utc::now() + chrono::Duration::seconds(secs as i64)
            });

            // Create auth file
            let auth_file = AuthFile {
                provider: "antigravity".to_string(),
                email: email.clone(),
                token: TokenInfo {
                    access_token: token_response.access_token,
                    refresh_token: token_response.refresh_token,
                    expires_at,
                    token_type: token_response.token_type,
                },
                enabled: true,
                prefix: None,
            };

            // Save auth file
            let identifier = email.as_deref().unwrap_or("default");
            let path = auth::get_auth_file_path("antigravity", identifier);

            if let Err(e) = auth::save_auth_file(&auth_file, &path) {
                tracing::error!("Failed to save auth file: {}", e);
                return Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Failed to save credentials: {}", e)));
            }

            tracing::info!("Saved Antigravity auth file to {:?}", path);
            Html(OAUTH_SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Token exchange failed: {}", e)))
        }
    }
}
