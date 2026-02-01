// SSE streaming support for API responses

use axum::response::sse::{Event, Sse};
use futures::stream::Stream;
use serde_json::{json, Value};
use std::convert::Infallible;

/// Create an SSE stream for OpenAI-compatible streaming responses
pub fn create_openai_stream(
    chunks: Vec<String>,
    model: &str,
    request_id: &str,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let model = model.to_string();
    let request_id = request_id.to_string();

    let stream = async_stream::stream! {
        for (i, content) in chunks.into_iter().enumerate() {
            let chunk = json!({
                "id": format!("chatcmpl-{}", request_id),
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": model,
                "choices": [{
                    "index": 0,
                    "delta": {
                        "role": if i == 0 { Some("assistant") } else { None::<&str> },
                        "content": content
                    },
                    "finish_reason": null
                }]
            });

            yield Ok(Event::default().data(serde_json::to_string(&chunk).unwrap()));
        }

        // Send final chunk with finish_reason
        let final_chunk = json!({
            "id": format!("chatcmpl-{}", request_id),
            "object": "chat.completion.chunk",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "delta": {},
                "finish_reason": "stop"
            }]
        });

        yield Ok(Event::default().data(serde_json::to_string(&final_chunk).unwrap()));
        yield Ok(Event::default().data("[DONE]"));
    };

    Sse::new(stream)
}

/// Parse Gemini streaming response and convert to OpenAI format
pub fn gemini_stream_to_openai_chunk(
    gemini_data: &Value,
    model: &str,
    request_id: &str,
    is_first: bool,
) -> Value {
    let mut content_parts: Vec<String> = Vec::new();
    let mut thought_parts: Vec<String> = Vec::new();
    if let Some(parts) = gemini_data
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array())
    {
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

    let content = if !content_parts.is_empty() {
        content_parts.join("")
    } else {
        thought_parts.join("")
    };

    let finish_reason = gemini_data
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("finishReason"))
        .and_then(|r| r.as_str())
        .map(|r| match r {
            "STOP" => "stop",
            "MAX_TOKENS" => "length",
            "SAFETY" => "content_filter",
            _ => "stop",
        });

    json!({
        "id": format!("chatcmpl-{}", request_id),
        "object": "chat.completion.chunk",
        "created": chrono::Utc::now().timestamp(),
        "model": model,
        "choices": [{
            "index": 0,
            "delta": {
                "role": if is_first { Some("assistant") } else { None::<&str> },
                "content": content
            },
            "finish_reason": finish_reason
        }]
    })
}
