// API request handlers

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderValue, StatusCode},
    response::{sse::Event, Html, IntoResponse, Json, Response, Sse},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::antigravity::{self, AntigravityClient};
use super::claude::{self, ClaudeClient, ClaudeRequest};
use super::codex::{self, CodexClient};
use super::gemini::{self, GeminiClient};
use super::AppState;
use crate::auth::{self, providers::{google, anthropic, antigravity as antigravity_oauth, openai}, AuthFile, TokenInfo};
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::convert::Infallible;
use std::path::PathBuf;
use std::sync::Mutex;
use futures::{StreamExt, TryStreamExt};
use flate2::read::GzDecoder;
use std::io::Read;

#[derive(Debug, Clone)]
struct GeminiAuth {
    access_token: String,
    project_id: Option<String>,
}

#[derive(Debug, Clone)]
struct AntigravityAuth {
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
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            let provider = json
                                .get("provider")
                                .or_else(|| json.get("type"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .trim()
                                .to_lowercase();
                            if provider.is_empty() {
                                continue;
                            }
                            let disabled = json.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
                            let enabled = json.get("enabled").and_then(|v| v.as_bool()).unwrap_or(!disabled);
                            if disabled || !enabled {
                                continue;
                            }
                            match provider.as_str() {
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

    // Add Gemini models if available
    if has_gemini {
        let base = get_gemini_models();
        models.extend(build_prefixed_models("gemini", &base));
    }

    // Add Codex/OpenAI models if available
    if has_codex {
        let base = get_codex_models();
        models.extend(build_prefixed_models("codex", &base));
    }

    // Add Antigravity models if available
    if has_antigravity {
        let base = get_antigravity_models();
        models.extend(build_prefixed_models("antigravity", &base));
    }

    // Add Claude models if available
    if has_claude {
        let base = get_claude_models();
        models.extend(build_prefixed_models("claude", &base));
    }

    Json(ModelsResponse {
        object: "list".to_string(),
        data: models,
    })
}

fn build_prefixed_models(prefix: &str, base: &[ModelInfo]) -> Vec<ModelInfo> {
    base.iter()
        .map(|m| ModelInfo {
            id: format!("{}/{}", prefix, m.id),
            object: m.object.clone(),
            created: m.created,
            owned_by: m.owned_by.clone(),
        })
        .collect()
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

fn parse_provider_prefix(model: &str) -> (Option<String>, String) {
    let trimmed = model.trim();
    if let Some((prefix, rest)) = trimmed.split_once('/') {
        if let Some(normalized) = normalize_provider_prefix(prefix) {
            return (Some(normalized), rest.to_string());
        }
    }
    if let Some((prefix, rest)) = trimmed.split_once(':') {
        if let Some(normalized) = normalize_provider_prefix(prefix) {
            return (Some(normalized), rest.to_string());
        }
    }
    (None, trimmed.to_string())
}

fn normalize_provider_prefix(prefix: &str) -> Option<String> {
    match prefix.trim().to_lowercase().as_str() {
        "gemini" => Some("gemini".to_string()),
        "codex" => Some("codex".to_string()),
        "openai" => Some("codex".to_string()),
        "claude" => Some("claude".to_string()),
        "antigravity" => Some("antigravity".to_string()),
        _ => None,
    }
}

fn completions_prompt_text(raw: &Value) -> String {
    if let Some(prompt) = raw.get("prompt") {
        if let Some(text) = prompt.as_str() {
            if !text.is_empty() {
                return text.to_string();
            }
        }
        if let Some(arr) = prompt.as_array() {
            let mut combined = String::new();
            for item in arr {
                if let Some(text) = item.as_str() {
                    combined.push_str(text);
                }
            }
            if !combined.is_empty() {
                return combined;
            }
        }
    }
    "Complete this:".to_string()
}

fn convert_completions_request_to_chat(raw: &Value) -> Value {
    let prompt = completions_prompt_text(raw);
    let mut out = json!({
        "model": "",
        "messages": [{
            "role": "user",
            "content": ""
        }]
    });

    if let Some(model) = raw.get("model").and_then(|v| v.as_str()) {
        out["model"] = json!(model);
    }
    out["messages"][0]["content"] = json!(prompt);

    for (key, dest_key) in [
        ("max_tokens", "max_tokens"),
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("frequency_penalty", "frequency_penalty"),
        ("presence_penalty", "presence_penalty"),
        ("stop", "stop"),
        ("stream", "stream"),
        ("logprobs", "logprobs"),
        ("top_logprobs", "top_logprobs"),
        ("echo", "echo"),
    ] {
        if let Some(value) = raw.get(key) {
            out[dest_key] = value.clone();
        }
    }

    out
}

fn convert_chat_response_to_completions(raw: &Value) -> Value {
    let mut out = json!({
        "id": "",
        "object": "text_completion",
        "created": 0,
        "model": "",
        "choices": []
    });

    if let Some(id) = raw.get("id") {
        out["id"] = id.clone();
    }
    if let Some(created) = raw.get("created") {
        out["created"] = created.clone();
    }
    if let Some(model) = raw.get("model") {
        out["model"] = model.clone();
    }
    if let Some(usage) = raw.get("usage") {
        out["usage"] = usage.clone();
    }

    if let Some(choices) = raw.get("choices").and_then(|v| v.as_array()) {
        let mut converted = Vec::with_capacity(choices.len());
        for choice in choices {
            let mut item = serde_json::Map::new();
            if let Some(index) = choice.get("index") {
                item.insert("index".to_string(), index.clone());
            }
            if let Some(message) = choice.get("message") {
                if let Some(content) = message.get("content") {
                    item.insert("text".to_string(), content.clone());
                }
            } else if let Some(delta) = choice.get("delta") {
                if let Some(content) = delta.get("content") {
                    item.insert("text".to_string(), content.clone());
                }
            }
            if let Some(finish) = choice.get("finish_reason") {
                item.insert("finish_reason".to_string(), finish.clone());
            }
            if let Some(logprobs) = choice.get("logprobs") {
                item.insert("logprobs".to_string(), logprobs.clone());
            }
            converted.push(Value::Object(item));
        }
        out["choices"] = Value::Array(converted);
    }

    out
}

fn convert_chat_stream_chunk_to_completions(chunk: &str) -> Option<String> {
    let raw: Value = serde_json::from_str(chunk).ok()?;
    let choices = raw.get("choices")?.as_array()?;

    let mut has_content = false;
    for choice in choices {
        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    has_content = true;
                    break;
                }
            }
        }
        if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if !finish.is_empty() && finish != "null" {
                has_content = true;
                break;
            }
        }
    }

