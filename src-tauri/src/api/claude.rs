// Claude API client for proxying requests

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const CLAUDE_API_BASE: &str = "https://api.anthropic.com/v1";

#[derive(Debug, Clone)]
pub struct ClaudeClient {
    access_token: String,
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
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn create_message(&self, request: ClaudeRequest) -> Result<ClaudeResponse> {
        let url = format!("{}/messages", CLAUDE_API_BASE);

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

fn extract_claude_text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => {
            let mut parts = Vec::new();
            for item in items {
                if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                    if !text.is_empty() {
                        parts.push(text.to_string());
                    }
                }
            }
            parts.join("\n")
        }
        _ => String::new(),
    }
}

pub fn claude_request_to_openai_chat(raw: &Value, model: &str) -> Value {
    let mut messages = Vec::new();

    if let Some(system) = raw.get("system") {
        let system_text = extract_claude_text(system);
        if !system_text.is_empty() {
            messages.push(json!({
                "role": "system",
                "content": system_text
            }));
        }
    }

    if let Some(msgs) = raw.get("messages").and_then(|v| v.as_array()) {
        for msg in msgs {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content").unwrap_or(&Value::Null);
            let text = extract_claude_text(content);
            messages.push(json!({
                "role": role,
                "content": text
            }));
        }
    }

    let mut out = json!({
        "model": model,
        "messages": messages
    });

    if let Some(v) = raw.get("stream") {
        out["stream"] = v.clone();
    }
    if let Some(v) = raw.get("temperature") {
        out["temperature"] = v.clone();
    }
    if let Some(v) = raw.get("max_tokens") {
        out["max_tokens"] = v.clone();
    }
    if let Some(v) = raw.get("top_p") {
        out["top_p"] = v.clone();
    }
    if let Some(v) = raw.get("top_k") {
        out["top_k"] = v.clone();
    }
    if let Some(v) = raw.get("reasoning_effort") {
        out["reasoning_effort"] = v.clone();
    }
    if let Some(v) = raw.get("stop") {
        out["stop"] = v.clone();
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
    let content_text = message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let mut content_blocks = Vec::new();
    if !content_text.is_empty() {
        content_blocks.push(json!({
            "type": "text",
            "text": content_text
        }));
    }

    let stop_reason = map_openai_finish_reason(
        choice
            .get("finish_reason")
            .and_then(|v| v.as_str()),
    );

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
