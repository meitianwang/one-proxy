// Gemini API client for proxying requests
// Uses Cloud Code Assist endpoint for OAuth tokens (same as CLIProxyAPI)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

// Cloud Code Assist endpoint for OAuth tokens (same as CLIProxyAPI gemini_cli_executor.go)
const CODE_ASSIST_ENDPOINT: &str = "https://cloudcode-pa.googleapis.com";
const CODE_ASSIST_VERSION: &str = "v1internal";

#[derive(Debug, Clone)]
pub struct GeminiClient {
    access_token: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiContent {
    pub role: String,
    pub parts: Vec<GeminiPart>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiPart {
    pub text: String,
}

/// Safety setting for Gemini API
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SafetySetting {
    pub category: String,
    pub threshold: String,
}

/// Default safety settings (same as CLIProxyAPI common/safety.go)
fn default_safety_settings() -> Vec<SafetySetting> {
    vec![
        SafetySetting {
            category: "HARM_CATEGORY_HARASSMENT".to_string(),
            threshold: "OFF".to_string(),
        },
        SafetySetting {
            category: "HARM_CATEGORY_HATE_SPEECH".to_string(),
            threshold: "OFF".to_string(),
        },
        SafetySetting {
            category: "HARM_CATEGORY_SEXUALLY_EXPLICIT".to_string(),
            threshold: "OFF".to_string(),
        },
        SafetySetting {
            category: "HARM_CATEGORY_DANGEROUS_CONTENT".to_string(),
            threshold: "OFF".to_string(),
        },
        SafetySetting {
            category: "HARM_CATEGORY_CIVIC_INTEGRITY".to_string(),
            threshold: "BLOCK_NONE".to_string(),
        },
    ]
}

/// Inner request structure for Cloud Code Assist API
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiInnerRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_instruction: Option<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub safety_settings: Option<Vec<SafetySetting>>,
}

/// Cloud Code Assist API request envelope (same as CLIProxyAPI gemini-cli format)
#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiCLIRequest {
    pub project: String,
    pub request: GeminiInnerRequest,
    pub model: String,
}

/// Legacy request structure (kept for compatibility)
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
}

static FUNCTION_CALL_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

impl GeminiClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            http_client: reqwest::Client::new(),
        }
    }

    /// Generate content using Cloud Code Assist endpoint (same as CLIProxyAPI)
    /// Payload should already be in Gemini CLI format.
    pub async fn generate_content(&self, payload: &Value) -> Result<Value> {
        // Use Cloud Code Assist endpoint like CLIProxyAPI gemini_cli_executor.go
        let url = format!(
            "{}/{}:generateContent",
            CODE_ASSIST_ENDPOINT,
            CODE_ASSIST_VERSION
        );

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", "google-api-nodejs-client/9.15.1")
            .header("X-Goog-Api-Client", "gl-node/22.17.0")
            .header("Client-Metadata", "ideType=IDE_UNSPECIFIED,platform=PLATFORM_UNSPECIFIED,pluginType=GEMINI")
            .json(payload)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;

        if !status.is_success() {
            return Ok(body);
        }

        Ok(body)
    }

    pub async fn list_models(&self) -> Result<Value> {
        // For listing models, we can use the standard endpoint
        let url = "https://generativelanguage.googleapis.com/v1beta/models";

        let response = self
            .http_client
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        let mut body: Value = response.json().await?;
        if let Some(wrapped) = body.get("response") {
            body = wrapped.clone();
        }
        Ok(body)
    }
}

/// Convert OpenAI chat messages to Gemini format
pub fn openai_to_gemini_messages(messages: &[super::handlers::ChatMessage]) -> Vec<GeminiContent> {
    messages
        .iter()
        .map(|msg| {
            let role = match msg.role.as_str() {
                "assistant" => "model",
                "system" => "user", // Gemini doesn't have system role, prepend to user
                _ => "user",
            };

            GeminiContent {
                role: role.to_string(),
                parts: vec![GeminiPart {
                    text: msg.content.clone(),
                }],
            }
        })
        .collect()
}