    if !has_content {
        return None;
    }

    let converted = convert_chat_response_to_completions(&raw);
    serde_json::to_string(&converted).ok()
}

fn maybe_decompress_gzip(bytes: &[u8]) -> Vec<u8> {
    if bytes.len() >= 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        let mut decoder = GzDecoder::new(bytes);
        let mut out = Vec::new();
        if decoder.read_to_end(&mut out).is_ok() {
            return out;
        }
    }
    bytes.to_vec()
}

#[derive(Default)]
struct ClaudeStreamState {
    message_id: String,
    model: String,
    message_started: bool,
    content_started: bool,
    finish_reason: Option<String>,
}

fn map_openai_finish_reason(reason: Option<&str>) -> Option<&'static str> {
    match reason {
        Some("stop") => Some("end_turn"),
        Some("length") => Some("max_tokens"),
        Some("tool_calls") => Some("tool_use"),
        Some("content_filter") => Some("stop_sequence"),
        _ => None,
    }
}

fn build_claude_event(event: &str, payload: Value) -> Event {
    Event::default().event(event).data(payload.to_string())
}

fn openai_chunk_to_claude_events(chunk: &str, state: &mut ClaudeStreamState) -> Vec<Event> {
    let mut events = Vec::new();
    let parsed: Value = match serde_json::from_str(chunk) {
        Ok(v) => v,
        Err(_) => return events,
    };

    if !state.message_started {
        if state.message_id.is_empty() {
            if let Some(id) = parsed.get("id").and_then(|v| v.as_str()) {
                if !id.is_empty() {
                    state.message_id = id.to_string();
                }
            }
            if state.message_id.is_empty() {
                state.message_id = format!("msg_{}", uuid::Uuid::new_v4());
            }
        }
        if state.model.is_empty() {
            if let Some(model) = parsed.get("model").and_then(|v| v.as_str()) {
                if !model.is_empty() {
                    state.model = model.to_string();
                }
            }
        }

        let payload = json!({
            "type": "message_start",
            "message": {
                "id": state.message_id,
                "type": "message",
                "role": "assistant",
                "model": state.model,
                "content": [],
                "stop_reason": null,
                "stop_sequence": null,
                "usage": {
                    "input_tokens": 0,
                    "output_tokens": 0
                }
            }
        });
        events.push(build_claude_event("message_start", payload));
        state.message_started = true;
    }

    if let Some(choice) = parsed
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
    {
        if let Some(finish) = choice.get("finish_reason").and_then(|v| v.as_str()) {
            if !finish.is_empty() {
                state.finish_reason = Some(finish.to_string());
            }
        }
        if let Some(delta) = choice.get("delta") {
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    if !state.content_started {
                        let payload = json!({
                            "type": "content_block_start",
                            "index": 0,
                            "content_block": {
                                "type": "text",
                                "text": ""
                            }
                        });
                        events.push(build_claude_event("content_block_start", payload));
                        state.content_started = true;
                    }
                    let payload = json!({
                        "type": "content_block_delta",
                        "index": 0,
                        "delta": {
                            "type": "text_delta",
                            "text": content
                        }
                    });
                    events.push(build_claude_event("content_block_delta", payload));
                }
            }
        }
    }

    events
}

fn finalize_claude_stream(state: &mut ClaudeStreamState) -> Vec<Event> {
    let mut events = Vec::new();
    if state.content_started {
        events.push(build_claude_event(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": 0
            }),
        ));
    }
    let stop_reason = map_openai_finish_reason(state.finish_reason.as_deref());
    events.push(build_claude_event(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            }
        }),
    ));
    events.push(build_claude_event(
        "message_stop",
        json!({ "type": "message_stop" }),
    ));
    events
}

fn openai_chunks_to_claude_events<S>(
    upstream: S,
    model_hint: &str,
) -> impl futures::Stream<Item = Result<Event, Infallible>>
where
    S: futures::Stream<Item = String>,
{
    let model_hint = model_hint.to_string();
    async_stream::stream! {
        let mut state = ClaudeStreamState {
            model: model_hint,
            ..ClaudeStreamState::default()
        };
        futures::pin_mut!(upstream);
        while let Some(chunk) = upstream.next().await {
            if chunk == "[DONE]" {
                for event in finalize_claude_stream(&mut state) {
                    yield Ok::<Event, Infallible>(event);
                }
                return;
            }
            for event in openai_chunk_to_claude_events(&chunk, &mut state) {
                yield Ok::<Event, Infallible>(event);
            }
        }
        for event in finalize_claude_stream(&mut state) {
            yield Ok::<Event, Infallible>(event);
        }
    }
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

#[derive(Clone)]
struct AuthCandidate {
    id: String,
    path: PathBuf,
    priority: i32,
}

static AUTH_SELECTOR: Lazy<Mutex<HashMap<String, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

fn collect_json_files(dir: &std::path::Path, out: &mut Vec<PathBuf>) {
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                continue;
            }
            if path.extension().map(|e| e == "json").unwrap_or(false) {
                out.push(path);
            }
        }
    }
}

