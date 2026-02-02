// Claude API client for proxying requests

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const CLAUDE_API_BASE: &str = "https://api.anthropic.com/v1";

#[derive(Debug, Clone)]
pub struct ClaudeClient {
    access_token: String,
    base_url: String,
    http_client: reqwest::Client,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeRequest {
    pub model: String,
    pub messages: Vec<ClaudeMessage>,
    pub max_tokens: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeResponse {
    pub id: Option<String>,
    pub content: Option<Vec<ClaudeContent>>,
    pub model: Option<String>,
    pub stop_reason: Option<String>,
    pub usage: Option<ClaudeUsage>,
    pub error: Option<ClaudeError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ClaudeError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
}

impl ClaudeClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            base_url: CLAUDE_API_BASE.to_string(),
            http_client: reqwest::Client::new(),
        }
    }

    pub fn new_with_base_url(access_token: String, base_url: impl Into<String>) -> Self {
        let mut base_url = base_url.into();
        while base_url.ends_with('/') {
            base_url.pop();
        }
        Self {
            access_token,
            base_url,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn create_message(&self, request: ClaudeRequest) -> Result<ClaudeResponse> {
        let url = format!("{}/messages", self.base_url);

        let response = self
            .http_client
            .post(&url)
            .header("x-api-key", &self.access_token)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;

        if !status.is_success() {
            if let Some(error) = body.get("error") {
                return Ok(ClaudeResponse {
                    id: None,
                    content: None,
                    model: None,
                    stop_reason: None,
                    usage: None,
                    error: serde_json::from_value(error.clone()).ok(),
                });
            }
            return Err(anyhow::anyhow!("Claude API error: {}", body));
        }

        let claude_response: ClaudeResponse = serde_json::from_value(body)?;
        Ok(claude_response)
    }
}

/// Convert OpenAI chat messages to Claude format
pub fn openai_to_claude_messages(
    messages: &[super::handlers::ChatMessage],
) -> (Vec<ClaudeMessage>, Option<String>) {
    let mut system_prompt = None;
    let mut claude_messages = Vec::new();

    for msg in messages {
        if msg.role == "system" {
            // Claude uses a separate system parameter
            system_prompt = Some(msg.content.clone());
        } else {
            claude_messages.push(ClaudeMessage {
                role: msg.role.clone(),
                content: msg.content.clone(),
            });
        }
    }

    (claude_messages, system_prompt)
}

/// Convert Claude response to OpenAI format
pub fn claude_to_openai_response(
    claude_response: &ClaudeResponse,
    model: &str,
    request_id: &str,
) -> Value {
    if let Some(error) = &claude_response.error {
        return serde_json::json!({
            "error": {
                "message": error.message,
                "type": error.error_type,
                "code": 500
            }
        });
    }

    let content = claude_response
        .content
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.text.clone())
        .unwrap_or_default();

    let finish_reason = claude_response
        .stop_reason
        .as_ref()
        .map(|r| match r.as_str() {
            "end_turn" => "stop",
            "max_tokens" => "length",
            "stop_sequence" => "stop",
            _ => "stop",
        })
        .unwrap_or("stop");

    let (prompt_tokens, completion_tokens) = claude_response
        .usage
        .as_ref()
        .map(|u| (u.input_tokens, u.output_tokens))
        .unwrap_or((0, 0));

    serde_json::json!({
        "id": format!("chatcmpl-{}", request_id),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}

/// Convert Claude response (as Value) to OpenAI format
pub fn claude_value_to_openai_response(
    claude_response: &Value,
    model: &str,
    request_id: &str,
) -> Value {
    // Check for error
    if let Some(error) = claude_response.get("error") {
        return serde_json::json!({
            "error": {
                "message": error.get("message").and_then(|v| v.as_str()).unwrap_or("Unknown error"),
                "type": error.get("type").and_then(|v| v.as_str()).unwrap_or("api_error"),
                "code": 500
            }
        });
    }

    // Extract content text
    let content = claude_response
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("text"))
        .and_then(|t| t.as_str())
        .unwrap_or("");

    // Extract finish reason
    let finish_reason = claude_response
        .get("stop_reason")
        .and_then(|v| v.as_str())
        .map(|r| match r {
            "end_turn" => "stop",
            "max_tokens" => "length",
            "stop_sequence" => "stop",
            _ => "stop",
        })
        .unwrap_or("stop");

    // Extract usage
    let usage = claude_response.get("usage");
    let prompt_tokens = usage
        .and_then(|u| u.get("input_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);
    let completion_tokens = usage
        .and_then(|u| u.get("output_tokens"))
        .and_then(|v| v.as_i64())
        .unwrap_or(0);

    serde_json::json!({
        "id": format!("chatcmpl-{}", request_id),
        "object": "chat.completion",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": content
            },
            "finish_reason": finish_reason
        }],
        "usage": {
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens,
            "total_tokens": prompt_tokens + completion_tokens
        }
    })
}

