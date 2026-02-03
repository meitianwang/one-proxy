// API request handlers

use axum::{
    body::{Body, Bytes},
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
use super::kiro;
use super::AppState;
use crate::auth::{self, providers::{google, anthropic, antigravity as antigravity_oauth, openai}, AuthFile, TokenInfo};
use crate::auth::providers::antigravity::QuotaData as AntigravityQuotaData;
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

const KIMI_ANTHROPIC_BASE: &str = "https://api.kimi.com/coding/v1";
const GLM_ANTHROPIC_BASE: &str = "https://open.bigmodel.cn/api/anthropic/v1";

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
    let mut has_kimi = false;
    let mut has_glm = false;
    let mut has_kiro = false;

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
                                "kimi" => has_kimi = true,
                                "glm" => has_glm = true,
                                "kiro" => has_kiro = true,
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

    // Add Codex/OpenAI models if available (with reasoning_effort variants)
    if has_codex {
        let base = get_codex_models();
        models.extend(build_codex_models_with_reasoning(&base));
    }

    // Add Antigravity models if available
    if has_antigravity {
        let base = get_antigravity_models();
        models.extend(build_prefixed_models("antigravity", &base));
        models.extend(build_antigravity_models_with_reasoning(&base));
    }

    // Add Claude models if available
    if has_claude {
        let base = get_claude_models();
        models.extend(build_prefixed_models("claude", &base));
    }

    // Add Kimi models if available
    if has_kimi {
        let base = get_kimi_models();
        models.extend(build_prefixed_models("kimi", &base));
    }

    // Add GLM models if available
    if has_glm {
        let base = get_glm_models();
        models.extend(build_prefixed_models("glm", &base));
    }

    // Add Kiro models if available
    if has_kiro {
        if let Some(auth) = get_kiro_auth("auto").await {
            if kiro::ensure_model_cache(&auth).await.is_ok() {
                let model_ids = kiro::available_models();
                if !model_ids.is_empty() {
                    let created = chrono::Utc::now().timestamp();
                    let base: Vec<ModelInfo> = model_ids
                        .into_iter()
                        .map(|id| ModelInfo {
                            id,
                            object: "model".to_string(),
                            created,
                            owned_by: "anthropic".to_string(),
                        })
                        .collect();
                    models.extend(build_prefixed_models("kiro", &base));
                }
            }
        }
    }

    // Add custom provider models
    if let Some(config) = crate::config::get_config() {
        let created = chrono::Utc::now().timestamp();

        // OpenAI-compatible providers
        for entry in &config.openai_compatibility {
            if entry.api_key_entries.is_empty() {
                continue;
            }
            let prefix = entry.prefix.as_ref().unwrap_or(&entry.name);
            let custom_models: Vec<ModelInfo> = entry.models.iter().map(|m| ModelInfo {
                id: m.clone(),
                object: "model".to_string(),
                created,
                owned_by: entry.name.clone(),
            }).collect();
            if custom_models.is_empty() {
                // If no models specified, add a placeholder
                models.push(ModelInfo {
                    id: format!("{}/default", prefix),
                    object: "model".to_string(),
                    created,
                    owned_by: entry.name.clone(),
                });
            } else {
                models.extend(build_prefixed_models(prefix, &custom_models));
            }
        }

        // Claude Code-compatible providers
        for entry in &config.claude_code_compatibility {
            if entry.api_key_entries.is_empty() {
                continue;
            }
            let prefix = entry.prefix.as_ref().unwrap_or(&entry.name);
            let custom_models: Vec<ModelInfo> = entry.models.iter().map(|m| ModelInfo {
                id: m.clone(),
                object: "model".to_string(),
                created,
                owned_by: entry.name.clone(),
            }).collect();
            if custom_models.is_empty() {
                // If no models specified, add a placeholder
                models.push(ModelInfo {
                    id: format!("{}/default", prefix),
                    object: "model".to_string(),
                    created,
                    owned_by: entry.name.clone(),
                });
            } else {
                models.extend(build_prefixed_models(prefix, &custom_models));
            }
        }
    }

    // In model aggregation mode, aggregate models by base name
    let config = crate::config::get_config().unwrap_or_default();
    if config.model_routing.mode == "model" {
        // Aggregate models: extract base model names and combine providers
        use std::collections::HashMap;
        let mut aggregated: HashMap<String, (ModelInfo, Vec<(String, String)>)> = HashMap::new();
        
        for model in &models {
            // Parse provider/model format
            if let Some((provider, base_model)) = model.id.split_once('/') {
                // Normalize model name to unify different naming conventions
                let normalized = normalize_model_name(base_model);
                let entry = aggregated.entry(normalized.clone()).or_insert_with(|| {
                    (ModelInfo {
                        id: normalized.clone(),
                        object: model.object.clone(),
                        created: model.created,
                        owned_by: String::new(),
                    }, Vec::new())
                });
                // Store both provider and original model name for routing
                entry.1.push((provider.to_string(), base_model.to_string()));
            }
        }
        
        // Sort providers by priority and build final list
        let priorities = super::model_router::get_sorted_priorities();
        let priority_order: HashMap<String, u32> = priorities.iter()
            .map(|p| (p.provider.clone(), p.priority))
            .collect();
        
        let mut aggregated_models: Vec<ModelInfo> = aggregated.into_iter()
            .map(|(_base_model, (mut info, mut providers))| {
                // Sort providers by priority
                providers.sort_by(|(a, _), (b, _)| {
                    let pa = priority_order.get(a).copied().unwrap_or(0);
                    let pb = priority_order.get(b).copied().unwrap_or(0);
                    pb.cmp(&pa)
                });
                // Set owned_by to show which providers support this model
                info.owned_by = providers.iter().map(|(p, _)| p.as_str()).collect::<Vec<_>>().join(", ");
                info
            })
            .collect();
        
        // Sort models alphabetically
        aggregated_models.sort_by(|a, b| a.id.cmp(&b.id));
        
        return Json(ModelsResponse {
            object: "list".to_string(),
            data: aggregated_models,
        });
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

/// Normalize model names to unify different naming conventions
/// e.g., "claude-sonnet-4.5" and "claude-sonnet-4-5" are the same model
fn normalize_model_name(name: &str) -> String {
    let normalized = name.to_lowercase();
    
    // Normalize version separators: 4.5 -> 4-5, 4_5 -> 4-5
    let normalized = normalized
        .replace(".", "-")
        .replace("_", "-");
    
    // Common model name mappings
    let normalized = match normalized.as_str() {
        // Claude models - normalize variations
        "claude-sonnet-4-5" => "claude-sonnet-4-5".to_string(),
        "claude-4-5-sonnet" => "claude-sonnet-4-5".to_string(),
        "claude-opus-4-5" => "claude-opus-4-5".to_string(),
        "claude-4-5-opus" => "claude-opus-4-5".to_string(),
        "claude-haiku-4-5" => "claude-haiku-4-5".to_string(),
        "claude-4-5-haiku" => "claude-haiku-4-5".to_string(),
        _ => normalized,
    };
    
    normalized
}

/// Build Codex models with reasoning_effort variants
fn build_codex_models_with_reasoning(base: &[ModelInfo]) -> Vec<ModelInfo> {
    let efforts = ["low", "medium", "high", "xhigh"];
    let mut models = Vec::new();
    for m in base {
        for effort in &efforts {
            models.push(ModelInfo {
                id: format!("codex/{}/{}", effort, m.id),
                object: m.object.clone(),
                created: m.created,
                owned_by: m.owned_by.clone(),
            });
        }
    }
    models
}

/// Build Antigravity models with thinking-level variants
fn build_antigravity_models_with_reasoning(base: &[ModelInfo]) -> Vec<ModelInfo> {
    let mut models = Vec::new();
    for m in base {
        if let Some(levels) = antigravity_supported_levels(&m.id) {
            for effort in levels {
                models.push(ModelInfo {
                    id: format!("antigravity/{}/{}", effort, m.id),
                    object: m.object.clone(),
                    created: m.created,
                    owned_by: m.owned_by.clone(),
                });
            }
        }
    }
    models
}

/// Parse Codex model name with reasoning_effort
/// e.g. "codex/high/gpt-5-codex" -> ("gpt-5-codex", Some("high"))
fn parse_codex_model_with_effort(model: &str) -> (String, Option<String>) {
    let efforts = ["low", "medium", "high", "xhigh"];
    let parts: Vec<&str> = model.splitn(2, '/').collect();
    if parts.len() == 2 {
        let potential_effort = parts[0];
        if efforts.contains(&potential_effort) {
            return (parts[1].to_string(), Some(potential_effort.to_string()));
        }
    }
    (model.to_string(), None)
}

/// Parse Antigravity model name with thinking level
/// e.g. "high/gemini-3-pro-high" -> ("gemini-3-pro-high", Some("high"))
fn parse_antigravity_model_with_effort(model: &str) -> (String, Option<String>) {
    let efforts = ["none", "auto", "minimal", "low", "medium", "high", "xhigh"];
    let parts: Vec<&str> = model.splitn(2, '/').collect();
    if parts.len() == 2 {
        let potential_effort = parts[0];
        if efforts.contains(&potential_effort) {
            return (parts[1].to_string(), Some(potential_effort.to_string()));
        }
    }
    (model.to_string(), None)
}

fn antigravity_supported_levels(model: &str) -> Option<&'static [&'static str]> {
    let lower = model.to_lowercase();
    if lower.starts_with("gemini-3-pro-high") || lower.starts_with("gemini-3-pro-image") {
        return Some(&["low", "high"]);
    }
    if lower.starts_with("gemini-3-flash") {
        return Some(&["minimal", "low", "medium", "high"]);
    }
    None
}

fn antigravity_level_supported(model: &str, level: &str) -> bool {
    let level = level.trim().to_lowercase();
    if level == "none" || level == "auto" {
        return antigravity_supported_levels(model).is_some();
    }
    if let Some(levels) = antigravity_supported_levels(model) {
        return levels.iter().any(|l| *l == level);
    }
    false
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
    let lower = prefix.trim().to_lowercase();
    match lower.as_str() {
        "gemini" => Some("gemini".to_string()),
        "codex" => Some("codex".to_string()),
        "openai" => Some("codex".to_string()),
        "claude" => Some("claude".to_string()),
        "antigravity" => Some("antigravity".to_string()),
        "kimi" => Some("kimi".to_string()),
        "glm" => Some("glm".to_string()),
        "kiro" => Some("kiro".to_string()),
        _ => {
            // Check custom providers
            if let Some(config) = crate::config::get_config() {
                // Check OpenAI-compatible providers
                for entry in &config.openai_compatibility {
                    let provider_prefix = entry.prefix.as_ref().unwrap_or(&entry.name);
                    if provider_prefix.to_lowercase() == lower {
                        return Some(format!("openai-compat:{}", provider_prefix.to_lowercase()));
                    }
                }
                // Check Claude Code-compatible providers
                for entry in &config.claude_code_compatibility {
                    let provider_prefix = entry.prefix.as_ref().unwrap_or(&entry.name);
                    if provider_prefix.to_lowercase() == lower {
                        return Some(format!("claude-compat:{}", provider_prefix.to_lowercase()));
                    }
                }
            }
            None
        }
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

fn strip_sse_data_line(chunk: &str) -> Option<String> {
    let trimmed = chunk.trim();
    let payload = trimmed.strip_prefix("data:")?.trim();
    if payload.is_empty() {
        None
    } else {
        Some(payload.to_string())
    }
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

async fn forward_claude_compatible(
    payload: Value,
    base_url: &str,
    token: &str,
    is_stream: bool,
    provider_label: &str,
) -> Response {
    let base = base_url.trim_end_matches('/').to_string();
    if base.is_empty() {
        return Json(json!({
            "type": "error",
            "error": {
                "type": "api_error",
                "message": format!("{} API error: missing base URL", provider_label)
            }
        }))
        .into_response();
    }
    let url = format!("{}/messages", base);
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
                    "message": format!("{} API error: {}", provider_label, e)
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
    Json(json_body).into_response()
}

#[derive(Default)]
struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments: String,
    started: bool,
}

#[derive(Default)]
struct ClaudeStreamState {
    message_id: String,
    model: String,
    message_started: bool,
    thinking_started: bool,
    thinking_closed: bool,
    text_started: bool,
    finish_reason: Option<String>,
    input_tokens: u32,
    output_tokens: u32,
    tool_calls: HashMap<i32, ToolCallAccumulator>,
    tool_call_block_index: HashMap<i32, i32>,
    thinking_index: Option<i32>,
    text_index: Option<i32>,
    next_block_index: i32,
    block_types: HashMap<i32, &'static str>,
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

fn openai_chunk_to_claude_events(
    chunk: &str,
    state: &mut ClaudeStreamState,
    reasoning_as_text: bool,
) -> Vec<Event> {
    fn alloc_block_index(state: &mut ClaudeStreamState) -> i32 {
        let idx = state.next_block_index;
        state.next_block_index += 1;
        idx
    }

    fn update_usage(state: &mut ClaudeStreamState, usage: &Value) {
        let input = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage.get("prompt_tokens").and_then(|v| v.as_u64()))
            .or_else(|| {
                usage
                    .get("total_tokens")
                    .and_then(|v| v.as_u64())
                    .and_then(|total| {
                        usage
                            .get("completion_tokens")
                            .and_then(|v| v.as_u64())
                            .map(|completion| total.saturating_sub(completion))
                    })
            })
            .unwrap_or(0) as u32;
        let output = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .or_else(|| usage.get("completion_tokens").and_then(|v| v.as_u64()))
            .unwrap_or(0) as u32;

        if input > 0 {
            state.input_tokens = input;
        }
        if output > 0 {
            state.output_tokens = output;
        }
    }

    let mut events = Vec::new();
    let parsed: Value = match serde_json::from_str(chunk) {
        Ok(v) => v,
        Err(_) => return events,
    };

    if let Some(usage) = parsed.get("usage") {
        update_usage(state, usage);
    }

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
                    "input_tokens": state.input_tokens,
                    "output_tokens": state.output_tokens
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
            let emit_text = |text: &str, state: &mut ClaudeStreamState, events: &mut Vec<Event>| {
                if text.is_empty() {
                    return;
                }
                // Close thinking block before starting text block
                if state.thinking_started && !state.thinking_closed {
                    let thinking_index = state.thinking_index.unwrap_or(0);
                    let stop_payload = json!({
                        "type": "content_block_stop",
                        "index": thinking_index
                    });
                    events.push(build_claude_event("content_block_stop", stop_payload));
                    state.thinking_closed = true;
                }

                if let Some(idx) = state.text_index {
                    if state.block_types.get(&idx).copied() != Some("text") {
                        state.text_index = None;
                        state.text_started = false;
                    }
                }

                let text_index = match state.text_index {
                    Some(idx) => idx,
                    None => {
                        let idx = alloc_block_index(state);
                        state.text_index = Some(idx);
                        idx
                    }
                };
                if !state.text_started {
                    let payload = json!({
                        "type": "content_block_start",
                        "index": text_index,
                        "content_block": {
                            "type": "text",
                            "text": ""
                        }
                    });
                    events.push(build_claude_event("content_block_start", payload));
                    state.block_types.insert(text_index, "text");
                    state.text_started = true;
                }
                let payload = json!({
                    "type": "content_block_delta",
                    "index": text_index,
                    "delta": {
                        "type": "text_delta",
                        "text": text
                    }
                });
                events.push(build_claude_event("content_block_delta", payload));
            };

            // Handle reasoning_content (thinking) - index 0
            // Only process if thinking block hasn't been closed yet
            if let Some(reasoning) = delta.get("reasoning_content").and_then(|v| v.as_str()) {
                if !reasoning.is_empty() {
                    if reasoning_as_text {
                        emit_text(reasoning, state, &mut events);
                    } else if !state.thinking_closed {
                        if let Some(idx) = state.thinking_index {
                            if state.block_types.get(&idx).copied() != Some("thinking") {
                                state.thinking_index = None;
                                state.thinking_started = false;
                            }
                        }
                        if !state.thinking_started {
                            let thinking_index = state.thinking_index.unwrap_or_else(|| {
                                let idx = alloc_block_index(state);
                                state.thinking_index = Some(idx);
                                idx
                            });
                            let payload = json!({
                                "type": "content_block_start",
                                "index": thinking_index,
                                "content_block": {
                                    "type": "thinking",
                                    "thinking": "",
                                    "signature": format!("sig_{}", uuid::Uuid::new_v4().to_string().replace("-", "")[..32].to_string())
                                }
                            });
                            events.push(build_claude_event("content_block_start", payload));
                            state.block_types.insert(thinking_index, "thinking");
                            state.thinking_started = true;
                        }
                        let thinking_index = state.thinking_index.unwrap_or(0);
                        let payload = json!({
                            "type": "content_block_delta",
                            "index": thinking_index,
                            "delta": {
                                "type": "thinking_delta",
                                "thinking": reasoning
                            }
                        });
                        events.push(build_claude_event("content_block_delta", payload));
                    }
                }
            }

            // Handle content (text) - index 1 if thinking exists, else index 0
            if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                emit_text(content, state, &mut events);
            }

            if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                for tool_call in tool_calls {
                    let index = tool_call
                        .get("index")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0) as i32;
                    let block_index = match state.tool_call_block_index.get(&index) {
                        Some(idx) => *idx,
                        None => {
                            let idx = alloc_block_index(state);
                            state.tool_call_block_index.insert(index, idx);
                            idx
                        }
                    };
                    let entry = state.tool_calls.entry(index).or_default();

                    if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                        if !id.is_empty() {
                            entry.id = id.to_string();
                        }
                    }

                    if let Some(function) = tool_call.get("function") {
                        if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                            if !name.is_empty() {
                                entry.name = name.to_string();
                            }
                        }

                        if let Some(args) = function.get("arguments").and_then(|v| v.as_str()) {
                            if !args.is_empty() {
                                entry.arguments.push_str(args);
                            }
                        }
                    }

                    if !entry.started && !entry.name.is_empty() {
                        if state.text_started {
                            let text_index = state.text_index.unwrap_or(0);
                            let stop_payload = json!({
                                "type": "content_block_stop",
                                "index": text_index
                            });
                            events.push(build_claude_event("content_block_stop", stop_payload));
                            state.text_started = false;
                            state.text_index = None;
                        }

                        let payload = json!({
                            "type": "content_block_start",
                            "index": block_index,
                            "content_block": {
                                "type": "tool_use",
                                "id": entry.id,
                                "name": entry.name,
                                "input": {}
                            }
                        });
                        events.push(build_claude_event("content_block_start", payload));
                        state.block_types.insert(block_index, "tool_use");
                        entry.started = true;
                    }
                }
            }
        }
    }

    events
}

