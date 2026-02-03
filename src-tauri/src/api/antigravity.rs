use anyhow::{anyhow, Result};
use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::convert::Infallible;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;
use sha2::{Digest, Sha256};

use super::{gemini, schema_cleaner};

const ANTIGRAVITY_BASE_URL_DAILY: &str = "https://daily-cloudcode-pa.googleapis.com";
const ANTIGRAVITY_BASE_URL_SANDBOX: &str = "https://daily-cloudcode-pa.sandbox.googleapis.com";
const ANTIGRAVITY_STREAM_PATH: &str = "/v1internal:streamGenerateContent";
const ANTIGRAVITY_GENERATE_PATH: &str = "/v1internal:generateContent";
const DEFAULT_USER_AGENT: &str = "antigravity/1.104.0 darwin/arm64";
const SYSTEM_INSTRUCTION: &str = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google Deepmind team working on Advanced Agentic Coding.You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.**Absolute paths only****Proactiveness**";

static FUNCTION_CALL_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct AntigravityClient {
    access_token: String,
    http_client: reqwest::Client,
}

impl AntigravityClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn generate_content(&self, payload: &Value, alt: Option<&str>) -> Result<Value> {
        let response = self
            .send_request(payload, false, alt)
            .await?;
        let body: Value = response.json().await?;
        Ok(body)
    }

    pub async fn stream_generate_content(&self, payload: &Value, alt: Option<&str>) -> Result<reqwest::Response> {
        self.send_request(payload, true, alt).await
    }

    async fn send_request(&self, payload: &Value, stream: bool, alt: Option<&str>) -> Result<reqwest::Response> {
        let base_urls = [ANTIGRAVITY_BASE_URL_DAILY, ANTIGRAVITY_BASE_URL_SANDBOX];
        let path = if stream {
            ANTIGRAVITY_STREAM_PATH
        } else {
            ANTIGRAVITY_GENERATE_PATH
        };

        let mut last_error: Option<String> = None;

        for base in base_urls {
            let mut url = format!("{}{}", base.trim_end_matches('/'), path);
            if stream {
                if let Some(alt) = alt {
                    url.push_str("?$alt=");
                    url.push_str(&urlencoding::encode(alt));
                } else {
                    url.push_str("?alt=sse");
                }
            } else if let Some(alt) = alt {
                url.push_str("?$alt=");
                url.push_str(&urlencoding::encode(alt));
            }

            let mut req = self
                .http_client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.access_token))
                .header("Content-Type", "application/json")
                .header("User-Agent", DEFAULT_USER_AGENT)
                .json(payload);

            req = if stream {
                req.header("Accept", "text/event-stream")
            } else {
                req.header("Accept", "application/json")
            };

            // Handle both network errors and HTTP errors
            let response = match req.send().await {
                Ok(resp) => resp,
                Err(e) => {
                    // Network error - log it and try next URL
                    let err_msg = format!("Network error for {}: {}", url, e);
                    tracing::warn!("{}", err_msg);
                    last_error = Some(err_msg);
                    continue;
                }
            };
            
            if response.status().is_success() {
                return Ok(response);
            }

            // HTTP error - log it and try next URL
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let err_msg = format!("HTTP {} from {}: {}", status, url, body);
            tracing::warn!("{}", err_msg);
            last_error = Some(err_msg);
        }

        Err(anyhow!(
            "Antigravity request failed: {}",
            last_error.unwrap_or_else(|| "unknown error".to_string())
        ))
    }
}

pub fn openai_to_antigravity_request(raw: &Value, model: &str, project_id: Option<String>) -> Value {
    // 尝试解析为 OpenAIRequest 结构体
    if let Ok(openai_req) = serde_json::from_value::<crate::api::mappers::openai::models::OpenAIRequest>(raw.clone()) {
        // 使用新的 mapper 模块进行转换
        let proj_id = project_id.unwrap_or_else(generate_project_id);
        return crate::api::mappers::openai::request::transform_openai_request(&openai_req, &proj_id, model);
    }
    
    // Fallback: 使用原有的转换逻辑
    let mut payload = gemini::openai_to_gemini_cli_request(raw, model);

    if let Some(max_tokens) = raw.get("max_tokens").and_then(|v| v.as_f64()) {
        ensure_generation_config(&mut payload);
        payload["request"]["generationConfig"]["maxOutputTokens"] = json!(max_tokens);
    }

    apply_antigravity_envelope(payload, model, project_id)
}