const GEMINI_CLI_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";

pub fn openai_to_gemini_cli_request(raw: &Value, model: &str) -> Value {
    let mut request = serde_json::Map::new();
    let mut generation_config = serde_json::Map::new();

    if let Some(re) = raw.get("reasoning_effort").and_then(|v| v.as_str()) {
        let effort = re.trim().to_lowercase();
        if !effort.is_empty() {
            let mut thinking_config = serde_json::Map::new();
            if effort == "auto" {
                thinking_config.insert("thinkingBudget".to_string(), json!(-1));
                thinking_config.insert("includeThoughts".to_string(), json!(true));
            } else {
                thinking_config.insert("thinkingLevel".to_string(), json!(effort));
                thinking_config.insert("includeThoughts".to_string(), json!(effort != "none"));
            }
            generation_config.insert("thinkingConfig".to_string(), Value::Object(thinking_config));
        }
    }

    if let Some(temp) = raw.get("temperature").and_then(|v| v.as_f64()) {
        generation_config.insert("temperature".to_string(), json!(temp));
    }
    if let Some(top_p) = raw.get("top_p").and_then(|v| v.as_f64()) {
        generation_config.insert("topP".to_string(), json!(top_p));
    }
    if let Some(top_k) = raw.get("top_k").and_then(|v| v.as_i64()) {
        generation_config.insert("topK".to_string(), json!(top_k));
    }
    if let Some(n) = raw.get("n").and_then(|v| v.as_i64()) {
        if n > 1 {
            generation_config.insert("candidateCount".to_string(), json!(n));
        }
    }
    if let Some(mods) = raw.get("modalities").and_then(|v| v.as_array()) {
        let mut response_mods = Vec::new();
        for m in mods {
            if let Some(m) = m.as_str() {
                match m.to_lowercase().as_str() {
                    "text" => response_mods.push("TEXT".to_string()),
                    "image" => response_mods.push("IMAGE".to_string()),
                    _ => {}
                }
            }
        }
        if !response_mods.is_empty() {
            generation_config.insert("responseModalities".to_string(), json!(response_mods));
        }
    }
    if let Some(img_cfg) = raw.get("image_config") {
        let mut image_config = serde_json::Map::new();
        if let Some(ar) = img_cfg.get("aspect_ratio").and_then(|v| v.as_str()) {
            image_config.insert("aspectRatio".to_string(), json!(ar));
        }
        if let Some(size) = img_cfg.get("image_size").and_then(|v| v.as_str()) {
            image_config.insert("imageSize".to_string(), json!(size));
        }
        if !image_config.is_empty() {
            generation_config.insert("imageConfig".to_string(), Value::Object(image_config));
        }
    }
    if !generation_config.is_empty() {
        request.insert("generationConfig".to_string(), Value::Object(generation_config));
    }

    let mut contents: Vec<Value> = Vec::new();
    let mut system_parts: Vec<Value> = Vec::new();

    if let Some(messages) = raw.get("messages").and_then(|v| v.as_array()) {
        let mut tc_id_to_name: HashMap<String, String> = HashMap::new();
        for m in messages {
            if m.get("role").and_then(|v| v.as_str()) == Some("assistant") {
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        if tc.get("type").and_then(|v| v.as_str()) != Some("function") {
                            continue;
                        }
                        let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = tc.get("function").and_then(|v| v.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                        if !id.is_empty() && !name.is_empty() {
                            tc_id_to_name.insert(id.to_string(), name.to_string());
                        }
                    }
                }
            }
        }

        let mut tool_responses: HashMap<String, Value> = HashMap::new();
        for m in messages {
            if m.get("role").and_then(|v| v.as_str()) == Some("tool") {
                if let Some(tool_call_id) = m.get("tool_call_id").and_then(|v| v.as_str()) {
                    if let Some(content) = m.get("content") {
                        tool_responses.insert(tool_call_id.to_string(), content.clone());
                    }
                }
            }
        }

        let has_multiple_messages = messages.len() > 1;

        for m in messages {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
            let content = m.get("content");

            if (role == "system" || role == "developer") && has_multiple_messages {
                if let Some(content) = content {
                    if let Some(text) = content.as_str() {
                        system_parts.push(json!({ "text": text }));
                    } else if content.get("type").and_then(|v| v.as_str()) == Some("text") {
                        if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
                            system_parts.push(json!({ "text": text }));
                        }
                    } else if let Some(arr) = content.as_array() {
                        for item in arr {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                system_parts.push(json!({ "text": text }));
                            }
                        }
                    }
                }
                continue;
            }

            if role == "user" || ((role == "system" || role == "developer") && !has_multiple_messages) {
                let mut parts = Vec::new();
                if let Some(content) = content {
                    if let Some(text) = content.as_str() {
                        parts.push(json!({ "text": text }));
                    } else if let Some(items) = content.as_array() {
                        for item in items {
                            match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "text" => {
                                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                        parts.push(json!({ "text": text }));
                                    }
                                }
                                "image_url" => {
                                    if let Some(url) = item.get("image_url").and_then(|v| v.get("url")).and_then(|v| v.as_str()) {
                                        if let Some((mime, data)) = parse_data_url(url) {
                                            parts.push(json!({
                                                "inlineData": { "mime_type": mime, "data": data },
                                                "thoughtSignature": GEMINI_CLI_THOUGHT_SIGNATURE
                                            }));
                                        }
                                    }
                                }
                                "file" => {}
                                _ => {}
                            }
                        }
                    }
                }
                contents.push(json!({ "role": "user", "parts": parts }));
                continue;
            }

            if role == "assistant" {
                let mut parts = Vec::new();
                if let Some(content) = content {
                    if let Some(text) = content.as_str() {
                        parts.push(json!({ "text": text }));
                    } else if let Some(items) = content.as_array() {
                        for item in items {
                            match item.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                                "text" => {
                                    if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                        parts.push(json!({ "text": text }));
                                    }
                                }
                                "image_url" => {
                                    if let Some(url) = item.get("image_url").and_then(|v| v.get("url")).and_then(|v| v.as_str()) {
                                        if let Some((mime, data)) = parse_data_url(url) {
                                            parts.push(json!({
                                                "inlineData": { "mime_type": mime, "data": data },
                                                "thoughtSignature": GEMINI_CLI_THOUGHT_SIGNATURE
                                            }));
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }

                let mut fids = Vec::new();
                if let Some(tcs) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tcs {
                        if tc.get("type").and_then(|v| v.as_str()) != Some("function") {
                            continue;
                        }
                        let fid = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let fname = tc.get("function").and_then(|v| v.get("name")).and_then(|v| v.as_str()).unwrap_or("");
                        let fargs_raw = tc.get("function").and_then(|v| v.get("arguments"));
                        let args_value = if let Some(args_str) = fargs_raw.and_then(|v| v.as_str()) {
                            serde_json::from_str(args_str).unwrap_or(Value::String(args_str.to_string()))
                        } else {
                            fargs_raw.cloned().unwrap_or(Value::Null)
                        };
                        parts.push(json!({
                            "functionCall": { "name": fname, "args": args_value },
                            "thoughtSignature": GEMINI_CLI_THOUGHT_SIGNATURE
                        }));
                        if !fid.is_empty() {
                            fids.push(fid.to_string());
                        }
                    }
                }

                contents.push(json!({ "role": "model", "parts": parts }));

                if !fids.is_empty() {
                    let mut tool_parts = Vec::new();
                    for fid in fids {
                        if let Some(name) = tc_id_to_name.get(&fid) {
                            let resp = tool_responses.get(&fid).cloned().unwrap_or(json!("{}"));
                            tool_parts.push(json!({
                                "functionResponse": {
                                    "name": name,
                                    "response": { "result": resp }
                                }
                            }));
                        }
                    }
                    if !tool_parts.is_empty() {
                        contents.push(json!({ "role": "user", "parts": tool_parts }));
                    }
                }
            }
        }
    }

    request.insert("contents".to_string(), json!(contents));
    if !system_parts.is_empty() {
        request.insert("systemInstruction".to_string(), json!({
            "role": "user",
            "parts": system_parts
        }));
    }

    if let Some(tools) = raw.get("tools").and_then(|v| v.as_array()) {
        let mut tools_node = Vec::new();
        let mut function_decls = Vec::new();
        for t in tools {
            if t.get("type").and_then(|v| v.as_str()) == Some("function") {
                if let Some(mut fn_obj) = t.get("function").cloned() {
                    if let Some(params) = fn_obj.get("parameters").cloned() {
                        if let Some(obj) = fn_obj.as_object_mut() {
                            obj.remove("parameters");
                        }
                        fn_obj["parametersJsonSchema"] = params;
                    } else {
                        fn_obj["parametersJsonSchema"] = json!({ "type": "object", "properties": {} });
                    }
                    if let Some(obj) = fn_obj.as_object_mut() {
                        obj.remove("strict");
                    }
                    function_decls.push(fn_obj);
                }
            }
            if let Some(gs) = t.get("google_search") {
                tools_node.push(json!({ "googleSearch": gs }));
            }
            if let Some(ce) = t.get("code_execution") {
                tools_node.push(json!({ "codeExecution": ce }));
            }
            if let Some(uc) = t.get("url_context") {
                tools_node.push(json!({ "urlContext": uc }));
            }
        }
        if !function_decls.is_empty() {
            tools_node.insert(0, json!({ "functionDeclarations": function_decls }));
        }
        if !tools_node.is_empty() {
            request.insert("tools".to_string(), json!(tools_node));
        }
    }

    request.insert("safetySettings".to_string(), json!(default_safety_settings()));

    json!({
        "project": "",
        "request": Value::Object(request),
        "model": model
    })
}

