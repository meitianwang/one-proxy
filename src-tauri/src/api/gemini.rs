// Gemini API client for proxying requests

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const GEMINI_API_BASE: &str = "https://generativelanguage.googleapis.com/v1beta";

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

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GeminiGenerateRequest {
    pub contents: Vec<GeminiContent>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation_config: Option<GeminiGenerationConfig>,
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

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiResponse {
    pub candidates: Option<Vec<GeminiCandidate>>,
    pub error: Option<GeminiError>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiCandidate {
    pub content: GeminiContent,
    #[serde(rename = "finishReason")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GeminiError {
    pub code: i32,
    pub message: String,
    pub status: String,
}

impl GeminiClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn generate_content(
        &self,
        model: &str,
        request: GeminiGenerateRequest,
    ) -> Result<GeminiResponse> {
        let url = format!(
            "{}/models/{}:generateContent",
            GEMINI_API_BASE,
            model
        );

        let response = self
            .http_client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&request)
            .send()
            .await?;

        let status = response.status();
        let body: Value = response.json().await?;

        if !status.is_success() {
            if let Some(error) = body.get("error") {
                return Ok(GeminiResponse {
                    candidates: None,
                    error: serde_json::from_value(error.clone()).ok(),
                });
            }
            return Err(anyhow::anyhow!("Gemini API error: {}", body));
        }

        let gemini_response: GeminiResponse = serde_json::from_value(body)?;
        Ok(gemini_response)
    }

    pub async fn list_models(&self) -> Result<Value> {
        let url = format!("{}/models", GEMINI_API_BASE);

        let response = self
            .http_client
            .get(&url)
            .bearer_auth(&self.access_token)
            .send()
            .await?;

        let body: Value = response.json().await?;
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

/// Convert Gemini response to OpenAI format
pub fn gemini_to_openai_response(
    gemini_response: &GeminiResponse,
    model: &str,
    request_id: &str,
) -> Value {
    if let Some(error) = &gemini_response.error {
        return serde_json::json!({
            "error": {
                "message": error.message,
                "type": error.status,
                "code": error.code
            }
        });
    }

    let content = gemini_response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .map(|c| {
            c.content
                .parts
                .iter()
                .map(|p| p.text.clone())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let finish_reason = gemini_response
        .candidates
        .as_ref()
        .and_then(|c| c.first())
        .and_then(|c| c.finish_reason.clone())
        .map(|r| match r.as_str() {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "SAFETY" => "content_filter",
            _ => "stop",
        })
        .unwrap_or("stop");

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
            "prompt_tokens": 0,
            "completion_tokens": 0,
            "total_tokens": 0
        }
    })
}