fn apply_antigravity_envelope(mut payload: Value, model: &str, project_id: Option<String>) -> Value {
    payload["model"] = json!(model);
    payload["userAgent"] = json!("antigravity");
    payload["requestType"] = json!("agent");

    let project = project_id.unwrap_or_else(generate_project_id);
    payload["project"] = json!(project);
    payload["requestId"] = json!(format!("agent-{}", Uuid::new_v4()));
    payload["request"]["sessionId"] = json!(generate_stable_session_id(&payload));

    if let Some(req) = payload.get_mut("request") {
        if let Some(obj) = req.as_object_mut() {
            obj.remove("safetySettings");
        }
    }

    if payload.get("toolConfig").is_some() && payload.get("request").and_then(|v| v.get("toolConfig")).is_none() {
        let tool_config = payload.get("toolConfig").cloned().unwrap_or(Value::Null);
        payload["request"]["toolConfig"] = tool_config;
        if let Some(obj) = payload.as_object_mut() {
            obj.remove("toolConfig");
        }
    }

    rename_key_recursive(&mut payload, "parametersJsonSchema", "parameters");

    let use_antigravity_schema = model.contains("claude") || model.contains("gemini-3-pro-high");
    clean_tool_schemas(&mut payload, use_antigravity_schema);

    if model.contains("claude") || model.contains("gemini-3-pro-high") {
        let existing_parts = payload
            .get("request")
            .and_then(|v| v.get("systemInstruction"))
            .and_then(|v| v.get("parts"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut parts = Vec::new();
        parts.push(json!({ "text": SYSTEM_INSTRUCTION }));
        parts.push(json!({
            "text": format!("Please ignore following [ignore]{}[/ignore]", SYSTEM_INSTRUCTION)
        }));
        for part in existing_parts {
            parts.push(part);
        }

        payload["request"]["systemInstruction"]["role"] = json!("user");
        payload["request"]["systemInstruction"]["parts"] = json!(parts);
    }

    if model.contains("claude") {
        payload["request"]["toolConfig"]["functionCallingConfig"]["mode"] = json!("VALIDATED");
    } else if let Some(gen_cfg) = payload
        .get_mut("request")
        .and_then(|v| v.get_mut("generationConfig"))
        .and_then(|v| v.as_object_mut())
    {
        gen_cfg.remove("maxOutputTokens");
    }

    payload
}

fn clean_tool_schemas(payload: &mut Value, use_antigravity_schema: bool) {
    match payload {
        Value::Object(map) => {
            if let Some(params) = map.get_mut("parameters") {
                let cleaned = if use_antigravity_schema {
                    schema_cleaner::clean_json_schema_for_antigravity(params)
                } else {
                    schema_cleaner::clean_json_schema_for_gemini(params)
                };
                *params = cleaned;
            }
            let keys: Vec<String> = map.keys().cloned().collect();
            for key in keys {
                if let Some(child) = map.get_mut(&key) {
                    clean_tool_schemas(child, use_antigravity_schema);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr {
                clean_tool_schemas(item, use_antigravity_schema);
            }
        }
        _ => {}
    }
}

fn ensure_generation_config(payload: &mut Value) {
    if !payload.get("request").and_then(|v| v.get("generationConfig")).is_some() {
        payload["request"]["generationConfig"] = json!({});
    }
}

fn rename_key_recursive(value: &mut Value, from: &str, to: &str) {
    match value {
        Value::Object(map) => {
            if let Some(v) = map.remove(from) {
                map.insert(to.to_string(), v);
            }
            for (_, v) in map.iter_mut() {
                rename_key_recursive(v, from, to);
            }
        }
        Value::Array(arr) => {
            for v in arr {
                rename_key_recursive(v, from, to);
            }
        }
        _ => {}
    }
}

fn generate_project_id() -> String {
    let adjectives = ["useful", "bright", "swift", "calm", "bold"];
    let nouns = ["fuze", "wave", "spark", "flow", "core"];
    let adj = adjectives[rand_index(adjectives.len())];
    let noun = nouns[rand_index(nouns.len())];
    let random_part = &Uuid::new_v4().to_string().to_lowercase()[..5];
    format!("{}-{}-{}", adj, noun, random_part)
}

fn generate_stable_session_id(payload: &Value) -> String {
    if let Some(contents) = payload
        .get("request")
        .and_then(|v| v.get("contents"))
        .and_then(|v| v.as_array())
    {
        for content in contents {
            if content.get("role").and_then(|v| v.as_str()) == Some("user") {
                if let Some(text) = content
                    .get("parts")
                    .and_then(|v| v.get(0))
                    .and_then(|v| v.get("text"))
                    .and_then(|v| v.as_str())
                {
                    if !text.is_empty() {
                        let mut hasher = Sha256::new();
                        hasher.update(text.as_bytes());
                        let hash = hasher.finalize();
                        if hash.len() >= 8 {
                            let mut arr = [0u8; 8];
                            arr.copy_from_slice(&hash[..8]);
                            let mut n = i64::from_be_bytes(arr);
                            n &= 0x7FFF_FFFF_FFFF_FFFF;
                            return format!("-{}", n);
                        }
                    }
                }
            }
        }
    }
    format!("-{}", rand_i64())
}

fn rand_index(max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    (rand_i64().unsigned_abs() as usize) % max
}

fn rand_i64() -> i64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let mixed = nanos ^ (nanos >> 33) ^ (nanos << 11);
    (mixed & 0x7FFF_FFFF_FFFF_FFFF) as i64
}

pub fn should_use_stream_for_non_stream(model: &str) -> bool {
    model.contains("claude") || model.contains("gemini-3-pro")
}

pub fn antigravity_stream_to_openai_chunks(
    response: reqwest::Response,
) -> impl Stream<Item = String> {
    async_stream::stream! {
        let mut state = AntigravityStreamState {
            unix_timestamp: 0,
            function_index: 0,
            active_function_name: None,
            active_function_id: None,
            active_function_args: String::new(),
            active_function_index: 0,
        };
        let mut buffer = String::new();
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = match chunk {
                Ok(b) => b,
                Err(_) => break,
            };
            let text = String::from_utf8_lossy(&bytes);
            buffer.push_str(&text);

            while let Some(pos) = buffer.find('\n') {
                let mut line = buffer[..pos].to_string();
                buffer = buffer[pos + 1..].to_string();
                line = line.trim_end_matches('\r').to_string();

                if !line.starts_with("data:") {
                    continue;
                }
                let data = line[5..].trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    yield "[DONE]".to_string();
                    return;
                }

                for chunk in convert_antigravity_stream_chunk(data, &mut state) {
                    yield chunk;
                }
            }
        }

        yield "[DONE]".to_string();
    }
}

pub fn antigravity_stream_to_openai_events(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Event, Infallible>> {
    antigravity_stream_to_openai_chunks(response)
        .map(|chunk| Ok(Event::default().data(chunk)))
}

pub async fn collect_antigravity_stream(response: reqwest::Response) -> Result<Value> {
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();
    let mut payloads: Vec<Value> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        let text = String::from_utf8_lossy(&bytes);
        buffer.push_str(&text);

        while let Some(pos) = buffer.find('\n') {
            let mut line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();
            line = line.trim_end_matches('\r').to_string();

            if !line.starts_with("data:") {
                continue;
            }
            let data = line[5..].trim();
            if data.is_empty() || data == "[DONE]" {
                continue;
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                payloads.push(parsed);
            }
        }
    }

    if payloads.is_empty() {
        return Err(anyhow!("stream closed before response payload"));
    }
    Ok(convert_stream_payloads_to_non_stream(&payloads))
}

struct AntigravityStreamState {
    unix_timestamp: i64,
    function_index: i32,
    active_function_name: Option<String>,
    active_function_id: Option<String>,
    active_function_args: String,
    active_function_index: i32,
}

fn convert_antigravity_stream_chunk(data: &str, state: &mut AntigravityStreamState) -> Vec<String> {
    let parsed: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let response = match parsed.get("response") {
        Some(r) => r,
        None => return Vec::new(),
    };

    let mut template = json!({
        "id": "",
        "object": "chat.completion.chunk",
        "created": 0,
        "model": "model",
        "choices": [{
            "index": 0,
            "delta": {
                "role": null,
                "content": null,
                "reasoning_content": null,
                "tool_calls": null
            },
            "finish_reason": null,
            "native_finish_reason": null
        }]
    });

    if let Some(model_version) = response.get("modelVersion").and_then(|v| v.as_str()) {
        template["model"] = json!(model_version);
    }

    if let Some(create_time) = response.get("createTime").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(create_time) {
            state.unix_timestamp = dt.timestamp();
        }
        template["created"] = json!(state.unix_timestamp);
    } else {
        template["created"] = json!(state.unix_timestamp);
    }

    if let Some(response_id) = response.get("responseId").and_then(|v| v.as_str()) {
        template["id"] = json!(response_id);
    }

    if let Some(finish_reason) = response
        .get("candidates")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("finishReason"))
        .and_then(|v| v.as_str())
    {
        let lower = finish_reason.to_ascii_lowercase();
        template["choices"][0]["finish_reason"] = json!(lower);
        template["choices"][0]["native_finish_reason"] = json!(lower);
    }

    if let Some(usage) = response.get("usageMetadata") {
        if let Some(candidates) = usage.get("candidatesTokenCount").and_then(|v| v.as_i64()) {
            template["usage"]["completion_tokens"] = json!(candidates);
        }
        if let Some(total) = usage.get("totalTokenCount").and_then(|v| v.as_i64()) {
            template["usage"]["total_tokens"] = json!(total);
        }
        let cached = usage.get("cachedContentTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        let prompt = usage.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        let thoughts = usage.get("thoughtsTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        template["usage"]["prompt_tokens"] = json!(prompt - cached + thoughts);
        if thoughts > 0 {
            template["usage"]["completion_tokens_details"]["reasoning_tokens"] = json!(thoughts);
        }
        if cached > 0 {
            template["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(cached);
        }
    }

    let mut has_function_call = false;

    if let Some(parts) = response
        .get("candidates")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("content"))
        .and_then(|v| v.get("parts"))
        .and_then(|v| v.as_array())
    {
        for part in parts {
            let part_text = part.get("text").and_then(|v| v.as_str());
            let function_call = part.get("functionCall");
            let thought_sig = part
                .get("thoughtSignature")
                .or_else(|| part.get("thought_signature"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let inline_data = part.get("inlineData").or_else(|| part.get("inline_data"));

            let has_thought_signature = !thought_sig.is_empty();
            let has_content_payload =
                part_text.is_some() || function_call.is_some() || inline_data.is_some();

            if has_thought_signature && !has_content_payload {
                continue;
            }

            if let Some(text) = part_text {
                if part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false) {
                    template["choices"][0]["delta"]["reasoning_content"] = json!(text);
                } else {
                    template["choices"][0]["delta"]["content"] = json!(text);
                }
                template["choices"][0]["delta"]["role"] = json!("assistant");
                continue;
            }

            if let Some(function_call) = function_call {
                has_function_call = true;
                if !template["choices"][0]["delta"]["tool_calls"].is_array() {
                    template["choices"][0]["delta"]["tool_calls"] = json!([]);
                }

                let fc_name = function_call
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if fc_name.is_empty() {
                    continue;
                }

                let args_value = function_call.get("args").cloned().unwrap_or(json!(""));
                let args_str = if let Some(s) = args_value.as_str() {
                    s.to_string()
                } else {
                    serde_json::to_string(&args_value).unwrap_or_else(|_| "{}".to_string())
                };

                let is_continuation = state
                    .active_function_name
                    .as_ref()
                    .map(|n| n == fc_name)
                    .unwrap_or(false)
                    && args_str.starts_with(&state.active_function_args);

                let (tool_id, tool_index, delta_args, include_name) = if is_continuation {
                    let delta = args_str[state.active_function_args.len()..].to_string();
                    state.active_function_args = args_str.clone();
                    (
                        state
                            .active_function_id
                            .clone()
                            .unwrap_or_else(|| fc_name.to_string()),
                        state.active_function_index,
                        delta,
                        false,
                    )
                } else {
                    let counter = FUNCTION_CALL_ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
                    let nanos = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    let new_id = format!("{}-{}-{}", fc_name, nanos, counter);
                    let new_index = state.function_index;
                    state.function_index += 1;
                    state.active_function_name = Some(fc_name.to_string());
                    state.active_function_id = Some(new_id.clone());
                    state.active_function_args = args_str.clone();
                    state.active_function_index = new_index;
                    (new_id, new_index, args_str.clone(), true)
                };

                if delta_args.is_empty() && !include_name {
                    continue;
                }

                let mut tool_call = json!({
                    "id": tool_id,
                    "index": tool_index,
                    "type": "function",
                    "function": {
                        "arguments": delta_args
                    }
                });
                if include_name {
                    tool_call["function"]["name"] = json!(fc_name);
                }
                if let Some(arr) = template["choices"][0]["delta"]["tool_calls"].as_array_mut() {
                    arr.push(tool_call);
                }
                template["choices"][0]["delta"]["role"] = json!("assistant");
                continue;
            }

            if let Some(inline_data) = inline_data {
                let data = inline_data.get("data").and_then(|v| v.as_str()).unwrap_or("");
                if data.is_empty() {
                    continue;
                }
                let mut mime_type = inline_data
                    .get("mimeType")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if mime_type.is_empty() {
                    mime_type = inline_data
                        .get("mime_type")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                }
                if mime_type.is_empty() {
                    mime_type = "image/png".to_string();
                }
                let image_url = format!("data:{};base64,{}", mime_type, data);
                if !template["choices"][0]["delta"]["images"].is_array() {
                    template["choices"][0]["delta"]["images"] = json!([]);
                }
                let index = template["choices"][0]["delta"]["images"]
                    .as_array()
                    .map(|a| a.len())
                    .unwrap_or(0);
                let image_payload = json!({
                    "type": "image_url",
                    "image_url": { "url": image_url },
                    "index": index
                });
                if let Some(arr) = template["choices"][0]["delta"]["images"].as_array_mut() {
                    arr.push(image_payload);
                }
                template["choices"][0]["delta"]["role"] = json!("assistant");
            }
        }
    }

    if has_function_call {
        template["choices"][0]["finish_reason"] = json!("tool_calls");
        template["choices"][0]["native_finish_reason"] = json!("tool_calls");
    }

    vec![template.to_string()]
}

fn convert_stream_payloads_to_non_stream(payloads: &[Value]) -> Value {
    let mut response_template: Option<Value> = None;
    let mut trace_id: Option<String> = None;
    let mut finish_reason: Option<String> = None;
    let mut model_version: Option<String> = None;
    let mut response_id: Option<String> = None;
    let mut role: Option<String> = None;
    let mut usage: Option<Value> = None;

    let mut parts: Vec<Value> = Vec::new();
    let mut pending_kind: Option<&'static str> = None;
    let mut pending_text = String::new();
    let mut pending_thought_sig = String::new();

    let flush_pending = |parts: &mut Vec<Value>,
                             pending_kind: &mut Option<&'static str>,
                             pending_text: &mut String,
                             pending_thought_sig: &mut String| {
        let kind = pending_kind.take();
        if kind.is_none() {
            return;
        }
        let text = std::mem::take(pending_text);
        let sig = std::mem::take(pending_thought_sig);
        match kind.unwrap() {
            "text" => {
                if text.trim().is_empty() {
                    return;
                }
                parts.push(json!({ "text": text }));
            }
            "thought" => {
                if text.trim().is_empty() && sig.is_empty() {
                    return;
                }
                let mut obj = serde_json::Map::new();
                obj.insert("thought".to_string(), json!(true));
                obj.insert("text".to_string(), json!(text));
                if !sig.is_empty() {
                    obj.insert("thoughtSignature".to_string(), json!(sig));
                }
                parts.push(Value::Object(obj));
            }
            _ => {}
        }
    };

    for payload in payloads {
        let response_node = if let Some(resp) = payload.get("response") {
            resp
        } else if payload.get("candidates").is_some() {
            payload
        } else {
            continue;
        };

        response_template = Some(response_node.clone());

        if let Some(trace) = payload.get("traceId").and_then(|v| v.as_str()) {
            if !trace.is_empty() {
                trace_id = Some(trace.to_string());
            }
        }

        if let Some(role_val) = response_node
            .get("candidates")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.get("role"))
            .and_then(|v| v.as_str())
        {
            if !role_val.is_empty() {
                role = Some(role_val.to_string());
            }
        }

        if let Some(fr) = response_node
            .get("candidates")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("finishReason"))
            .and_then(|v| v.as_str())
        {
            if !fr.is_empty() {
                finish_reason = Some(fr.to_string());
            }
        }

        if let Some(mv) = response_node.get("modelVersion").and_then(|v| v.as_str()) {
            if !mv.is_empty() {
                model_version = Some(mv.to_string());
            }
        }
        if let Some(rid) = response_node.get("responseId").and_then(|v| v.as_str()) {
            if !rid.is_empty() {
                response_id = Some(rid.to_string());
            }
        }

        if let Some(usage_node) = response_node.get("usageMetadata") {
            usage = Some(usage_node.clone());
        } else if let Some(usage_node) = payload.get("usageMetadata") {
            usage = Some(usage_node.clone());
        }

        if let Some(parts_arr) = response_node
            .get("candidates")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("content"))
            .and_then(|v| v.get("parts"))
            .and_then(|v| v.as_array())
        {
            for part in parts_arr {
                let has_function_call = part.get("functionCall").is_some();
                let has_inline_data =
                    part.get("inlineData").is_some() || part.get("inline_data").is_some();
                let sig = part
                    .get("thoughtSignature")
                    .or_else(|| part.get("thought_signature"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
                let thought = part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false);

                if has_function_call || has_inline_data {
                    flush_pending(
                        &mut parts,
                        &mut pending_kind,
                        &mut pending_text,
                        &mut pending_thought_sig,
                    );
                    parts.push(normalize_part(part));
                    continue;
                }

                if thought || part.get("text").is_some() {
                    let kind = if thought { "thought" } else { "text" };
                    if pending_kind.is_some() && pending_kind != Some(kind) {
                        flush_pending(
                            &mut parts,
                            &mut pending_kind,
                            &mut pending_text,
                            &mut pending_thought_sig,
                        );
                    }
                    pending_kind = Some(kind);
                    pending_text.push_str(text);
                    if kind == "thought" && !sig.is_empty() {
                        pending_thought_sig = sig;
                    }
                    continue;
                }

                flush_pending(
                    &mut parts,
                    &mut pending_kind,
                    &mut pending_text,
                    &mut pending_thought_sig,
                );
                parts.push(normalize_part(part));
            }
        }
    }

    flush_pending(
        &mut parts,
        &mut pending_kind,
        &mut pending_text,
        &mut pending_thought_sig,
    );

    let mut response = response_template.unwrap_or_else(|| {
        json!({
            "candidates": [{
                "content": { "role": "model", "parts": [] }
            }]
        })
    });

    if response
        .get("candidates")
        .and_then(|v| v.get(0))
        .and_then(|v| v.get("content"))
        .is_none()
    {
        response["candidates"] = json!([{
            "content": { "role": "model", "parts": [] }
        }]);
    }

    response["candidates"][0]["content"]["parts"] = json!(parts);
    if let Some(role_val) = role {
        response["candidates"][0]["content"]["role"] = json!(role_val);
    }
    if let Some(fr) = finish_reason {
        response["candidates"][0]["finishReason"] = json!(fr);
    }
    if let Some(mv) = model_version {
        response["modelVersion"] = json!(mv);
    }
    if let Some(rid) = response_id {
        response["responseId"] = json!(rid);
    }
    if let Some(usage_val) = usage {
        response["usageMetadata"] = usage_val;
    } else if response.get("usageMetadata").is_none() {
        response["usageMetadata"] = json!({
            "promptTokenCount": 0,
            "candidatesTokenCount": 0,
            "totalTokenCount": 0
        });
    }

    let mut output = json!({
        "response": response,
        "traceId": ""
    });
    if let Some(trace) = trace_id {
        output["traceId"] = json!(trace);
    }
    output
}

fn normalize_part(part: &Value) -> Value {
    if let Some(obj) = part.as_object() {
        let mut map = obj.clone();
        if let Some(sig) = map.remove("thought_signature") {
            if !map.contains_key("thoughtSignature") {
                map.insert("thoughtSignature".to_string(), sig);
            }
        }
        if let Some(inline) = map.remove("inline_data") {
            if !map.contains_key("inlineData") {
                map.insert("inlineData".to_string(), inline);
            }
        }
        return Value::Object(map);
    }
    part.clone()
}