fn finalize_claude_stream(state: &mut ClaudeStreamState) -> Vec<Event> {
    let mut events = Vec::new();

    if !state.message_started {
        if state.message_id.is_empty() {
            state.message_id = format!("msg_{}", uuid::Uuid::new_v4());
        }
        if state.model.is_empty() {
            state.model = "unknown".to_string();
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
                    "input_tokens": state.input_tokens,
                    "output_tokens": state.output_tokens
                }
            }
        });
        events.push(build_claude_event("message_start", payload));
        state.message_started = true;
    }

    // Close thinking block if it was started but not yet closed
    if state.thinking_started && !state.thinking_closed {
        let thinking_index = state.thinking_index.unwrap_or(0);
        events.push(build_claude_event(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": thinking_index
            }),
        ));
    }

    // Close text block
    if state.text_started {
        let text_index = state.text_index.unwrap_or(0);
        events.push(build_claude_event(
            "content_block_stop",
            json!({
                "type": "content_block_stop",
                "index": text_index
            }),
        ));
    }

    let mut tool_blocks: Vec<(i32, &ToolCallAccumulator)> = Vec::new();
    for (tool_index, tool_call) in &state.tool_calls {
        if let Some(block_index) = state.tool_call_block_index.get(tool_index) {
            tool_blocks.push((*block_index, tool_call));
        }
    }
    tool_blocks.sort_by_key(|(index, _)| *index);
    for (block_index, tool_call) in tool_blocks {
            let args = if tool_call.arguments.trim().is_empty() {
                "{}".to_string()
            } else {
                tool_call.arguments.clone()
            };
            let input_delta = json!({
                "type": "content_block_delta",
                "index": block_index,
                "delta": {
                    "type": "input_json_delta",
                    "partial_json": args
                }
            });
            events.push(build_claude_event("content_block_delta", input_delta));
            let stop_payload = json!({
                "type": "content_block_stop",
                "index": block_index
            });
            events.push(build_claude_event("content_block_stop", stop_payload));
    }

    let stop_reason = map_openai_finish_reason(state.finish_reason.as_deref());
    events.push(build_claude_event(
        "message_delta",
        json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": stop_reason,
                "stop_sequence": null
            },
            "usage": {
                "input_tokens": state.input_tokens,
                "output_tokens": state.output_tokens
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
    openai_chunks_to_claude_events_with_options(upstream, model_hint, false)
}

fn openai_chunks_to_claude_events_with_options<S>(
    upstream: S,
    model_hint: &str,
    reasoning_as_text: bool,
) -> impl futures::Stream<Item = Result<Event, Infallible>>
where
    S: futures::Stream<Item = String>,
{
    let model_hint = model_hint.to_string();
    let reasoning_as_text = reasoning_as_text;
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
            for event in openai_chunk_to_claude_events(&chunk, &mut state, reasoning_as_text) {
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

/// Get static Kimi model definitions
fn get_kimi_models() -> Vec<ModelInfo> {
    let created = chrono::Utc::now().timestamp();
    vec![ModelInfo {
        id: "kimi-for-coding".to_string(),
        object: "model".to_string(),
        created,
        owned_by: "kimi".to_string(),
    }]
}

/// Get static GLM model definitions
fn get_glm_models() -> Vec<ModelInfo> {
    let created = chrono::Utc::now().timestamp();
    vec![
        ModelInfo {
            id: "glm-4.7".to_string(),
            object: "model".to_string(),
            created,
            owned_by: "zhipuai".to_string(),
        },
        ModelInfo {
            id: "glm-4.5-air".to_string(),
            object: "model".to_string(),
            created,
            owned_by: "zhipuai".to_string(),
        },
        ModelInfo {
            id: "glm-4.5-flash".to_string(),
            object: "model".to_string(),
            created,
            owned_by: "zhipuai".to_string(),
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

fn extract_api_key(json: &Value) -> Option<String> {
    let fields = ["access_token", "api_key"];
    if let Some(token_obj) = json.get("token").and_then(|v| v.as_object()) {
        for key in fields {
            let value = token_obj
                .get(key)
                .and_then(|v| v.as_str())
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            if let Some(found) = value {
                return Some(found.to_string());
            }
        }
    }

    for key in fields {
        let value = json
            .get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty());
        if let Some(found) = value {
            return Some(found.to_string());
        }
    }

    None
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

fn normalize_antigravity_model(model: &str) -> String {
    let name = model.split('/').last().unwrap_or(model).trim().to_lowercase();
    name
}

fn antigravity_quota_status(quota: &AntigravityQuotaData, model: &str) -> Option<bool> {
    let model_name = normalize_antigravity_model(model);
    let is_claude = model_name.contains("claude");
    let mut matched = false;
    let mut any_available = false;

    for entry in &quota.models {
        let entry_name = entry.name.trim().to_lowercase();
        let is_match = entry_name == model_name || (is_claude && entry_name.contains("claude"));
        if !is_match {
            continue;
        }
        matched = true;
        if entry.percentage > 0 {
            any_available = true;
            break;
        }
    }

    if !matched {
        None
    } else {
        Some(any_available)
    }
}

fn antigravity_candidate_has_quota(candidate: &AuthCandidate, model: &str) -> Option<bool> {
    let account_id = candidate
        .path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(|s| s.to_string())?;

    let cache = crate::db::get_quota_cache(&account_id).ok().flatten()?;
    if cache.provider != "antigravity" {
        return None;
    }
    let quota: AntigravityQuotaData = serde_json::from_str(&cache.quota_data).ok()?;
    antigravity_quota_status(&quota, model)
}

fn parse_antigravity_status(message: &str) -> Option<u16> {
    let needle = "Antigravity request failed:";
    let start = message.find(needle)?;
    let rest = &message[start + needle.len()..];
    let mut digits = String::new();
    for ch in rest.chars() {
        if ch.is_ascii_digit() {
            digits.push(ch);
        } else if !digits.is_empty() {
            break;
        }
    }
    digits.parse().ok()
}

fn should_rotate_antigravity_error(message: &str) -> bool {
    match parse_antigravity_status(message) {
        Some(429 | 401 | 403 | 500) => true,
        _ => {
            let lower = message.to_lowercase();
            lower.contains("quota_exhausted")
                || lower.contains("resource_exhausted")
                || lower.contains("rate_limit_exceeded")
        }
    }
}

/// Get a valid Antigravity access token from stored credentials
async fn get_antigravity_auth(model: &str) -> Option<AntigravityAuth> {
    get_antigravity_auths(model).await.into_iter().next()
}

async fn load_antigravity_auth_from_candidate(candidate: &AuthCandidate) -> Option<AntigravityAuth> {
    let content = std::fs::read_to_string(&candidate.path).ok()?;
    let mut json: serde_json::Value = serde_json::from_str(&content).ok()?;

    let snapshot = parse_token_snapshot(&json)?;

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

    let refresh_token = snapshot.refresh_token?;
    let new_tokens = antigravity_oauth::refresh_token(&refresh_token).await.ok()?;
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

    Some(AntigravityAuth {
        access_token: new_tokens.access_token,
        project_id,
    })
}

async fn get_antigravity_auths(model: &str) -> Vec<AntigravityAuth> {
    let candidates = select_auth_candidates("antigravity", model);
    let candidates = if candidates.len() <= 1 {
        candidates
    } else {
        let mut ranked: Vec<(usize, AuthCandidate, i32)> = candidates
            .into_iter()
            .enumerate()
            .map(|(idx, candidate)| {
                let rank = match antigravity_candidate_has_quota(&candidate, model) {
                    Some(true) => 0,
                    None => 1,
                    Some(false) => 2,
                };
                (idx, candidate, rank)
            })
            .collect();
        ranked.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
        ranked.into_iter().map(|(_, candidate, _)| candidate).collect()
    };

    let mut auths = Vec::new();
    for candidate in candidates {
        if let Some(auth) = load_antigravity_auth_from_candidate(&candidate).await {
            auths.push(auth);
        }
    }
    auths
}

/// Get a valid Kiro access token from stored credentials
async fn get_kiro_auth(model: &str) -> Option<kiro::KiroAuth> {
    let candidates = select_auth_candidates("kiro", model);
    for candidate in candidates {
        let snapshot = match kiro::load_kiro_auth(&candidate.path).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        if !is_expired(snapshot.expires_at) && !snapshot.access_token.trim().is_empty() {
            if let Ok(auth) = kiro::snapshot_to_auth(snapshot) {
                return Some(auth);
            }
            continue;
        }

        if snapshot.refresh_token.is_none() {
            continue;
        }

        if let Ok(updated) = kiro::refresh_kiro_auth(&candidate.path, &snapshot).await {
            if let Ok(auth) = kiro::snapshot_to_auth(updated) {
                return Some(auth);
            }
        }
    }
    None
}

/// Get a Kimi API key from stored credentials
async fn get_kimi_token(model: &str) -> Option<String> {
    let candidates = select_auth_candidates("kimi", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(token) = extract_api_key(&json) {
            return Some(token);
        }
    }
    None
}

/// Get a GLM API key from stored credentials
async fn get_glm_token(model: &str) -> Option<String> {
    let candidates = select_auth_candidates("glm", model);
    for candidate in candidates {
        let content = match std::fs::read_to_string(&candidate.path) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if let Some(token) = extract_api_key(&json) {
            return Some(token);
        }
    }
    None
}

/// Custom provider info for OpenAI-compatible or Claude Code-compatible providers
#[derive(Debug, Clone)]
struct CustomProviderInfo {
    base_url: String,
    api_key: String,
    provider_type: CustomProviderType,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CustomProviderType {
    OpenAICompat,
    ClaudeCodeCompat,
}

/// API key selector state for custom providers (round-robin)
static CUSTOM_PROVIDER_KEY_SELECTOR: Lazy<Mutex<HashMap<String, usize>>> = Lazy::new(|| Mutex::new(HashMap::new()));

/// Get custom provider info by prefix
fn get_custom_provider_info(provider_key: &str) -> Option<CustomProviderInfo> {
    let config = crate::config::get_config()?;

    // Check if it's an OpenAI-compatible provider
    if let Some(prefix) = provider_key.strip_prefix("openai-compat:") {
        for entry in &config.openai_compatibility {
            let entry_prefix = entry.prefix.as_ref().unwrap_or(&entry.name).to_lowercase();
            if entry_prefix == prefix {
                if entry.api_key_entries.is_empty() {
                    return None;
                }
                // Round-robin selection of API keys
                let api_key = {
                    let mut selector = CUSTOM_PROVIDER_KEY_SELECTOR.lock().unwrap();
                    let idx = selector.entry(provider_key.to_string()).or_insert(0);
                    let key = &entry.api_key_entries[*idx % entry.api_key_entries.len()].api_key;
                    *idx = idx.wrapping_add(1);
                    key.clone()
                };
                return Some(CustomProviderInfo {
                    base_url: entry.base_url.clone(),
                    api_key,
                    provider_type: CustomProviderType::OpenAICompat,
                });
            }
        }
    }

    // Check if it's a Claude Code-compatible provider
    if let Some(prefix) = provider_key.strip_prefix("claude-compat:") {
        for entry in &config.claude_code_compatibility {
            let entry_prefix = entry.prefix.as_ref().unwrap_or(&entry.name).to_lowercase();
            if entry_prefix == prefix {
                if entry.api_key_entries.is_empty() {
                    return None;
                }
                // Round-robin selection of API keys
                let api_key = {
                    let mut selector = CUSTOM_PROVIDER_KEY_SELECTOR.lock().unwrap();
                    let idx = selector.entry(provider_key.to_string()).or_insert(0);
                    let key = &entry.api_key_entries[*idx % entry.api_key_entries.len()].api_key;
                    *idx = idx.wrapping_add(1);
                    key.clone()
                };
                return Some(CustomProviderInfo {
                    base_url: entry.base_url.clone(),
                    api_key,
                    provider_type: CustomProviderType::ClaudeCodeCompat,
                });
            }
        }
    }

    None
}

/// Forward request to OpenAI-compatible provider
async fn forward_openai_compatible(
    payload: Value,
    base_url: &str,
    api_key: &str,
    is_stream: bool,
    provider_label: &str,
) -> Response {
    let base = base_url.trim_end_matches('/').to_string();
    if base.is_empty() {
        return Json(json!({
            "error": {
                "message": format!("{} API error: missing base URL", provider_label),
                "type": "api_error",
                "code": 500
            }
        }))
        .into_response();
    }
    let url = format!("{}/chat/completions", base);
    let client = reqwest::Client::new();
    let response = match client
        .post(&url)
        .header("Authorization", format!("Bearer {}", api_key))
        .header("content-type", "application/json")
        .json(&payload)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            return Json(json!({
                "error": {
                    "message": format!("{} API error: {}", provider_label, e),
                    "type": "api_error",
                    "code": 500
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
    Json(json_body).into_response()
}

pub async fn chat_completions(
    State(_state): State<AppState>,
    Json(raw): Json<Value>,
) -> Response {
    let request_id = uuid::Uuid::new_v4().to_string();
    let raw_model = raw.get("model").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let is_stream = raw.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let (provider_override, model) = parse_provider_prefix(&raw_model);

    // Use model router to resolve provider in aggregation mode
    let (resolved_provider, resolved_model, fallback_providers) = if provider_override.is_some() {
        (provider_override, model, Vec::new())
    } else {
        use super::model_router::{resolve_model, ResolvedModel};
        match resolve_model(&raw_model, None) {
            ResolvedModel::Explicit { provider, model } => {
                (Some(provider), model, Vec::new())
            }
            ResolvedModel::Aggregated { provider, model, fallbacks } => {
                tracing::info!("[ModelAggregation] Resolved {} to provider '{}' with fallbacks: {:?}", raw_model, provider, fallbacks);
                (Some(provider), model, fallbacks)
            }
            ResolvedModel::NoProvider { model } => {
                return Json(json!({
                    "error": {
                        "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...'). Or enable Model Aggregation Mode in settings to use models without prefix.",
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        }
    };

    let provider_override = resolved_provider;
    let model = resolved_model;
    // Note: fallback_providers can be used for automatic retry on quota exhaustion in future enhancement

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...').",
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
        // Parse reasoning_effort from model name (e.g., "high/gpt-5-codex")
        let (actual_model, reasoning_effort) = parse_codex_model_with_effort(&model);

        let token = match get_codex_token(&actual_model).await {
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

        // Inject reasoning_effort into request if parsed from model name
        let mut modified_raw = raw.clone();
        if let Some(effort) = reasoning_effort {
            modified_raw["reasoning_effort"] = json!(effort);
        }

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&modified_raw, &actual_model, true);

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
        let (actual_model, reasoning_effort) = parse_antigravity_model_with_effort(&model);
        let mut openai_raw = raw.clone();
        if let Some(ref effort) = reasoning_effort {
            if !antigravity_level_supported(&actual_model, effort) {
                let supported = antigravity_supported_levels(&actual_model)
                    .map(|levels| levels.join(", "))
                    .unwrap_or_else(|| "none".to_string());
                return Json(json!({
                    "error": {
                        "message": format!(
                            "Thinking level '{}' is not supported by model '{}'. Supported levels: {}. Remove the level prefix to use the default behavior.",
                            effort, actual_model, supported
                        ),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
            openai_raw["reasoning_effort"] = json!(effort);
        }

        let auths = get_antigravity_auths(&actual_model).await;
        if auths.is_empty() {
            return Json(json!({
                "error": {
                    "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                    "type": "authentication_error",
                    "code": 401
                }
            }))
            .into_response();
        }

        let mut last_error: Option<String> = None;
        let total = auths.len();
        for (idx, auth) in auths.into_iter().enumerate() {
            let AntigravityAuth {
                access_token,
                project_id,
            } = auth;
            let client = AntigravityClient::new(access_token);
            let antigravity_request =
                antigravity::openai_to_antigravity_request(&openai_raw, &actual_model, project_id);

            if is_stream {
                match client.stream_generate_content(&antigravity_request, None).await {
                    Ok(response) => {
                        let stream = antigravity::antigravity_stream_to_openai_events(response);
                        return Sse::new(stream).into_response();
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                }
            }

            if antigravity::should_use_stream_for_non_stream(&actual_model) {
                match client.stream_generate_content(&antigravity_request, None).await {
                    Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                        Ok(payload) => {
                            let openai_response =
                                gemini::gemini_to_openai_response(&payload, &actual_model, &request_id);
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
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
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
                        gemini::gemini_to_openai_response(&response, &actual_model, &request_id);
                    return Json(openai_response).into_response();
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!("Antigravity API error: {}", msg);
                    last_error = Some(msg.clone());
                    if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                        continue;
                    }
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", msg),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        let message = last_error.unwrap_or_else(|| "Antigravity API error".to_string());
        return Json(json!({
            "error": {
                "message": format!("Antigravity API error: {}", message),
                "type": "api_error",
                "code": 500
            }
        }))
        .into_response();
    }

    if provider_override.as_deref() == Some("kiro") {
        let auth = match get_kiro_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Kiro credentials found. Please login with Kiro first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        if kiro::ensure_model_cache(&auth).await.is_err() {
            return Json(json!({
                "error": {
                    "message": "Failed to load Kiro models. Please try again.",
                    "type": "api_error",
                    "code": 500
                }
            }))
            .into_response();
        }

        let resolution = kiro::resolve_model(&model);
        let conversation_id = kiro::generate_conversation_id(raw.get("messages"));
        let profile_arn = if matches!(auth.auth_type, kiro::KiroAuthType::KiroDesktop) {
            auth.profile_arn.clone()
        } else {
            None
        };

        let payload = match kiro::build_kiro_payload_from_openai(
            &raw,
            &resolution.internal_id,
            conversation_id,
            profile_arn,
        ) {
            Ok(p) => p,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Invalid Kiro request: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        };

        let response = match kiro::send_kiro_request(&auth, &payload, true).await {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        };

        if is_stream {
            let upstream = kiro::stream_kiro_to_openai(
                response,
                model.clone(),
                raw.get("messages").cloned(),
                raw.get("tools").cloned(),
            );
            let stream = upstream.filter_map(|chunk| async move {
                match chunk {
                    Ok(data) => strip_sse_data_line(&data),
                    Err(_) => None,
                }
            });
            let stream =
                stream.map(|payload| Ok::<Event, Infallible>(Event::default().data(payload)));
            return Sse::new(stream).into_response();
        }

        match kiro::collect_stream_response(
            response,
            model.clone(),
            raw.get("messages").cloned(),
            raw.get("tools").cloned(),
        )
        .await
        {
            Ok(openai_response) => return Json(openai_response).into_response(),
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if matches!(provider_override.as_deref(), Some("kimi") | Some("glm")) {
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

        let (token, base_url, provider_label) = match provider_override.as_deref() {
            Some("kimi") => (get_kimi_token(&model).await, KIMI_ANTHROPIC_BASE, "Kimi"),
            Some("glm") => (get_glm_token(&model).await, GLM_ANTHROPIC_BASE, "GLM"),
            _ => (None, "", "Unknown"),
        };

        let token = match token {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": format!("No valid {} credentials found. Please add an API key first.", provider_label),
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = ClaudeClient::new_with_base_url(token, base_url);
        let (messages, system) = claude::openai_to_claude_messages(&request.messages);
        let claude_request = ClaudeRequest {
            model: model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            system,
        };

        match client.create_message(claude_request).await {
            Ok(response) => {
                let openai_response =
                    claude::claude_to_openai_response(&response, &model, &request_id);
                return Json(openai_response).into_response();
            }
            Err(e) => {
                tracing::error!("{} API error: {}", provider_label, e);
                return Json(json!({
                    "error": {
                        "message": format!("{} API error: {}", provider_label, e),
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
            model: model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            system,
        };

        match client.create_message(claude_request).await {
            Ok(response) => {
            let openai_response =
                claude::claude_to_openai_response(&response, &model, &request_id);
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

    // Handle custom providers (OpenAI-compatible and Claude Code-compatible)
    if let Some(ref provider_key) = provider_override {
        if provider_key.starts_with("openai-compat:") || provider_key.starts_with("claude-compat:") {
            let provider_info = match get_custom_provider_info(provider_key) {
                Some(info) => info,
                None => {
                    let provider_name = provider_key.split(':').nth(1).unwrap_or("unknown");
                    return Json(json!({
                        "error": {
                            "message": format!("No API key configured for custom provider '{}'. Please add an API key in settings.", provider_name),
                            "type": "authentication_error",
                            "code": 401
                        }
                    }))
                    .into_response();
                }
            };

            let provider_name = provider_key.split(':').nth(1).unwrap_or("custom");

            // Prepare the request payload with the actual model name
            let mut payload = raw.clone();
            payload["model"] = json!(model);

            match provider_info.provider_type {
                CustomProviderType::OpenAICompat => {
                    return forward_openai_compatible(
                        payload,
                        &provider_info.base_url,
                        &provider_info.api_key,
                        is_stream,
                        provider_name,
                    ).await;
                }
                CustomProviderType::ClaudeCodeCompat => {
                    // Convert OpenAI request to Claude format, call API, convert response back
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

                    let (messages, system) = claude::openai_to_claude_messages(&request.messages);
                    let claude_payload = json!({
                        "model": model,
                        "messages": messages,
                        "max_tokens": request.max_tokens.unwrap_or(4096),
                        "temperature": request.temperature,
                        "system": system,
                        "stream": is_stream
                    });

                    if is_stream {
                        // Streaming: forward and convert Claude stream to OpenAI stream
                        let base = provider_info.base_url.trim_end_matches('/').to_string();
                        let url = format!("{}/messages", base);
                        let client = reqwest::Client::new();
                        let response = match client
                            .post(&url)
                            .header("x-api-key", &provider_info.api_key)
                            .header("anthropic-version", "2023-06-01")
                            .header("content-type", "application/json")
                            .json(&claude_payload)
                            .send()
                            .await
                        {
                            Ok(r) => r,
                            Err(e) => {
                                return Json(json!({
                                    "error": {
                                        "message": format!("{} API error: {}", provider_name, e),
                                        "type": "api_error",
                                        "code": 500
                                    }
                                }))
                                .into_response();
                            }
                        };

                        if !response.status().is_success() {
                            let status = response.status();
                            let body = response.bytes().await.unwrap_or_default();
                            let mut resp = Response::new(Body::from(body));
                            *resp.status_mut() = status;
                            resp.headers_mut()
                                .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                            return resp;
                        }

                        // Convert Claude stream to OpenAI stream
                        let byte_stream = response.bytes_stream();
                        let model_clone = model.clone();
                        let stream = byte_stream
                            .map(move |result| {
                                match result {
                                    Ok(bytes) => {
                                        let text = String::from_utf8_lossy(&bytes);
                                        let mut output = String::new();
                                        for line in text.lines() {
                                            if let Some(data) = line.strip_prefix("data: ") {
                                                if data.trim().is_empty() || data.trim() == "[DONE]" {
                                                    continue;
                                                }
                                                if let Ok(event) = serde_json::from_str::<Value>(data) {
                                                    // Convert Claude event to OpenAI chunk
                                                    let openai_chunk = claude::claude_stream_to_openai_chunk(&event, &model_clone);
                                                    if let Some(chunk) = openai_chunk {
                                                        output.push_str(&format!("data: {}\n\n", serde_json::to_string(&chunk).unwrap_or_default()));
                                                    }
                                                }
                                            }
                                        }
                                        Ok::<_, std::io::Error>(Bytes::from(output))
                                    }
                                    Err(e) => Err(std::io::Error::new(std::io::ErrorKind::Other, e)),
                                }
                            });

                        let mut resp = Response::new(Body::from_stream(stream));
                        *resp.status_mut() = StatusCode::OK;
                        resp.headers_mut().insert(
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("text/event-stream"),
                        );
                        return resp;
                    }

                    // Non-streaming: call API and convert response
                    let response = forward_claude_compatible(
                        claude_payload,
                        &provider_info.base_url,
                        &provider_info.api_key,
                        false,
                        provider_name,
                    ).await;

                    // Extract the Claude response and convert to OpenAI format
                    let (parts, body) = response.into_parts();
                    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
                        Ok(b) => b,
                        Err(e) => {
                            return Json(json!({
                                "error": {
                                    "message": format!("Failed to read response: {}", e),
                                    "type": "api_error",
                                    "code": 500
                                }
                            }))
                            .into_response();
                        }
                    };

                    if !parts.status.is_success() {
                        let mut resp = Response::new(Body::from(body_bytes));
                        *resp.status_mut() = parts.status;
                        resp.headers_mut()
                            .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                        return resp;
                    }

                    let claude_response: Value = match serde_json::from_slice(&body_bytes) {
                        Ok(v) => v,
                        Err(e) => {
                            return Json(json!({
                                "error": {
                                    "message": format!("Failed to parse response: {}", e),
                                    "type": "api_error",
                                    "code": 500
                                }
                            }))
                            .into_response();
                        }
                    };

                    let openai_response = claude::claude_value_to_openai_response(&claude_response, &model, &request_id);
                    return Json(openai_response).into_response();
                }
            }
        }
    }

    Json(json!({
        "error": {
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/..., kimi/..., glm/..., kiro/..., or custom providers).",
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

    // Use model router to resolve provider in aggregation mode
    let (resolved_provider, resolved_model, _fallback_providers) = if provider_override.is_some() {
        (provider_override, model, Vec::new())
    } else {
        use super::model_router::{resolve_model, ResolvedModel};
        match resolve_model(&raw_model, None) {
            ResolvedModel::Explicit { provider, model } => {
                (Some(provider), model, Vec::new())
            }
            ResolvedModel::Aggregated { provider, model, fallbacks } => {
                tracing::info!("[ModelAggregation] Resolved {} to provider '{}' with fallbacks: {:?}", raw_model, provider, fallbacks);
                (Some(provider), model, fallbacks)
            }
            ResolvedModel::NoProvider { model } => {
                return Json(json!({
                    "error": {
                        "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...'). Or enable Model Aggregation Mode in settings.",
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        }
    };

    let provider_override = resolved_provider;
    let model = resolved_model;

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...').",
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
        // Parse reasoning_effort from model name (e.g., "high/gpt-5-codex")
        let (actual_model, reasoning_effort) = parse_codex_model_with_effort(&model);

        let token = match get_codex_token(&actual_model).await {
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

        // Inject reasoning_effort into request if parsed from model name
        let mut modified_request = chat_request.clone();
        if let Some(effort) = reasoning_effort {
            modified_request["reasoning_effort"] = json!(effort);
        }

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&modified_request, &actual_model, true);

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
        let (actual_model, reasoning_effort) = parse_antigravity_model_with_effort(&model);
        let mut openai_raw = chat_request.clone();
        if let Some(ref effort) = reasoning_effort {
            if !antigravity_level_supported(&actual_model, effort) {
                let supported = antigravity_supported_levels(&actual_model)
                    .map(|levels| levels.join(", "))
                    .unwrap_or_else(|| "none".to_string());
                return Json(json!({
                    "error": {
                        "message": format!(
                            "Thinking level '{}' is not supported by model '{}'. Supported levels: {}. Remove the level prefix to use the default behavior.",
                            effort, actual_model, supported
                        ),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
            openai_raw["reasoning_effort"] = json!(effort);
        }

        let auths = get_antigravity_auths(&actual_model).await;
        if auths.is_empty() {
            return Json(json!({
                "error": {
                    "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                    "type": "authentication_error",
                    "code": 401
                }
            }))
            .into_response();
        }

        let mut last_error: Option<String> = None;
        let total = auths.len();
        for (idx, auth) in auths.into_iter().enumerate() {
            let AntigravityAuth {
                access_token,
                project_id,
            } = auth;
            let client = AntigravityClient::new(access_token);
            let antigravity_request =
                antigravity::openai_to_antigravity_request(&openai_raw, &actual_model, project_id);

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
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                }
            }

            if antigravity::should_use_stream_for_non_stream(&actual_model) {
                match client.stream_generate_content(&antigravity_request, None).await {
                    Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                        Ok(payload) => {
                            let openai_response =
                                gemini::gemini_to_openai_response(&payload, &actual_model, &request_id);
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
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
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
                        gemini::gemini_to_openai_response(&response, &actual_model, &request_id);
                    let completions_response = convert_chat_response_to_completions(&openai_response);
                    return Json(completions_response).into_response();
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!("Antigravity API error: {}", msg);
                    last_error = Some(msg.clone());
                    if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                        continue;
                    }
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", msg),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        let message = last_error.unwrap_or_else(|| "Antigravity API error".to_string());
        return Json(json!({
            "error": {
                "message": format!("Antigravity API error: {}", message),
                "type": "api_error",
                "code": 500
            }
        }))
        .into_response();
    }

    if provider_override.as_deref() == Some("kiro") {
        let auth = match get_kiro_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Kiro credentials found. Please login with Kiro first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        if kiro::ensure_model_cache(&auth).await.is_err() {
            return Json(json!({
                "error": {
                    "message": "Failed to load Kiro models. Please try again.",
                    "type": "api_error",
                    "code": 500
                }
            }))
            .into_response();
        }

        let resolution = kiro::resolve_model(&model);
        let conversation_id = kiro::generate_conversation_id(chat_request.get("messages"));
        let profile_arn = if matches!(auth.auth_type, kiro::KiroAuthType::KiroDesktop) {
            auth.profile_arn.clone()
        } else {
            None
        };

        let payload = match kiro::build_kiro_payload_from_openai(&chat_request, &resolution.internal_id, conversation_id, profile_arn) {
            Ok(p) => p,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Invalid Kiro request: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        };

        let response = match kiro::send_kiro_request(&auth, &payload, true).await {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        };

        if is_stream {
            let upstream = kiro::stream_kiro_to_openai(
                response,
                model.clone(),
                chat_request.get("messages").cloned(),
                chat_request.get("tools").cloned(),
            );
            let stream = upstream.filter_map(|chunk| async move {
                match chunk {
                    Ok(data) => strip_sse_data_line(&data).filter(|payload| payload != "[DONE]"),
                    Err(_) => None,
                }
            });
            let stream = stream.map(|chunk| {
                convert_chat_stream_chunk_to_completions(&chunk)
                    .map(|data| Ok::<Event, Infallible>(Event::default().data(data)))
                    .unwrap_or_else(|| Ok::<Event, Infallible>(Event::default().data("{\"choices\":[]}")))
            });
            let stream = stream.chain(futures::stream::once(async {
                Ok(Event::default().data("[DONE]"))
            }));
            return Sse::new(stream).into_response();
        }

        match kiro::collect_stream_response(
            response,
            model.clone(),
            chat_request.get("messages").cloned(),
            chat_request.get("tools").cloned(),
        )
        .await
        {
            Ok(openai_response) => {
                let completions_response = convert_chat_response_to_completions(&openai_response);
                return Json(completions_response).into_response();
            }
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if matches!(provider_override.as_deref(), Some("kimi") | Some("glm")) {
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

        let (token, base_url, provider_label) = match provider_override.as_deref() {
            Some("kimi") => (get_kimi_token(&model).await, KIMI_ANTHROPIC_BASE, "Kimi"),
            Some("glm") => (get_glm_token(&model).await, GLM_ANTHROPIC_BASE, "GLM"),
            _ => (None, "", "Unknown"),
        };

        let token = match token {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": format!("No valid {} credentials found. Please add an API key first.", provider_label),
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        let client = ClaudeClient::new_with_base_url(token, base_url);
        let (messages, system) = claude::openai_to_claude_messages(&request.messages);
        let claude_request = ClaudeRequest {
            model: model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            system,
        };

        match client.create_message(claude_request).await {
            Ok(response) => {
                let openai_response =
                    claude::claude_to_openai_response(&response, &model, &request_id);
                let completions_response = convert_chat_response_to_completions(&openai_response);
                return Json(completions_response).into_response();
            }
            Err(e) => {
                tracing::error!("{} API error: {}", provider_label, e);
                return Json(json!({
                    "error": {
                        "message": format!("{} API error: {}", provider_label, e),
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
            model: model.clone(),
            messages,
            max_tokens: request.max_tokens.unwrap_or(4096),
            temperature: request.temperature,
            system,
        };

        match client.create_message(claude_request).await {
            Ok(response) => {
                let openai_response =
                    claude::claude_to_openai_response(&response, &model, &request_id);
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
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/..., kimi/..., glm/..., kiro/...).",
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

    // Use model router to resolve provider in aggregation mode
    let (resolved_provider, resolved_model, _fallback_providers) = if provider_override.is_some() {
        (provider_override, model, Vec::new())
    } else {
        use super::model_router::{resolve_model, ResolvedModel};
        match resolve_model(&raw_model, None) {
            ResolvedModel::Explicit { provider, model } => {
                (Some(provider), model, Vec::new())
            }
            ResolvedModel::Aggregated { provider, model, fallbacks } => {
                tracing::info!("[ModelAggregation] Resolved {} to provider '{}' with fallbacks: {:?}", raw_model, provider, fallbacks);
                (Some(provider), model, fallbacks)
            }
            ResolvedModel::NoProvider { model } => {
                return Json(json!({
                    "error": {
                        "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...'). Or enable Model Aggregation Mode in settings.",
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        }
    };

    let provider_override = resolved_provider;
    let model = resolved_model;

    if provider_override.is_none() {
        return Json(json!({
            "error": {
                "message": "Model must include provider prefix (e.g. 'gemini/...', 'claude/...', 'codex/...', 'antigravity/...', 'kimi/...', 'glm/...', 'kiro/...').",
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

        return forward_claude_compatible(
            payload,
            "https://api.anthropic.com/v1",
            &token,
            is_stream,
            "Claude",
        )
        .await;
    }

    if matches!(provider_override.as_deref(), Some("kimi") | Some("glm")) {
        let (token, base_url, provider_label) = match provider_override.as_deref() {
            Some("kimi") => (get_kimi_token(&model).await, KIMI_ANTHROPIC_BASE, "Kimi"),
            Some("glm") => (get_glm_token(&model).await, GLM_ANTHROPIC_BASE, "GLM"),
            _ => (None, "", "Unknown"),
        };

        let token = match token {
            Some(t) => t,
            None => {
                return Json(json!({
                    "error": {
                        "message": format!("No valid {} credentials found. Please add an API key first.", provider_label),
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

        return forward_claude_compatible(payload, base_url, &token, is_stream, provider_label).await;
    }

    let image_handling = match provider_override.as_deref() {
        Some("gemini") => claude::ClaudeImageHandling::Base64Any,
        Some("codex") => claude::ClaudeImageHandling::Base64Any,
        Some("antigravity") => claude::ClaudeImageHandling::Base64TypeOnly,
        Some("kiro") => claude::ClaudeImageHandling::Base64TypeOnly,
        _ => claude::ClaudeImageHandling::Base64AndUrl,
    };
    let guard_thinking = provider_override.as_deref() == Some("antigravity");
    let mut openai_raw = claude::claude_request_to_openai_chat(&raw, &model, image_handling, guard_thinking);

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
        // Parse reasoning_effort from model name (e.g., "high/gpt-5-codex")
        let (actual_model, reasoning_effort) = parse_codex_model_with_effort(&model);

        let token = match get_codex_token(&actual_model).await {
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

        // Inject reasoning_effort into request if parsed from model name
        let mut modified_openai_raw = openai_raw.clone();
        if let Some(effort) = reasoning_effort {
            modified_openai_raw["reasoning_effort"] = json!(effort);
        }

        let client = CodexClient::new(token);
        let codex_request = codex::openai_to_codex_request(&modified_openai_raw, &actual_model, true);

        if is_stream {
            match client.stream_responses(&codex_request, true).await {
                Ok(response) => {
                    let upstream = codex::codex_stream_to_openai_chunks(response, modified_openai_raw.clone());
                    let stream = openai_chunks_to_claude_events(upstream, &actual_model);
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
            Ok(response) => match codex::collect_non_stream_response(response, &modified_openai_raw).await {
                Ok(openai_response) => {
                    let claude_response =
                        claude::openai_to_claude_response(&openai_response, &actual_model, &request_id);
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

    if provider_override.as_deref() == Some("kiro") {
        let auth = match get_kiro_auth(&model).await {
            Some(a) => a,
            None => {
                return Json(json!({
                    "error": {
                        "message": "No valid Kiro credentials found. Please login with Kiro first.",
                        "type": "authentication_error",
                        "code": 401
                    }
                }))
                .into_response();
            }
        };

        if kiro::ensure_model_cache(&auth).await.is_err() {
            return Json(json!({
                "error": {
                    "message": "Failed to load Kiro models. Please try again.",
                    "type": "api_error",
                    "code": 500
                }
            }))
            .into_response();
        }

        let resolution = kiro::resolve_model(&model);
        let conversation_id = kiro::generate_conversation_id(openai_raw.get("messages"));
        let profile_arn = if matches!(auth.auth_type, kiro::KiroAuthType::KiroDesktop) {
            auth.profile_arn.clone()
        } else {
            None
        };

        let payload = match kiro::build_kiro_payload_from_openai(
            &openai_raw,
            &resolution.internal_id,
            conversation_id,
            profile_arn,
        ) {
            Ok(p) => p,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Invalid Kiro request: {}", e),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
        };

        let response = match kiro::send_kiro_request(&auth, &payload, true).await {
            Ok(r) => r,
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        };

        if is_stream {
            let upstream = kiro::stream_kiro_to_openai(
                response,
                model.clone(),
                openai_raw.get("messages").cloned(),
                openai_raw.get("tools").cloned(),
            );
            let stream = upstream.filter_map(|chunk| async move {
                match chunk {
                    Ok(data) => strip_sse_data_line(&data),
                    Err(_) => None,
                }
            });
            let stream = openai_chunks_to_claude_events(stream, &model);
            return Sse::new(stream).into_response();
        }

        match kiro::collect_stream_response(
            response,
            model.clone(),
            openai_raw.get("messages").cloned(),
            openai_raw.get("tools").cloned(),
        )
        .await
        {
            Ok(openai_response) => {
                let claude_response =
                    claude::openai_to_claude_response(&openai_response, &model, &request_id);
                return Json(claude_response).into_response();
            }
            Err(e) => {
                return Json(json!({
                    "error": {
                        "message": format!("Kiro API error: {}", e),
                        "type": "api_error",
                        "code": 500
                    }
                }))
                .into_response();
            }
        }
    }

    if provider_override.as_deref() == Some("antigravity") {
        let (actual_model, reasoning_effort) = parse_antigravity_model_with_effort(&model);
        let model_lower = actual_model.to_lowercase();
        let supports_thinking =
            model_lower.contains("-thinking") || model_lower.starts_with("claude-");
        let reasoning_as_text = !supports_thinking;

        if let Some(ref effort) = reasoning_effort {
            if !antigravity_level_supported(&actual_model, effort) {
                let supported = antigravity_supported_levels(&actual_model)
                    .map(|levels| levels.join(", "))
                    .unwrap_or_else(|| "none".to_string());
                return Json(json!({
                    "error": {
                        "message": format!(
                            "Thinking level '{}' is not supported by model '{}'. Supported levels: {}. Remove the level prefix to use the default behavior.",
                            effort, actual_model, supported
                        ),
                        "type": "invalid_request_error",
                        "code": 400
                    }
                }))
                .into_response();
            }
            openai_raw["reasoning_effort"] = json!(effort);
        }

        let auths = get_antigravity_auths(&actual_model).await;
        if auths.is_empty() {
            return Json(json!({
                "error": {
                    "message": "No valid Antigravity credentials found. Please login with Antigravity first.",
                    "type": "authentication_error",
                    "code": 401
                }
            }))
            .into_response();
        }

        let mut last_error: Option<String> = None;
        let total = auths.len();
        for (idx, auth) in auths.into_iter().enumerate() {
            let AntigravityAuth {
                access_token,
                project_id,
            } = auth;
            let client = AntigravityClient::new(access_token);
            let antigravity_request =
                antigravity::openai_to_antigravity_request(&openai_raw, &actual_model, project_id);

            if is_stream {
                match client.stream_generate_content(&antigravity_request, None).await {
                    Ok(response) => {
                        let upstream = antigravity::antigravity_stream_to_openai_chunks(response);
                        let stream = openai_chunks_to_claude_events_with_options(
                            upstream,
                            &actual_model,
                            reasoning_as_text,
                        );
                        return Sse::new(stream).into_response();
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                }
            }

            if antigravity::should_use_stream_for_non_stream(&actual_model) {
                match client.stream_generate_content(&antigravity_request, None).await {
                    Ok(response) => match antigravity::collect_antigravity_stream(response).await {
                        Ok(payload) => {
                            let openai_response =
                                gemini::gemini_to_openai_response(&payload, &actual_model, &request_id);
                            let claude_response = claude::openai_to_claude_response_with_options(
                                &openai_response,
                                &actual_model,
                                &request_id,
                                reasoning_as_text,
                            );
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
                        let msg = e.to_string();
                        tracing::error!("Antigravity API error: {}", msg);
                        last_error = Some(msg.clone());
                        if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                            continue;
                        }
                        return Json(json!({
                            "error": {
                                "message": format!("Antigravity API error: {}", msg),
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
                        gemini::gemini_to_openai_response(&response, &actual_model, &request_id);
                    let claude_response = claude::openai_to_claude_response_with_options(
                        &openai_response,
                        &actual_model,
                        &request_id,
                        reasoning_as_text,
                    );
                    return Json(claude_response).into_response();
                }
                Err(e) => {
                    let msg = e.to_string();
                    tracing::error!("Antigravity API error: {}", msg);
                    last_error = Some(msg.clone());
                    if should_rotate_antigravity_error(&msg) && idx + 1 < total {
                        continue;
                    }
                    return Json(json!({
                        "error": {
                            "message": format!("Antigravity API error: {}", msg),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            }
        }

        let message = last_error.unwrap_or_else(|| "Antigravity API error".to_string());
        return Json(json!({
            "error": {
                "message": format!("Antigravity API error: {}", message),
                "type": "api_error",
                "code": 500
            }
        }))
        .into_response();
    }

    // Handle custom providers (Claude Code-compatible only for /v1/messages endpoint)
    if let Some(ref provider_key) = provider_override {
        if provider_key.starts_with("claude-compat:") {
            let provider_info = match get_custom_provider_info(provider_key) {
                Some(info) => info,
                None => {
                    let provider_name = provider_key.split(':').nth(1).unwrap_or("unknown");
                    return Json(json!({
                        "error": {
                            "message": format!("No API key configured for custom provider '{}'. Please add an API key in settings.", provider_name),
                            "type": "authentication_error",
                            "code": 401
                        }
                    }))
                    .into_response();
                }
            };

            let provider_name = provider_key.split(':').nth(1).unwrap_or("custom");

            let mut payload = raw.clone();
            payload["model"] = json!(model);
            if is_stream {
                payload["stream"] = json!(true);
            }

            return forward_claude_compatible(
                payload,
                &provider_info.base_url,
                &provider_info.api_key,
                is_stream,
                provider_name,
            ).await;
        }

        // Handle OpenAI-compatible providers for /v1/messages endpoint
        // Convert Claude request to OpenAI format, call API, convert response back
        if provider_key.starts_with("openai-compat:") {
            let provider_info = match get_custom_provider_info(provider_key) {
                Some(info) => info,
                None => {
                    let provider_name = provider_key.split(':').nth(1).unwrap_or("unknown");
                    return Json(json!({
                        "error": {
                            "message": format!("No API key configured for custom provider '{}'. Please add an API key in settings.", provider_name),
                            "type": "authentication_error",
                            "code": 401
                        }
                    }))
                    .into_response();
                }
            };

            let provider_name = provider_key.split(':').nth(1).unwrap_or("custom");

            // Convert Claude request to OpenAI format
            let openai_request = claude::claude_request_to_openai_chat(&raw, &model, claude::ClaudeImageHandling::Base64Any, false);
            let mut payload = openai_request;
            payload["model"] = json!(model);
            if is_stream {
                payload["stream"] = json!(true);
            }

            // For streaming, we need to convert OpenAI stream to Claude stream
            if is_stream {
                let base = provider_info.base_url.trim_end_matches('/').to_string();
                let url = format!("{}/chat/completions", base);
                let client = reqwest::Client::new();
                let response = match client
                    .post(&url)
                    .header("Authorization", format!("Bearer {}", provider_info.api_key))
                    .header("content-type", "application/json")
                    .json(&payload)
                    .send()
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        return Json(json!({
                            "error": {
                                "message": format!("{} API error: {}", provider_name, e),
                                "type": "api_error",
                                "code": 500
                            }
                        }))
                        .into_response();
                    }
                };

                if !response.status().is_success() {
                    let status = response.status();
                    let body = response.bytes().await.unwrap_or_default();
                    let mut resp = Response::new(Body::from(body));
                    *resp.status_mut() = status;
                    resp.headers_mut()
                        .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                    return resp;
                }

                // Convert OpenAI stream to Claude stream
                let byte_stream = response.bytes_stream();
                let upstream = byte_stream
                    .map(|result| {
                        result.map(|bytes| String::from_utf8_lossy(&bytes).to_string())
                    })
                    .filter_map(|result| async move {
                        match result {
                            Ok(text) => {
                                // Parse SSE lines
                                let mut chunks = Vec::new();
                                for line in text.lines() {
                                    if let Some(data) = line.strip_prefix("data: ") {
                                        if data.trim() != "[DONE]" && !data.trim().is_empty() {
                                            chunks.push(data.to_string());
                                        }
                                    }
                                }
                                if chunks.is_empty() {
                                    None
                                } else {
                                    Some(chunks.join("\n"))
                                }
                            }
                            Err(_) => None,
                        }
                    })
                    .flat_map(|text| futures::stream::iter(text.lines().map(|s| s.to_string()).collect::<Vec<_>>()));

                let stream = openai_chunks_to_claude_events(upstream, &model);
                return Sse::new(stream).into_response();
            }

            // Non-streaming: call API and convert response
            let response = forward_openai_compatible(
                payload,
                &provider_info.base_url,
                &provider_info.api_key,
                false,
                provider_name,
            ).await;

            // Extract the OpenAI response and convert to Claude format
            let (parts, body) = response.into_parts();
            let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
                Ok(b) => b,
                Err(e) => {
                    return Json(json!({
                        "error": {
                            "message": format!("Failed to read response: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            };

            if !parts.status.is_success() {
                let mut resp = Response::new(Body::from(body_bytes));
                *resp.status_mut() = parts.status;
                resp.headers_mut()
                    .insert(header::CONTENT_TYPE, HeaderValue::from_static("application/json"));
                return resp;
            }

            let openai_response: Value = match serde_json::from_slice(&body_bytes) {
                Ok(v) => v,
                Err(e) => {
                    return Json(json!({
                        "error": {
                            "message": format!("Failed to parse response: {}", e),
                            "type": "api_error",
                            "code": 500
                        }
                    }))
                    .into_response();
                }
            };

            let claude_response = claude::openai_to_claude_response(&openai_response, &model, &request_id);
            return Json(claude_response).into_response();
        }
    }

    Json(json!({
        "error": {
            "message": "Unsupported provider. Use a supported provider prefix (gemini/..., claude/..., codex/..., antigravity/..., kimi/..., glm/..., kiro/..., or custom providers).",
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
            let account_id = format!("antigravity_{}", identifier);
            match auth::fetch_antigravity_quota(&account_id).await {
                Ok(_) => tracing::info!("Fetched Antigravity quota for new account {}", account_id),
                Err(e) => tracing::warn!(
                    "Failed to fetch Antigravity quota for new account {}: {}",
                    account_id,
                    e
                ),
            }
            Html(OAUTH_SUCCESS_HTML.to_string())
        }
        Err(e) => {
            tracing::error!("Failed to exchange code: {}", e);
            Html(OAUTH_ERROR_HTML.replace("{{ERROR}}", &format!("Token exchange failed: {}", e)))
        }
    }
}
