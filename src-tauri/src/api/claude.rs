// Claude API client for proxying requests

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