/// Convert Claude streaming event to OpenAI streaming chunk
pub fn claude_stream_to_openai_chunk(event: &Value, model: &str) -> Option<Value> {
    let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

    match event_type {
        "message_start" => {
            // First chunk with role
            Some(serde_json::json!({
                "id": event.get("message").and_then(|m| m.get("id")).and_then(|v| v.as_str()).unwrap_or("chatcmpl-stream"),
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": "assistant",
                        "content": ""
                    },
                    "finish_reason": null
                }]
            }))
        }
        "content_block_delta" => {
            // Text delta
            let empty = serde_json::json!({});
            let delta = event.get("delta").unwrap_or(&empty);
            let text = delta.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if text.is_empty() {
                return None;
            }
            Some(serde_json::json!({
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "content": text
                    },
                    "finish_reason": null
                }]
            }))
        }
        "message_delta" => {
            // Final chunk with finish_reason
            let stop_reason = event.get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .map(|r| match r {
                    "end_turn" => "stop",
                    "max_tokens" => "length",
                    "stop_sequence" => "stop",
                    _ => "stop",
                })
                .unwrap_or("stop");
            Some(serde_json::json!({
                "id": "chatcmpl-stream",
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {},
                    "finish_reason": stop_reason
                }]
            }))
        }
        _ => None,
    }
}

#[derive(Clone, Copy)]
pub enum ClaudeImageHandling {
    Drop,
    Base64Any,
    Base64TypeOnly,
    Base64AndUrl,
}

fn extract_claude_thinking_text(value: &Value) -> String {
    value
        .get("thinking")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("text").and_then(|v| v.as_str()))
        .unwrap_or("")
        .to_string()
}

fn extract_base64_image(source: &Value) -> Option<(String, String)> {
    let data = source
        .get("data")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .or_else(|| source.get("base64").and_then(|v| v.as_str()).filter(|v| !v.is_empty()))?;
    let mime_type = source
        .get("media_type")
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty())
        .or_else(|| source.get("mime_type").and_then(|v| v.as_str()).filter(|v| !v.is_empty()))
        .unwrap_or("application/octet-stream");
    Some((mime_type.to_string(), data.to_string()))
}

fn convert_claude_content_part(
    part: &Value,
    image_handling: ClaudeImageHandling,
) -> Option<Value> {
    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
    match part_type {
        "text" => {
            let text = part.get("text").and_then(|v| v.as_str()).unwrap_or("");
            if text.trim().is_empty() {
                None
            } else {
                Some(json!({ "type": "text", "text": text }))
            }
        }
        "image" => {
            match image_handling {
                ClaudeImageHandling::Drop => None,
                ClaudeImageHandling::Base64Any => {
                    let source = part.get("source")?;
                    let (mime, data) = extract_base64_image(source)?;
                    Some(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{};base64,{}", mime, data) }
                    }))
                }
                ClaudeImageHandling::Base64TypeOnly => {
                    let source = part.get("source")?;
                    let source_type = source.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    if source_type != "base64" {
                        return None;
                    }
                    let (mime, data) = extract_base64_image(source)?;
                    Some(json!({
                        "type": "image_url",
                        "image_url": { "url": format!("data:{};base64,{}", mime, data) }
                    }))
                }
                ClaudeImageHandling::Base64AndUrl => {
                    if let Some(source) = part.get("source") {
                        if let Some((mime, data)) = extract_base64_image(source) {
                            return Some(json!({
                                "type": "image_url",
                                "image_url": { "url": format!("data:{};base64,{}", mime, data) }
                            }));
                        }
                        if let Some(url) = source.get("url").and_then(|v| v.as_str()) {
                            if !url.is_empty() {
                                return Some(json!({
                                    "type": "image_url",
                                    "image_url": { "url": url }
                                }));
                            }
                        }
                    }
                    if let Some(url) = part.get("url").and_then(|v| v.as_str()) {
                        if !url.is_empty() {
                            return Some(json!({
                                "type": "image_url",
                                "image_url": { "url": url }
                            }));
                        }
                    }
                    None
                }
            }
        }
        _ => None,
    }
}

