// Proxy module - handles request routing and translation

pub mod router;
pub mod translator;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Provider {
    Gemini,
    Claude,
    Codex,
    OpenAICompat(String),
}

#[derive(Debug, Clone)]
pub struct ProxyRequest {
    pub provider: Provider,
    pub model: String,
    pub body: serde_json::Value,
    pub stream: bool,
}

#[derive(Debug, Clone)]
pub struct ProxyResponse {
    pub status: u16,
    pub body: serde_json::Value,
}

pub async fn route_request(request: ProxyRequest) -> Result<ProxyResponse> {
    // TODO: Implement request routing logic
    // 1. Select credential based on provider and model
    // 2. Translate request format if needed
    // 3. Forward to upstream
    // 4. Translate response format if needed
    // 5. Return response

    Err(anyhow::anyhow!("Proxy routing not yet implemented"))
}
