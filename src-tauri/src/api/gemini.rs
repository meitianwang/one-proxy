// Gemini API client for proxying requests
// Uses Cloud Code Assist endpoint for OAuth tokens (same as CLIProxyAPI)

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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

    /// Generate content using Cloud Code Assist endpoint (same as CLIProxyAPI)
    /// Request format matches CLIProxyAPI gemini_cli_executor.go
    pub async fn generate_content(
        &self,
        model: &str,
        request: GeminiGenerateRequest,
    ) -> Result<GeminiResponse> {
        // Use Cloud Code Assist endpoint like CLIProxyAPI gemini_cli_executor.go
        let url = format!(
            "{}/{}:generateContent",
            CODE_ASSIST_ENDPOINT,
            CODE_ASSIST_VERSION
        );

        // Build Cloud Code Assist request envelope (same as CLIProxyAPI)
        // Format: {"project":"","request":{"contents":[...],"safetySettings":[...]},"model":"gemini-2.5-pro"}
        let cli_request = GeminiCLIRequest {
            project: request.project.unwrap_or_default(),
            request: GeminiInnerRequest {
                contents: request.contents,
                generation_config: request.generation_config,
                system_instruction: None,
                safety_settings: Some(default_safety_settings()),
            },
            model: model.to_string(),
        };

        let request_json = serde_json::to_string(&cli_request)?;
        tracing::info!("Gemini request URL: {}", url);
        tracing::info!("Gemini request body: {}", request_json);

        let response = self
            .http_client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Content-Type", "application/json")
            .header("Accept", "application/json")
            .header("User-Agent", "google-api-nodejs-client/9.15.1")
            .header("X-Goog-Api-Client", "gl-node/22.17.0")
            .header("Client-Metadata", "ideType=IDE_UNSPECIFIED,platform=PLATFORM_UNSPECIFIED,pluginType=GEMINI")
            .json(&cli_request)
            .send()
            .await?;

        let status = response.status();
        let body_text = response.text().await?;
        tracing::info!("Gemini response status: {}", status);
        tracing::info!("Gemini response body: {}", body_text);

        let body: Value = serde_json::from_str(&body_text)?;

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

    let mut content_parts: Vec<String> = Vec::new();
    let mut thought_parts: Vec<String> = Vec::new();
    if let Some(candidates) = gemini_response.candidates.as_ref() {
        if let Some(first) = candidates.first() {
            if let Ok(value) = serde_json::to_value(&first.content) {
                if let Some(parts) = value.get("parts").and_then(|p| p.as_array()) {
                    for part in parts {
                        let text = part.get("text").and_then(|t| t.as_str()).unwrap_or("");
                        if text.is_empty() {
                            continue;
                        }
                        let is_thought = part
                            .get("thought")
                            .and_then(|v| v.as_bool())
                            .unwrap_or(false);
                        if is_thought {
                            thought_parts.push(text.to_string());
                        } else {
                            content_parts.push(text.to_string());
                        }
                    }
                }
            }
        }
    }

    let content = if !content_parts.is_empty() {
        content_parts.join("")
    } else {
        thought_parts.join("")
    };

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