fn parse_data_url(url: &str) -> Option<(String, String)> {
    if !url.starts_with("data:") {
        return None;
    }
    let without_prefix = &url[5..];
    let mut parts = without_prefix.splitn(2, ';');
    let mime = parts.next()?.to_string();
    let rest = parts.next()?;
    if !rest.starts_with("base64,") {
        return None;
    }
    let data = rest[7..].to_string();
    if data.is_empty() {
        return None;
    }
    Some((mime, data))
}

/// Convert Gemini response to OpenAI format
pub fn gemini_to_openai_response(
    gemini_response: &Value,
    _model: &str,
    _request_id: &str,
) -> Value {
    if let Some(error) = gemini_response.get("error") {
        let message = error.get("message").and_then(|v| v.as_str()).unwrap_or("");
        let status = error.get("status").and_then(|v| v.as_str()).unwrap_or("api_error");
        let code = error.get("code").and_then(|v| v.as_i64()).unwrap_or(500);
        return json!({
            "error": {
                "message": message,
                "type": status,
                "code": code
            }
        });
    }

    let root = gemini_response.get("response").unwrap_or(gemini_response);
    convert_gemini_response_to_openai(root)
}

fn convert_gemini_response_to_openai(root: &Value) -> Value {
    let mut template = json!({
        "id": "",
        "object": "chat.completion",
        "created": 0,
        "model": "model",
        "choices": []
    });

    if let Some(model_version) = root.get("modelVersion").and_then(|v| v.as_str()) {
        template["model"] = json!(model_version);
    }

    if let Some(create_time) = root.get("createTime").and_then(|v| v.as_str()) {
        if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(create_time) {
            template["created"] = json!(dt.timestamp());
        }
    }

    if let Some(response_id) = root.get("responseId").and_then(|v| v.as_str()) {
        template["id"] = json!(response_id);
    }

    if let Some(usage) = root.get("usageMetadata") {
        if let Some(candidates) = usage.get("candidatesTokenCount").and_then(|v| v.as_i64()) {
            template["usage"]["completion_tokens"] = json!(candidates);
        }
        if let Some(total) = usage.get("totalTokenCount").and_then(|v| v.as_i64()) {
            template["usage"]["total_tokens"] = json!(total);
        }
        let prompt = usage.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        let thoughts = usage.get("thoughtsTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        template["usage"]["prompt_tokens"] = json!(prompt + thoughts);
        if thoughts > 0 {
            template["usage"]["completion_tokens_details"]["reasoning_tokens"] = json!(thoughts);
        }
        let cached = usage.get("cachedContentTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        if cached > 0 {
            template["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(cached);
        }
    }

    if let Some(candidates) = root.get("candidates").and_then(|v| v.as_array()) {
        for candidate in candidates {
            let mut choice = json!({
                "index": 0,
                "message": {
                    "role": "assistant",
                    "content": null,
                    "reasoning_content": null,
                    "tool_calls": null
                },
                "finish_reason": null,
                "native_finish_reason": null
            });

            if let Some(index) = candidate.get("index").and_then(|v| v.as_i64()) {
                choice["index"] = json!(index);
            }

            if let Some(finish_reason) = candidate.get("finishReason").and_then(|v| v.as_str()) {
                let lower = finish_reason.to_ascii_lowercase();
                choice["finish_reason"] = json!(lower);
                choice["native_finish_reason"] = json!(lower);
            }

            let mut has_function_call = false;

            if let Some(parts) = candidate
                .get("content")
                .and_then(|v| v.get("parts"))
                .and_then(|v| v.as_array())
            {
                for part in parts {
                    if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                        if part.get("thought").and_then(|v| v.as_bool()).unwrap_or(false) {
                            let current = choice["message"]["reasoning_content"].as_str().unwrap_or("");
                            choice["message"]["reasoning_content"] = json!(format!("{}{}", current, text));
                        } else {
                            let current = choice["message"]["content"].as_str().unwrap_or("");
                            choice["message"]["content"] = json!(format!("{}{}", current, text));
                        }
                        choice["message"]["role"] = json!("assistant");
                        continue;
                    }

                    if let Some(function_call) = part.get("functionCall") {
                        has_function_call = true;
                        if !choice["message"]["tool_calls"].is_array() {
                            choice["message"]["tool_calls"] = json!([]);
                        }
                        let name = function_call.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let counter = FUNCTION_CALL_ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
                        let nanos = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_nanos())
                            .unwrap_or(0);
                        let mut tool_call = json!({
                            "id": format!("{}-{}-{}", name, nanos, counter),
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": ""
                            }
                        });
                        if let Some(args) = function_call.get("args") {
                            let raw = serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string());
                            tool_call["function"]["arguments"] = json!(raw);
                        }
                        if let Some(arr) = choice["message"]["tool_calls"].as_array_mut() {
                            arr.push(tool_call);
                        }
                        choice["message"]["role"] = json!("assistant");
                        continue;
                    }

                    let inline_data = part.get("inlineData").or_else(|| part.get("inline_data"));
                    if let Some(inline_data) = inline_data {
                        let data = inline_data.get("data").and_then(|v| v.as_str()).unwrap_or("");
                        if data.is_empty() {
                            continue;
                        }
                        let mut mime_type = inline_data.get("mimeType").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        if mime_type.is_empty() {
                            mime_type = inline_data.get("mime_type").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        }
                        if mime_type.is_empty() {
                            mime_type = "image/png".to_string();
                        }
                        let image_url = format!("data:{};base64,{}", mime_type, data);
                        if !choice["message"]["images"].is_array() {
                            choice["message"]["images"] = json!([]);
                        }
                        let index = choice["message"]["images"].as_array().map(|a| a.len()).unwrap_or(0);
                        let image_payload = json!({
                            "type": "image_url",
                            "image_url": { "url": image_url },
                            "index": index
                        });
                        if let Some(arr) = choice["message"]["images"].as_array_mut() {
                            arr.push(image_payload);
                        }
                        choice["message"]["role"] = json!("assistant");
                    }
                }
            }

            if has_function_call {
                choice["finish_reason"] = json!("tool_calls");
                choice["native_finish_reason"] = json!("tool_calls");
            }

            if let Some(arr) = template["choices"].as_array_mut() {
                arr.push(choice);
            }
        }
    }

    template
}