fn parse_candidate_priority(json: &Value) -> i32 {
    match json.get("priority") {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0) as i32,
        Some(Value::String(s)) => s.trim().parse::<i32>().unwrap_or(0),
        _ => 0,
    }
}

fn candidate_from_path(
    provider: &str,
    auth_dir: &std::path::Path,
    path: &std::path::Path,
) -> Option<AuthCandidate> {
    let content = std::fs::read_to_string(path).ok()?;
    let json: Value = serde_json::from_str(&content).ok()?;

    let provider_key = provider.trim().to_lowercase();
    let json_provider = json
        .get("provider")
        .or_else(|| json.get("type"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_lowercase();
    if json_provider != provider_key {
        return None;
    }

    let disabled = json.get("disabled").and_then(|v| v.as_bool()).unwrap_or(false);
    let enabled = json.get("enabled").and_then(|v| v.as_bool()).unwrap_or(!disabled);
    if disabled || !enabled {
        return None;
    }

    let priority = parse_candidate_priority(&json);
    let id = path
        .strip_prefix(auth_dir)
        .unwrap_or(path)
        .to_string_lossy()
        .to_string();
    Some(AuthCandidate {
        id,
        path: path.to_path_buf(),
        priority,
    })
}

fn select_auth_candidates(provider: &str, model: &str) -> Vec<AuthCandidate> {
    let auth_dir = crate::config::resolve_auth_dir();
    if !auth_dir.exists() {
        return Vec::new();
    }

    let mut files = Vec::new();
    collect_json_files(&auth_dir, &mut files);

    let mut candidates = Vec::new();
    for path in files {
        if let Some(candidate) = candidate_from_path(provider, &auth_dir, &path) {
            candidates.push(candidate);
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    let max_priority = candidates.iter().map(|c| c.priority).max().unwrap_or(0);
    let mut available: Vec<AuthCandidate> = candidates
        .into_iter()
        .filter(|c| c.priority == max_priority)
        .collect();
    available.sort_by(|a, b| a.id.cmp(&b.id));

    if available.len() <= 1 {
        return available;
    }

    let strategy = crate::config::get_config()
        .map(|c| c.routing.strategy)
        .unwrap_or_else(|| "round-robin".to_string());
    let strategy = strategy.trim().to_lowercase();
    let use_round_robin = strategy.is_empty()
        || matches!(strategy.as_str(), "round-robin" | "roundrobin" | "rr");
    if !use_round_robin {
        return available;
    }

    let key = format!("{}:{}", provider.trim().to_lowercase(), model.trim());
    let start = {
        let mut cursor = AUTH_SELECTOR.lock().unwrap();
        let entry = cursor.entry(key).or_insert(0);
        let idx = *entry % available.len();
        *entry = entry.wrapping_add(1);
        idx
    };

    available.rotate_left(start);
    available
}

#[derive(Clone, Copy)]
enum TokenLocation {
    Nested,
    Root,
}

struct TokenSnapshot {
    access_token: String,
    refresh_token: Option<String>,
    expires_at: Option<chrono::DateTime<chrono::Utc>>,
    location: TokenLocation,
    expiry_key: Option<&'static str>,
}

fn parse_rfc3339(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

fn parse_token_snapshot(json: &Value) -> Option<TokenSnapshot> {
    if let Some(token_obj) = json.get("token").and_then(|v| v.as_object()) {
        let access = token_obj
            .get("access_token")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if !access.is_empty() {
            let refresh = token_obj
                .get("refresh_token")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            let expiry_value = token_obj.get("expires_at").or_else(|| token_obj.get("expiry"));
            let expires_at = expiry_value
                .and_then(|v| v.as_str())
                .and_then(parse_rfc3339);
            let expiry_key = if token_obj.contains_key("expires_at") {
                "expires_at"
            } else {
                "expiry"
            };
            return Some(TokenSnapshot {
                access_token: access,
                refresh_token: refresh,
                expires_at,
                location: TokenLocation::Nested,
                expiry_key: Some(expiry_key),
            });
        }
    }

    let access = json
        .get("access_token")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    if access.is_empty() {
        return None;
    }
    let refresh = json
        .get("refresh_token")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let expires_at = json
        .get("expired")
        .and_then(|v| v.as_str())
        .and_then(parse_rfc3339);
    Some(TokenSnapshot {
        access_token: access,
        refresh_token: refresh,
        expires_at,
        location: TokenLocation::Root,
        expiry_key: Some("expired"),
    })
}

fn is_expired(expires_at: Option<chrono::DateTime<chrono::Utc>>) -> bool {
    if let Some(expiry) = expires_at {
        return expiry <= chrono::Utc::now();
    }
    false
}

/// Get a valid Gemini access token from stored credentials
/// Supports CLIProxyAPI format (gemini-*.json)
async fn get_gemini_auth(model: &str) -> Option<GeminiAuth> {
    let candidates = select_auth_candidates("gemini", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let snapshot = match parse_token_snapshot(&json) {
            Some(s) => s,
            None => continue,
        };

        let project_id = json
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        if !is_expired(snapshot.expires_at) {
            return Some(GeminiAuth {
                access_token: snapshot.access_token,
                project_id,
            });
        }

        let refresh_token = match snapshot.refresh_token {
            Some(v) => v,
            None => continue,
        };

        if let Ok(new_tokens) = google::refresh_token(&refresh_token).await {
            let new_expiry = new_tokens.expires_in.map(|secs| {
                (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
            });

            match snapshot.location {
                TokenLocation::Nested => {
                    if json.get("token").is_none() {
                        json["token"] = json!({});
                    }
                    if let Some(obj) = json.get_mut("token").and_then(|v| v.as_object_mut()) {
                        obj.insert(
                            "access_token".to_string(),
                            serde_json::json!(new_tokens.access_token),
                        );
                        if let Some(new_refresh) = &new_tokens.refresh_token {
                            obj.insert("refresh_token".to_string(), serde_json::json!(new_refresh));
                        }
                        if let Some(exp) = new_expiry {
                            let key = snapshot.expiry_key.unwrap_or("expiry");
                            obj.insert(key.to_string(), serde_json::json!(exp));
                        }
                        obj.insert(
                            "token_type".to_string(),
                            serde_json::json!(new_tokens.token_type),
                        );
                    }
                }
                TokenLocation::Root => {
                    json["access_token"] = serde_json::json!(new_tokens.access_token);
                    if let Some(new_refresh) = &new_tokens.refresh_token {
                        json["refresh_token"] = serde_json::json!(new_refresh);
                    }
                    if let Some(exp) = new_expiry {
                        json["expired"] = serde_json::json!(exp);
                    }
                    json["token_type"] = serde_json::json!(new_tokens.token_type);
                }
            }

            if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                let _ = std::fs::write(&candidate.path, updated_content);
            }

            return Some(GeminiAuth {
                access_token: new_tokens.access_token,
                project_id,
            });
        }
    }
    None
}

/// Get a valid Claude access token from stored credentials
async fn get_claude_token(model: &str) -> Option<String> {
    let candidates = select_auth_candidates("claude", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let snapshot = match parse_token_snapshot(&json) {
            Some(s) => s,
            None => continue,
        };

        if !is_expired(snapshot.expires_at) {
            return Some(snapshot.access_token);
        }

        let refresh_token = match snapshot.refresh_token {
            Some(v) => v,
            None => continue,
        };

        if let Ok(new_tokens) = anthropic::refresh_token(&refresh_token).await {
            let new_expiry = new_tokens.expires_in.map(|secs| {
                (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
            });

            match snapshot.location {
                TokenLocation::Nested => {
                    if json.get("token").is_none() {
                        json["token"] = json!({});
                    }
                    if let Some(obj) = json.get_mut("token").and_then(|v| v.as_object_mut()) {
                        obj.insert(
                            "access_token".to_string(),
                            serde_json::json!(new_tokens.access_token),
                        );
                        if let Some(new_refresh) = &new_tokens.refresh_token {
                            obj.insert("refresh_token".to_string(), serde_json::json!(new_refresh));
                        }
                        if let Some(exp) = new_expiry {
                            let key = snapshot.expiry_key.unwrap_or("expires_at");
                            obj.insert(key.to_string(), serde_json::json!(exp));
                        }
                        obj.insert(
                            "token_type".to_string(),
                            serde_json::json!(new_tokens.token_type),
                        );
                    }
                }
                TokenLocation::Root => {
                    json["access_token"] = serde_json::json!(new_tokens.access_token);
                    if let Some(new_refresh) = &new_tokens.refresh_token {
                        json["refresh_token"] = serde_json::json!(new_refresh);
                    }
                    if let Some(exp) = new_expiry {
                        json["expired"] = serde_json::json!(exp);
                    }
                }
            }

            if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                let _ = std::fs::write(&candidate.path, updated_content);
            }

            return Some(new_tokens.access_token);
        }
    }
    None
}

/// Get a valid Codex access token from stored credentials
async fn get_codex_token(model: &str) -> Option<String> {
    let candidates = select_auth_candidates("codex", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let snapshot = match parse_token_snapshot(&json) {
            Some(s) => s,
            None => continue,
        };

        if !is_expired(snapshot.expires_at) {
            return Some(snapshot.access_token);
        }

        let refresh_token = match snapshot.refresh_token {
            Some(v) => v,
            None => continue,
        };

        if let Ok(new_tokens) = openai::refresh_token(&refresh_token).await {
            let new_expiry = new_tokens.expires_in.map(|secs| {
                (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
            });

            match snapshot.location {
                TokenLocation::Nested => {
                    if json.get("token").is_none() {
                        json["token"] = json!({});
                    }
                    if let Some(obj) = json.get_mut("token").and_then(|v| v.as_object_mut()) {
                        obj.insert(
                            "access_token".to_string(),
                            serde_json::json!(new_tokens.access_token),
                        );
                        if let Some(new_refresh) = &new_tokens.refresh_token {
                            obj.insert("refresh_token".to_string(), serde_json::json!(new_refresh));
                        }
                        if let Some(exp) = new_expiry {
                            let key = snapshot.expiry_key.unwrap_or("expires_at");
                            obj.insert(key.to_string(), serde_json::json!(exp));
                        }
                        obj.insert(
                            "token_type".to_string(),
                            serde_json::json!(new_tokens.token_type),
                        );
                        if let Some(id_token) = &new_tokens.id_token {
                            obj.insert("id_token".to_string(), serde_json::json!(id_token));
                        }
                    }
                }
                TokenLocation::Root => {
                    json["access_token"] = serde_json::json!(new_tokens.access_token);
                    if let Some(new_refresh) = &new_tokens.refresh_token {
                        json["refresh_token"] = serde_json::json!(new_refresh);
                    }
                    if let Some(exp) = new_expiry {
                        json["expired"] = serde_json::json!(exp);
                    }
                    if let Some(id_token) = &new_tokens.id_token {
                        json["id_token"] = serde_json::json!(id_token);
                    }
                }
            }

            if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                let _ = std::fs::write(&candidate.path, updated_content);
            }

            return Some(new_tokens.access_token);
        }
    }
    None
}

/// Get a valid Antigravity access token from stored credentials
async fn get_antigravity_auth(model: &str) -> Option<AntigravityAuth> {
    let candidates = select_auth_candidates("antigravity", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let mut json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let snapshot = match parse_token_snapshot(&json) {
            Some(s) => s,
            None => continue,
        };

        let mut project_id = json
            .get("project_id")
            .and_then(|v| v.as_str())
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty());

        if !is_expired(snapshot.expires_at) {
            if project_id.is_none() {
                if let Ok(pid) = antigravity_oauth::fetch_project_id(&snapshot.access_token).await {
                    if !pid.trim().is_empty() {
                        project_id = Some(pid.clone());
                        json["project_id"] = serde_json::json!(pid);
                        if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                            let _ = std::fs::write(&candidate.path, updated_content);
                        }
                    }
                }
            }
            return Some(AntigravityAuth {
                access_token: snapshot.access_token,
                project_id,
            });
        }

        let refresh_token = match snapshot.refresh_token {
            Some(v) => v,
            None => continue,
        };

        if let Ok(new_tokens) = antigravity_oauth::refresh_token(&refresh_token).await {
            let new_expiry = new_tokens.expires_in.map(|secs| {
                (chrono::Utc::now() + chrono::Duration::seconds(secs as i64)).to_rfc3339()
            });

            match snapshot.location {
                TokenLocation::Nested => {
                    if json.get("token").is_none() {
                        json["token"] = json!({});
                    }
                    if let Some(obj) = json.get_mut("token").and_then(|v| v.as_object_mut()) {
                        obj.insert(
                            "access_token".to_string(),
                            serde_json::json!(new_tokens.access_token),
                        );
                        if let Some(new_refresh) = &new_tokens.refresh_token {
                            obj.insert("refresh_token".to_string(), serde_json::json!(new_refresh));
                        }
                        if let Some(exp) = new_expiry.clone() {
                            let key = snapshot.expiry_key.unwrap_or("expires_at");
                            obj.insert(key.to_string(), serde_json::json!(exp));
                        }
                        obj.insert(
                            "token_type".to_string(),
                            serde_json::json!(new_tokens.token_type),
                        );
                    }
                }
                TokenLocation::Root => {
                    json["access_token"] = serde_json::json!(new_tokens.access_token);
                    if let Some(new_refresh) = &new_tokens.refresh_token {
                        json["refresh_token"] = serde_json::json!(new_refresh);
                    }
                    if let Some(exp) = new_expiry.clone() {
                        json["expired"] = serde_json::json!(exp);
                    }
                    if let Some(secs) = new_tokens.expires_in {
                        json["expires_in"] = serde_json::json!(secs);
                        json["timestamp"] = serde_json::json!(chrono::Utc::now().timestamp_millis());
                    }
                    json["token_type"] = serde_json::json!(new_tokens.token_type);
                    json["type"] = serde_json::json!("antigravity");
                }
            }

            if project_id.is_none() {
                if let Ok(pid) = antigravity_oauth::fetch_project_id(&new_tokens.access_token).await {
                    if !pid.trim().is_empty() {
                        project_id = Some(pid.clone());
                        json["project_id"] = serde_json::json!(pid);
                    }
                }
            }

            if let Ok(updated_content) = serde_json::to_string_pretty(&json) {
                let _ = std::fs::write(&candidate.path, updated_content);
            }

            return Some(AntigravityAuth {
                access_token: new_tokens.access_token,
                project_id,
            });
        }
    }
    None
}

pub async fn chat_completions(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let raw_model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let is_stream = raw.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let (provider_override, model) = parse_provider_prefix(&raw_model);

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...').",
                "type": "invalid_request_error",
                "code": 400
            }
        }))
        .into_response();
    }

    if provider_override.as_deref() == Some("gemini") {
        // Get Gemini token
        let auth = match get_gemini_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Gemini credentials found. Please login with Google first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = GeminiClient::new(auth.access_token);

        let mut gemini_request = gemini::openai_to_gemini_cli_request(&raw, &model);
        if let Some(project_id) = auth.project_id {
            gemini_request["project"] = json!(project_id);
        }

        if is_stream {
            match client.stream_generate_content(&gemini_request).await {
                Ok(response) => {
                    let stream = gemini::gemini_cli_stream_to_openai_events(response);
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Gemini API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Gemini API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&gemini_request).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                return Json(openai_response).into_response();
            }
            Err(e) => {
                tracing::error!("Gemini API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("codex") {
        let token = match get_codex_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Codex credentials found. Please login with Codex first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&raw, &model, true);

        if is_stream {
            match client.stream_responses(&codex_request, true).await {
                Ok(response) => {
                    let stream = codex::codex_stream_to_openai_events(response, raw.clone());
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.stream_responses(&codex_request, true).await {
            Ok(response) => match codex::collect_non_stream_response(response, &raw).await {
                Ok(openai_response) => return Json(openai_response).into_response(),
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            },
            Err(e) => {
                tracing::error!("Codex API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Codex API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("antigravity") {
        let auth = match get_antigravity_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let AntigravityAuth {
            access_token,
            project_id,
        } = auth;
        let client = AntigravityClient::new(access_token);
        let antigravity_request =
            antigravity::openai_to_antigravity_request(&raw, &model, project_id);

        if is_stream {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => {
                    let stream = antigravity::antigravity_stream_to_openai_events(response);
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        if antigravity::should_use_stream_for_non_stream(&model) {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                    Ok(payload) => {
                        let openai_response =
                            gemini::gemini_to_openai_response(&payload, &model, &request_id);
                        return Json(openai_response).into_response();
                    }
                    Err(e) => {
                        tracing::error!("Antigravity API error: {}", e);
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", e),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                },
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&antigravity_request, None).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                return Json(openai_response).into_response();
            }
            Err(e) => {
                tracing::error!("Antigravity API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Antigravity API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("claude") {
        let request: ChatCompletionRequest = match serde_json::from_value(raw.clone()) {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Invalid request: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        };
        // Get Claude token
        let token = match get_claude_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Claude credentials found. Please login with Anthropic first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
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
            let openai_response =
                claude::claude_to_openai_response(&response, &request.model, &request_id);
            return Json(openai_response).into_response();
        }
            Err(e) => {
                tracing::error!("Claude API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Claude API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    Json(json!({
        "error": {
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/...).",
            "type": "invalid_request_error",
            "code": 400
        }
    }))
    .into_response()
}

pub async fn completions(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let is_stream = raw.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let chat_request = convert_completions_request_to_chat(&raw);
    let raw_model = chat_request.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (provider_override, model) = parse_provider_prefix(&raw_model);

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...').",
                "type": "invalid_request_error",
                "code": 400
            }
        }))
        .into_response();
    }

    if provider_override.as_deref() == Some("gemini") {
        let auth = match get_gemini_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Gemini credentials found. Please login with Google first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = GeminiClient::new(auth.access_token);
        let mut gemini_request = gemini::openai_to_gemini_cli_request(&chat_request, &model);
        if let Some(project_id) = auth.project_id {
            gemini_request["project"] = json!(project_id);
        }

        if is_stream {
            match client.stream_generate_content(&gemini_request).await {
                Ok(response) => {
                    let upstream = gemini::gemini_cli_stream_to_openai_chunks(response);
                    let stream = async_stream::stream! {
                        futures::pin_mut!(upstream);
                        while let Some(chunk) = upstream.next().await {
                            if chunk == "[DONE]" {
                                yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                                return;
                            }
                            if let Some(converted) = convert_chat_stream_chunk_to_completions(&chunk) {
                                yield Ok::<Event, Infallible>(Event::default().data(converted));
                            }
                        }
                        yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                    };
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Gemini API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Gemini API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&gemini_request).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                let completions_response = convert_chat_response_to_completions(&openai_response);
                return Json(completions_response).into_response();
            }
            Err(e) => {
                tracing::error!("Gemini API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("codex") {
        let token = match get_codex_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Codex credentials found. Please login with Codex first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&chat_request, &model, true);

        if is_stream {
            match client.stream_responses(&codex_request, true).await {
                Ok(response) => {
                    let upstream = codex::codex_stream_to_openai_chunks(response, chat_request.clone());
                    let stream = async_stream::stream! {
                        futures::pin_mut!(upstream);
                        while let Some(chunk) = upstream.next().await {
                            if chunk == "[DONE]" {
                                yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                                return;
                            }
                            if let Some(converted) = convert_chat_stream_chunk_to_completions(&chunk) {
                                yield Ok::<Event, Infallible>(Event::default().data(converted));
                            }
                        }
                        yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                    };
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.stream_responses(&codex_request, true).await {
            Ok(response) => match codex::collect_non_stream_response(response, &chat_request).await {
                Ok(openai_response) => {
                    let completions_response = convert_chat_response_to_completions(&openai_response);
                    return Json(completions_response).into_response();
                }
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            },
            Err(e) => {
                tracing::error!("Codex API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Codex API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("antigravity") {
        let auth = match get_antigravity_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let AntigravityAuth {
            access_token,
            project_id,
        } = auth;
        let client = AntigravityClient::new(access_token);
        let antigravity_request =
            antigravity::openai_to_antigravity_request(&chat_request, &model, project_id);

        if is_stream {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => {
                    let upstream = antigravity::antigravity_stream_to_openai_chunks(response);
                    let stream = async_stream::stream! {
                        futures::pin_mut!(upstream);
                        while let Some(chunk) = upstream.next().await {
                            if chunk == "[DONE]" {
                                yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                                return;
                            }
                            if let Some(converted) = convert_chat_stream_chunk_to_completions(&chunk) {
                                yield Ok::<Event, Infallible>(Event::default().data(converted));
                            }
                        }
                        yield Ok::<Event, Infallible>(Event::default().data("[DONE]"));
                    };
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        if antigravity::should_use_stream_for_non_stream(&model) {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                    Ok(payload) => {
                        let openai_response =
                            gemini::gemini_to_openai_response(&payload, &model, &request_id);
                        let completions_response = convert_chat_response_to_completions(&openai_response);
                        return Json(completions_response).into_response();
                    }
                    Err(e) => {
                        tracing::error!("Antigravity API error: {}", e);
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", e),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                },
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&antigravity_request, None).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                let completions_response = convert_chat_response_to_completions(&openai_response);
                return Json(completions_response).into_response();
            }
            Err(e) => {
                tracing::error!("Antigravity API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Antigravity API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("claude") {
        let request: ChatCompletionRequest = match serde_json::from_value(chat_request.clone()) {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Invalid request: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        };

        let token = match get_claude_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Claude credentials found. Please login with Anthropic first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = ClaudeClient::new(token);
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
                let openai_response =
                    claude::claude_to_openai_response(&response, &request.model, &request_id);
                let completions_response = convert_chat_response_to_completions(&openai_response);
                return Json(completions_response).into_response();
            }
            Err(e) => {
                tracing::error!("Claude API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Claude API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    Json(json!({
        "error": {
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/...).",
            "type": "invalid_request_error",
            "code": 400
        }
    }))
    .into_response()
}

// Claude compatible endpoint
pub async fn claude_messages(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let raw_model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (provider_override, model) = parse_provider_prefix(&raw_model);
    let is_stream = raw.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...').",
                "type": "invalid_request_error",
                "code": 400
            }
        }))
        .into_response();
    }

    if provider_override.as_deref() == Some("claude") {
        let token = match get_claude_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Claude credentials found. Please login with Anthropic first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let mut payload = raw.clone();
        payload["model"] = json!(model);
        if is_stream {
            payload["stream"] = json!(true);
        }

        let url = "https://api.anthropic.com/v1/messages";
        let client = reqwest::Client::new();
        let response = match client
            .post(url)
            .header("x-api-key", token)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&payload)
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "type": "error",
                    "error": {
                        "type": "api_error",
                        "message": format!("Claude API error: {}", e)
                    }
                }))
                .into_response();
            }
        };

        let status = response.status();
        if !status.is_success() {
            let body = response.bytes().await.unwrap_or_default();
            let mut resp = Response::new(Body::from(body));
            *resp.status_mut() = status;
            resp.headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
            return resp;
        }

        if is_stream {
            let stream = response
                .bytes_stream()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
            let mut resp = Response::new(Body::from_stream(stream));
            *resp.status_mut() = StatusCode::OK;
            resp.headers_mut().insert(
                header::CONTENT_TYPE,
                HeaderValue::from_static("text/event-stream"),
            );
            return resp;
        }

        let body = response.bytes().await.unwrap_or_default();
        let body = maybe_decompress_gzip(&body);
        let json_body: Value = serde_json::from_slice(&body).unwrap_or_else(|_| json!({}));
        return Json(json_body).into_response();
    }

    let openai_raw = claude::claude_request_to_openai_chat(&raw, &model);

    if provider_override.as_deref() == Some("gemini") {
        let auth = match get_gemini_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Gemini credentials found. Please login with Google first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = GeminiClient::new(auth.access_token);
        let mut gemini_request = gemini::openai_to_gemini_cli_request(&openai_raw, &model);
        if let Some(project_id) = auth.project_id {
            gemini_request["project"] = json!(project_id);
        }

        if is_stream {
            match client.stream_generate_content(&gemini_request).await {
                Ok(response) => {
                    let upstream = gemini::gemini_cli_stream_to_openai_chunks(response);
                    let stream = openai_chunks_to_claude_events(upstream, &model);
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Gemini API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Gemini API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&gemini_request).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                let claude_response =
                    claude::openai_to_claude_response(&openai_response, &model, &request_id);
                return Json(claude_response).into_response();
            }
            Err(e) => {
                tracing::error!("Gemini API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("codex") {
        let token = match get_codex_token(&model).await {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Codex credentials found. Please login with Codex first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&openai_raw, &model, true);

        if is_stream {
            match client.stream_responses(&codex_request, true).await {
                Ok(response) => {
                    let upstream = codex::codex_stream_to_openai_chunks(response, openai_raw.clone());
                    let stream = openai_chunks_to_claude_events(upstream, &model);
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.stream_responses(&codex_request, true).await {
            Ok(response) => match codex::collect_non_stream_response(response, &openai_raw).await {
                Ok(openai_response) => {
                    let claude_response =
                        claude::openai_to_claude_response(&openai_response, &model, &request_id);
                    return Json(claude_response).into_response();
                }
                Err(e) => {
                    tracing::error!("Codex API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Codex API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            },
            Err(e) => {
                tracing::error!("Codex API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Codex API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("antigravity") {
        let auth = match get_antigravity_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let AntigravityAuth {
            access_token,
            project_id,
        } = auth;
        let client = AntigravityClient::new(access_token);
        let antigravity_request =
            antigravity::openai_to_antigravity_request(&openai_raw, &model, project_id);

        if is_stream {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => {
                    let upstream = antigravity::antigravity_stream_to_openai_chunks(response);
                    let stream = openai_chunks_to_claude_events(upstream, &model);
                    return Sse::new(stream).into_response();
                }
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        if antigravity::should_use_stream_for_non_stream(&model) {
            match client.stream_generate_content(&antigravity_request, None).await {
                Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                    Ok(payload) => {
                        let openai_response =
                            gemini::gemini_to_openai_response(&payload, &model, &request_id);
                        let claude_response =
                            claude::openai_to_claude_response(&openai_response, &model, &request_id);
                        return Json(claude_response).into_response();
                    }
                    Err(e) => {
                        tracing::error!("Antigravity API error: {}", e);
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", e),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                },
                Err(e) => {
                    tracing::error!("Antigravity API error: {}", e);
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        match client.generate_content(&antigravity_request, None).await {
            Ok(response) => {
                let openai_response =
                    gemini::gemini_to_openai_response(&response, &model, &request_id);
                let claude_response =
                    claude::openai_to_claude_response(&openai_response, &model, &request_id);
                return Json(claude_response).into_response();
            }
            Err(e) => {
                tracing::error!("Antigravity API error: {}", e);
                return Json(json!({
                    "error": {
                        "message": format!("Antigravity API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    Json(json!({
        "error": {
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/...).",
            "type": "invalid_request_error",
            "code": 400
        }
    }))
    .into_response()
}

pub async fn claude_count_tokens(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> Response {
    let raw_model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let (provider_override, model) = parse_provider_prefix(&raw_model);

    if provider_override.as_deref() != Some("claude") {
        return Json(json!({
            "error": {
                "message": "Model must include claude/ prefix for Claude token counting.",
                "type": "invalid_request_error",
                "code": 400
            }
        }))
        .into_response();
    }

    let token = match get_claude_token(&model).await {
        Some(t) => t,
        None => {
            return Json(json!({
                "error": {
                    "message": "No valid Claude credentials found. Please login with Anthropic first.",
                    "type": "authentication_error",
                    "code": 401
                }
            }))
            .into_response();
        }
    };

    let mut payload = raw.clone();
    payload["model"] = json!(model);

    let url = "https://api.anthropic.com/v1/messages/count_tokens";
    let client = reqwest::Client::new();
    let response = match client
        .post(url)
        .header("x-api-key", token)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({
                "type": "error",
                "error": {
                    "type": "api_error",
                    "message": format!("Claude API error: {}", e)
                }
            }))
            .into_response();
        }
    };

    let status = response.status();
    let body = response.bytes().await.unwrap_or_default();
    let mut resp = Response::new(Body::from(body));
    *resp.status_mut() = status;
    resp.headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
    resp
}

// Gemini compatible endpoints
pub async fn gemini_models(State(_state): State<AppState>) -> Json<Value> {
    let base = get_gemini_models();
    let mut models = Vec::new();
    for model in base {
        let name = if model.id.starts_with("models/") {
            model.id.clone()
        } else {
            format!("models/{}", model.id)
        };
        models.push(json!({
            "name": name,
            "displayName": model.id,
            "description": model.id,
            "supportedGenerationMethods": ["generateContent"]
        }));
    }
    Json(json!({ "models": models }))
}

pub async fn gemini_get_handler(
    State(_state): State<AppState>,
    Path(action): Path<String>,
) -> impl IntoResponse {
    let action = action.trim_start_matches('/');
    let available = get_gemini_models();
    for model in available {
        let name = if model.id.starts_with("models/") {
            model.id.clone()
        } else {
            format!("models/{}", model.id)
        };
        if name == action || model.id == action {
            return Json(json!({
                "name": name,
                "displayName": model.id,
                "description": model.id,
                "supportedGenerationMethods": ["generateContent"]
            }))
            .into_response();
        }
    }

    Json(json!({
        "error": {
            "message": "Not Found",
            "type": "not_found"
        }
    }))
    .into_response()
}

pub async fn gemini_handler(
    State(_state): State<AppState>,
    Path(action): Path<String>,
    Query(params): Query<HashMap<String, String>>,
    Json(request): Json<Value>,
) -> impl IntoResponse {
    let action = action.trim_start_matches('/').to_string();
    let parts: Vec<&str> = action.split(':').collect();
    if parts.len() != 2 {
        return Json(json!({
            "error": {
                "message": format!("{} not found.", action),
                "type": "invalid_request_error"
            }
        }))
        .into_response();
    }

    let method = parts[1];
    let mut model_name = parts[0].to_string();
    if let Some(stripped) = model_name.strip_prefix("models/") {
        model_name = stripped.to_string();
    }

    let auth = match get_gemini_auth(&model_name).await {
        Some(a) => a,
        None => {
            return Json(json!({
                "error": {
                    "message": "No valid Gemini credentials found. Please login with Google first.",
                    "type": "authentication_error",
                    "code": 401
                }
            }))
            .into_response();
        }
    };

    let mut payload = json!({
        "model": model_name,
        "request": request
    });
    if let Some(project_id) = auth.project_id {
        payload["project"] = json!(project_id);
    }

    let client = GeminiClient::new(auth.access_token);
    match method {
        "generateContent" => match client.generate_content(&payload).await {
            Ok(response) => Json(response).into_response(),
            Err(e) => Json(json!({
                "error": {
                    "message": format!("Gemini API error: {}", e),
                    "type": "api_error",
                    "code": 500
                }
            }))
            .into_response(),
        },
        "streamGenerateContent" => {
            let alt = params.get("alt").map(|v| v.as_str());
            match client.stream_generate_content_with_alt(&payload, alt).await {
                Ok(response) => {
                    let stream = response
                        .bytes_stream()
                        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e));
                    let mut resp = Response::new(Body::from_stream(stream));
                    *resp.status_mut() = StatusCode::OK;
                    let content_type = if alt.unwrap_or("sse") == "sse" {
                        "text/event-stream"
                    } else {
                        "application/json"
                    };
                    resp.headers_mut().insert(
                        header::CONTENT_TYPE,
                        HeaderValue::from_static(content_type),
                    );
                    resp.into_response()
                }
                Err(e) => Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response(),
            }
        }
        "countTokens" => {
            if let Some(obj) = payload.as_object_mut() {
                obj.remove("project");
                obj.remove("model");
            }
            match client.count_tokens(&payload).await {
                Ok(response) => Json(response).into_response(),
                Err(e) => Json(json!({
                    "error": {
                        "message": format!("Gemini API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response(),
            }
        }
        _ => Json(json!({
            "error": {
                "message": format!("Gemini action '{}' not yet implemented", method),
                "type": "invalid_request_error",
                "code": 400
            }
        }))
        .into_response(),
    }
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
                project_id: None,
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
                project_id: None,
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
    match antigravity_oauth::exchange_code(&code, &state).await {
        Ok(token_response) => {
            tracing::info!("Successfully exchanged code for Antigravity tokens");

            // Get user info
            let email = match antigravity_oauth::get_user_info(&token_response.access_token).await {
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
            let project_id = match antigravity_oauth::fetch_project_id(&token_response.access_token).await {
                Ok(pid) if !pid.trim().is_empty() => Some(pid),
                Ok(_) => None,
                Err(e) => {
                    tracing::warn!("Failed to fetch Antigravity project id: {}", e);
                    None
                }
            };

            let auth_file = AuthFile {
                provider: "antigravity".to_string(),
                email: email.clone(),
                token: TokenInfo {
                    access_token: token_response.access_token,
                    refresh_token: token_response.refresh_token,
                    expires_at,
                    token_type: token_response.token_type,
                },
                project_id,
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