fn convert_claude_tool_result_content_to_string(content: &Value) -> String {
    match content {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                match item {
                    Value::String(s) => parts.push(s.clone()),
                    Value::Object(obj) => {
                        if let Some(Value::String(text)) = obj.get("text") {
                            parts.push(text.clone());
                        } else {
                            parts.push(item.to_string());
                        }
                    }
                    _ => parts.push(item.to_string()),
                }
            }
            let joined = parts.join("\n\n");
            if joined.trim().is_empty() {
                content.to_string()
            } else {
                joined
            }
        }
        Value::Object(obj) => {
            if let Some(Value::String(text)) = obj.get("text") {
                text.clone()
            } else {
                content.to_string()
            }
        }
        _ => content.to_string(),
    }
}

fn convert_budget_to_level(budget: i64) -> Option<&'static str> {
    match budget {
        -1 => Some("auto"),
        0 => Some("none"),
        1..=512 => Some("minimal"),
        513..=1024 => Some("low"),
        1025..=8192 => Some("medium"),
        8193..=24576 => Some("high"),
        24577..=i64::MAX => Some("xhigh"),
        _ => None,
    }
}

pub fn claude_request_to_openai_chat(
    raw: &Value,
    model: &str,
    image_handling: ClaudeImageHandling,
    guard_thinking: bool,
) -> Value {
    let mut out = json!({
        "model": model,
        "messages": []
    });

    if let Some(max_tokens) = raw.get("max_tokens") {
        out["max_tokens"] = max_tokens.clone();
    }
    if let Some(temp) = raw.get("temperature") {
        out["temperature"] = temp.clone();
    } else if let Some(top_p) = raw.get("top_p") {
        out["top_p"] = top_p.clone();
    }
    if let Some(stop_sequences) = raw.get("stop_sequences") {
        if let Some(arr) = stop_sequences.as_array() {
            let stops: Vec<Value> = arr.iter().cloned().collect();
            if stops.len() == 1 {
                out["stop"] = stops[0].clone();
            } else if !stops.is_empty() {
                out["stop"] = Value::Array(stops);
            }
        }
    }
    if let Some(stream) = raw.get("stream") {
        out["stream"] = stream.clone();
    }

    let model_lower = model.to_lowercase();
    let supports_thinking = model_lower.contains("-thinking") || model_lower.starts_with("claude-");
    let allow_thinking_map = !guard_thinking || supports_thinking;
    if guard_thinking {
        tracing::info!(
            "[Claude->OpenAI] guard_thinking enabled | model={} | supports_thinking={}",
            model,
            supports_thinking
        );
    }
    if allow_thinking_map {
        if let Some(thinking) = raw.get("thinking").and_then(|v| v.as_object()) {
            if let Some(Value::String(tt)) = thinking.get("type") {
                match tt.as_str() {
                    "enabled" => {
                        if let Some(budget) = thinking.get("budget_tokens").and_then(|v| v.as_i64()) {
                            if let Some(level) = convert_budget_to_level(budget) {
                                out["reasoning_effort"] = json!(level);
                            }
                        } else if let Some(level) = convert_budget_to_level(-1) {
                            out["reasoning_effort"] = json!(level);
                        }
                    }
                    "disabled" => {
                        if let Some(level) = convert_budget_to_level(0) {
                            out["reasoning_effort"] = json!(level);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let mut messages: Vec<Value> = Vec::new();

    if let Some(system) = raw.get("system") {
        let mut system_items = Vec::new();
        match system {
            Value::String(s) => {
                if !s.trim().is_empty() {
                    system_items.push(json!({ "type": "text", "text": s }));
                }
            }
            Value::Array(items) => {
                for item in items {
                    if let Some(part) = convert_claude_content_part(item, image_handling) {
                        system_items.push(part);
                    }
                }
            }
            _ => {}
        }
        if !system_items.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": system_items
            }));
        }
    }

    if let Some(msgs) = raw.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content").unwrap_or(&Value::Null);

            if let Some(parts) = content.as_array() {
                let mut content_items: Vec<Value> = Vec::new();
                let mut reasoning_parts: Vec<String> = Vec::new();
                let mut tool_calls: Vec<Value> = Vec::new();
                let mut tool_results: Vec<Value> = Vec::new();

                for part in parts {
                    let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match part_type {
                        "thinking" => {
                            if role == "assistant" {
                                let thinking_text = extract_claude_thinking_text(part);
                                if !thinking_text.trim().is_empty() {
                                    reasoning_parts.push(thinking_text);
                                }
                            }
                        }
                        "text" | "image" => {
                            if let Some(item) = convert_claude_content_part(part, image_handling) {
                                content_items.push(item);
                            }
                        }
                        "tool_use" => {
                            if role == "assistant" {
                                let id = part.get("id").and_then(|v| v.as_str()).unwrap_or("");
                                let name = part.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                let input = part.get("input").cloned().unwrap_or_else(|| json!({}));
                                let args = serde_json::to_string(&input).unwrap_or_else(|_| "{}".to_string());
                                tool_calls.push(json!({
                                    "id": id,
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        "arguments": args
                                    }
                                }));
                            }
                        }
                        "tool_result" => {
                            let tool_call_id = part.get("tool_use_id").and_then(|v| v.as_str()).unwrap_or("");
                            let tool_content = part.get("content").unwrap_or(&Value::Null);
                            let content_str = convert_claude_tool_result_content_to_string(tool_content);
                            tool_results.push(json!({
                                "role": "tool",
                                "tool_call_id": tool_call_id,
                                "content": content_str
                            }));
                        }
                        _ => {}
                    }
                }

                for tool_msg in tool_results {
                    messages.push(tool_msg);
                }

                let has_content = !content_items.is_empty();
                let has_reasoning = !reasoning_parts.is_empty();
                let has_tool_calls = !tool_calls.is_empty();

                if role == "assistant" {
                    if has_content || has_reasoning || has_tool_calls {
                        let mut msg_obj = serde_json::Map::new();
                        msg_obj.insert("role".to_string(), json!("assistant"));
                        if has_content {
                            msg_obj.insert("content".to_string(), Value::Array(content_items));
                        } else {
                            msg_obj.insert("content".to_string(), json!(""));
                        }
                        if has_reasoning {
                            msg_obj.insert(
                                "reasoning_content".to_string(),
                                json!(reasoning_parts.join("\n\n")),
                            );
                        }
                        if has_tool_calls {
                            msg_obj.insert("tool_calls".to_string(), Value::Array(tool_calls));
                        }
                        messages.push(Value::Object(msg_obj));
                    }
                } else if has_content {
                    messages.push(json!({
                        "role": role,
                        "content": content_items
                    }));
                }
            } else if let Some(text) = content.as_str() {
                messages.push(json!({
                    "role": role,
                    "content": text
                }));
            }
        }
    }

    if !messages.is_empty() {
        out["messages"] = Value::Array(messages);
    }

    if let Some(tools) = raw.get("tools").and_then(|v| v.as_array()) {
        let mut tool_defs = Vec::new();
        for tool in tools {
            let name = tool.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if name.is_empty() {
                continue;
            }
            let description = tool.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let mut tool_def = json!({
                "type": "function",
                "function": {
                    "name": name,
                    "description": description
                }
            });
            if let Some(input_schema) = tool.get("input_schema") {
                tool_def["function"]["parameters"] = input_schema.clone();
            }
            tool_defs.push(tool_def);
        }
        if !tool_defs.is_empty() {
            out["tools"] = Value::Array(tool_defs);
        }
    }

    if let Some(tool_choice) = raw.get("tool_choice") {
        match tool_choice.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "auto" => {
                out["tool_choice"] = json!("auto");
            }
            "any" => {
                out["tool_choice"] = json!("required");
            }
            "tool" => {
                if let Some(name) = tool_choice.get("name").and_then(|v| v.as_str()) {
                    out["tool_choice"] = json!({
                        "type": "function",
                        "function": { "name": name }
                    });
                }
            }
            _ => {
                out["tool_choice"] = json!("auto");
            }
        }
    }

    if let Some(user) = raw.get("user") {
        out["user"] = user.clone();
    }

    out
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

pub fn openai_to_claude_response(
    openai_response: &Value,
    model: &str,
    request_id: &str,
) -> Value {
    openai_to_claude_response_with_options(openai_response, model, request_id, false)
}

pub fn openai_to_claude_response_with_options(
    openai_response: &Value,
    model: &str,
    request_id: &str,
    reasoning_as_text: bool,
) -> Value {
    if let Some(error) = openai_response.get("error") {
        return json!({
            "error": error
        });
    }

    let choice = openai_response
        .get("choices")
        .and_then(|v| v.as_array())
        .and_then(|v| v.first())
        .cloned()
        .unwrap_or_else(|| json!({}));

    let message = choice.get("message").cloned().unwrap_or_else(|| json!({}));
    let mut content_blocks: Vec<Value> = Vec::new();
    let mut has_tool_call = false;

    if let Some(content) = message.get("content") {
        match content {
            Value::String(text) => {
                if !text.is_empty() {
                    content_blocks.push(json!({
                        "type": "text",
                        "text": text
                    }));
                }
            }
            Value::Array(items) => {
                for item in items {
                    let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    match item_type {
                        "text" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    content_blocks.push(json!({ "type": "text", "text": text }));
                                }
                            }
                        }
                        "reasoning" => {
                            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                                if !text.is_empty() {
                                    content_blocks.push(json!({ "type": "thinking", "thinking": text }));
                                }
                            }
                        }
                        "tool_calls" => {
                            if let Some(tool_calls) = item.get("tool_calls").and_then(|v| v.as_array()) {
                                for tool_call in tool_calls {
                                    if let Some(tool_use) = convert_openai_tool_call(tool_call) {
                                        has_tool_call = true;
                                        content_blocks.push(tool_use);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    if let Some(reasoning) = message.get("reasoning_content").and_then(|v| v.as_str()) {
        if !reasoning.is_empty() {
            if reasoning_as_text {
                content_blocks.push(json!({ "type": "text", "text": reasoning }));
            } else {
                content_blocks.push(json!({ "type": "thinking", "thinking": reasoning }));
            }
        }
    }

    if let Some(tool_calls) = message.get("tool_calls").and_then(|v| v.as_array()) {
        for tool_call in tool_calls {
            if let Some(tool_use) = convert_openai_tool_call(tool_call) {
                has_tool_call = true;
                content_blocks.push(tool_use);
            }
        }
    }

    let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());
    let mut stop_reason = map_openai_finish_reason(finish_reason);
    if stop_reason.is_none() {
        stop_reason = if has_tool_call { Some("tool_use") } else { Some("end_turn") };
    }

    let (input_tokens, output_tokens) = openai_response
        .get("usage")
        .and_then(|v| v.as_object())
        .map(|u| {
            let prompt = u
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            let completion = u
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as u32;
            (prompt, completion)
        })
        .unwrap_or((0, 0));

    json!({
        "id": format!("msg_{}", request_id),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content_blocks,
        "stop_reason": stop_reason,
        "stop_sequence": null,
        "usage": {
            "input_tokens": input_tokens,
            "output_tokens": output_tokens
        }
    })
}

fn convert_openai_tool_call(tool_call: &Value) -> Option<Value> {
    let id = tool_call.get("id").and_then(|v| v.as_str()).unwrap_or("");
    let name = tool_call
        .get("function")
        .and_then(|v| v.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let args = tool_call
        .get("function")
        .and_then(|v| v.get("arguments"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if id.is_empty() && name.is_empty() {
        return None;
    }

    let input = if !args.is_empty() {
        serde_json::from_str::<Value>(args).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    Some(json!({
        "type": "tool_use",
        "id": id,
        "name": name,
        "input": input
    }))
}
