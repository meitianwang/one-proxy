use anyhow::{anyhow, Result};
use axum::response::sse::Event;
use futures::{Stream, StreamExt};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::convert::Infallible;
use uuid::Uuid;

const CODEX_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const DEFAULT_USER_AGENT: &str = "codex_cli_rs/0.101.0 (Mac OS 26.0.1; arm64) Apple_Terminal/464";

#[derive(Debug, Clone)]
pub struct CodexClient {
    access_token: String,
    http_client: reqwest::Client,
}

impl CodexClient {
    pub fn new(access_token: String) -> Self {
        Self {
            access_token,
            http_client: reqwest::Client::new(),
        }
    }

    pub async fn stream_responses(
        &self,
        payload: &Value,
        stream: bool,
    ) -> Result<reqwest::Response> {
        let url = format!("{}/responses", CODEX_BASE_URL.trim_end_matches('/'));
        let mut req = self
            .http_client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", self.access_token))
            .header("Version", "0.21.0")
            .header("Openai-Beta", "responses=experimental")
            .header("Session_id", Uuid::new_v4().to_string())
            .header("User-Agent", DEFAULT_USER_AGENT)
            .header("Connection", "Keep-Alive")
            .header("Originator", "codex_cli_rs")
            .json(payload);

        req = if stream {
            req.header("Accept", "text/event-stream")
        } else {
            req.header("Accept", "application/json")
        };

        let response = req.send().await?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("Codex request failed: {} {}", status, body));
        }
        Ok(response)
    }
}

pub fn normalize_responses_websocket_request(
    raw: &Value,
    last_request: Option<&Value>,
    last_response_output: &Value,
    allow_incremental_previous_response: bool,
) -> Result<(Value, Value)> {
    match raw.get("type").and_then(|v| v.as_str()).unwrap_or("") {
        "response.create" => {
            if let Some(previous) = last_request {
                normalize_response_subsequent_request(
                    raw,
                    previous,
                    last_response_output,
                    allow_incremental_previous_response,
                )
            } else {
                normalize_response_create_request(raw)
            }
        }
        "response.append" => {
            let previous = last_request
                .ok_or_else(|| anyhow!("websocket request received before response.create"))?;
            normalize_response_subsequent_request(
                raw,
                previous,
                last_response_output,
                allow_incremental_previous_response,
            )
        }
        other => Err(anyhow!("unsupported websocket request type: {}", other)),
    }
}

pub fn should_handle_responses_websocket_prewarm(
    raw: &Value,
    last_request: Option<&Value>,
    allow_incremental_previous_response: bool,
) -> bool {
    if allow_incremental_previous_response || last_request.is_some() {
        return false;
    }

    raw.get("type").and_then(|v| v.as_str()) == Some("response.create")
        && raw.get("generate").and_then(|v| v.as_bool()) == Some(false)
}

pub fn synthetic_responses_websocket_prewarm_payloads(request: &Value) -> Result<Vec<String>> {
    let response_id = format!("resp_prewarm_{}", Uuid::new_v4());
    let created_at = chrono::Utc::now().timestamp();
    let model_name = request
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .to_string();

    let mut created = json!({
        "type": "response.created",
        "sequence_number": 0,
        "response": {
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": "in_progress",
            "background": false,
            "error": null,
            "output": []
        }
    });
    if !model_name.is_empty() {
        created["response"]["model"] = json!(model_name);
    }

    let mut completed = json!({
        "type": "response.completed",
        "sequence_number": 1,
        "response": {
            "id": response_id,
            "object": "response",
            "created_at": created_at,
            "status": "completed",
            "background": false,
            "error": null,
            "output": [],
            "usage": {
                "input_tokens": 0,
                "output_tokens": 0,
                "total_tokens": 0
            }
        }
    });
    if !model_name.is_empty() {
        completed["response"]["model"] = json!(model_name);
    }

    Ok(vec![created.to_string(), completed.to_string()])
}

pub fn response_completed_output(payload: &Value) -> Value {
    payload
        .get("response")
        .and_then(|v| v.get("output"))
        .filter(|v| v.is_array())
        .cloned()
        .unwrap_or_else(|| json!([]))
}

fn normalize_response_create_request(raw: &Value) -> Result<(Value, Value)> {
    let mut normalized = raw.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
    }
    normalized["stream"] = json!(true);
    if !normalized.get("input").is_some() {
        normalized["input"] = json!([]);
    }

    let model_name = normalized
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim();
    if model_name.is_empty() {
        return Err(anyhow!("missing model in response.create request"));
    }

    Ok((normalized.clone(), normalized))
}

fn normalize_response_subsequent_request(
    raw: &Value,
    last_request: &Value,
    last_response_output: &Value,
    allow_incremental_previous_response: bool,
) -> Result<(Value, Value)> {
    let next_input = raw
        .get("input")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("websocket request requires array field: input"))?;

    if allow_incremental_previous_response
        && raw
            .get("previous_response_id")
            .and_then(|v| v.as_str())
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    {
        let mut normalized = raw.clone();
        if let Some(obj) = normalized.as_object_mut() {
            obj.remove("type");
        }
        if normalized.get("model").is_none() {
            if let Some(model) = last_request.get("model") {
                normalized["model"] = model.clone();
            }
        }
        if normalized.get("instructions").is_none() {
            if let Some(instructions) = last_request.get("instructions") {
                normalized["instructions"] = instructions.clone();
            }
        }
        normalized["stream"] = json!(true);
        return Ok((normalized.clone(), normalized));
    }

    let mut merged = last_request
        .get("input")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(previous_output) = last_response_output.as_array() {
        merged.extend(previous_output.iter().cloned());
    }
    merged.extend(next_input.iter().cloned());

    let mut normalized = raw.clone();
    if let Some(obj) = normalized.as_object_mut() {
        obj.remove("type");
        obj.remove("previous_response_id");
    }
    normalized["input"] = Value::Array(merged);
    if normalized.get("model").is_none() {
        if let Some(model) = last_request.get("model") {
            normalized["model"] = model.clone();
        }
    }
    if normalized.get("instructions").is_none() {
        if let Some(instructions) = last_request.get("instructions") {
            normalized["instructions"] = instructions.clone();
        }
    }
    normalized["stream"] = json!(true);

    Ok((normalized.clone(), normalized))
}

pub fn openai_to_codex_request(raw: &Value, model: &str, stream: bool) -> Value {
    let mut out = json!({
        "instructions": "",
        "stream": stream,
        "parallel_tool_calls": true,
        "reasoning": {
            "effort": "medium",
            "summary": "auto"
        },
        "include": ["reasoning.encrypted_content"],
        "model": model,
        "input": [],
        "store": false
    });

    if let Some(re) = raw.get("reasoning_effort") {
        if let Some(reasoning) = out.get_mut("reasoning") {
            reasoning["effort"] = re.clone();
        }
    }

    let original_tool_name_map = build_short_name_map_from_tools(raw);

    if let Some(messages) = raw.get("messages").and_then(|v| v.as_array()) {
        let input = out
            .get_mut("input")
            .and_then(|v| v.as_array_mut())
            .expect("input array");

        for m in messages {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("");
            if role == "tool" {
                let tool_call_id = m.get("tool_call_id").and_then(|v| v.as_str()).unwrap_or("");
                let content = m.get("content").unwrap_or(&Value::Null);
                let output = string_from_json_value(content);
                let func_output = json!({
                    "type": "function_call_output",
                    "call_id": tool_call_id,
                    "output": output
                });
                input.push(func_output);
                continue;
            }

            let role_value = if role == "system" { "developer" } else { role };
            let mut msg = json!({
                "type": "message",
                "role": role_value,
                "content": []
            });

            if let Some(content_arr) = msg.get_mut("content").and_then(|v| v.as_array_mut()) {
                let content = m.get("content").unwrap_or(&Value::Null);
                if let Some(text) = content.as_str() {
                    if !text.is_empty() {
                        let part_type = if role == "assistant" {
                            "output_text"
                        } else {
                            "input_text"
                        };
                        content_arr.push(json!({ "type": part_type, "text": text }));
                    }
                } else if let Some(items) = content.as_array() {
                    for item in items {
                        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
                        match item_type {
                            "text" => {
                                let part_type = if role == "assistant" {
                                    "output_text"
                                } else {
                                    "input_text"
                                };
                                let text = item.get("text").and_then(|v| v.as_str()).unwrap_or("");
                                content_arr.push(json!({ "type": part_type, "text": text }));
                            }
                            "image_url" => {
                                if role == "user" {
                                    if let Some(url) = item
                                        .get("image_url")
                                        .and_then(|v| v.get("url"))
                                        .and_then(|v| v.as_str())
                                    {
                                        content_arr.push(
                                            json!({ "type": "input_image", "image_url": url }),
                                        );
                                    }
                                }
                            }
                            "file" => {
                                if role == "user" {
                                    let file_data = item
                                        .get("file")
                                        .and_then(|v| v.get("file_data"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    let filename = item
                                        .get("file")
                                        .and_then(|v| v.get("filename"))
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("");
                                    if !file_data.is_empty() {
                                        let mut part = json!({
                                            "type": "input_file",
                                            "file_data": file_data,
                                        });
                                        if !filename.is_empty() {
                                            part["filename"] = json!(filename);
                                        }
                                        content_arr.push(part);
                                    }
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }

            input.push(msg);

            if role == "assistant" {
                if let Some(tool_calls) = m.get("tool_calls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        if tc.get("type").and_then(|v| v.as_str()) != Some("function") {
                            continue;
                        }
                        let call_id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let mut name = tc
                            .get("function")
                            .and_then(|v| v.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        if let Some(short) = original_tool_name_map.get(&name) {
                            name = short.clone();
                        } else {
                            name = shorten_name_if_needed(&name);
                        }
                        let args_value = tc
                            .get("function")
                            .and_then(|v| v.get("arguments"))
                            .unwrap_or(&Value::Null);
                        let args = string_from_json_value(args_value);
                        let func_call = json!({
                            "type": "function_call",
                            "call_id": call_id,
                            "name": name,
                            "arguments": args
                        });
                        input.push(func_call);
                    }
                }
            }
        }
    }

    let response_format = raw.get("response_format");
    let text_settings = raw.get("text");
    if let Some(rf) = response_format {
        ensure_text_object(&mut out);
        if let Some(rf_type) = rf.get("type").and_then(|v| v.as_str()) {
            match rf_type {
                "text" => {
                    set_text_format(&mut out, json!({ "type": "text" }));
                }
                "json_schema" => {
                    let mut format = json!({ "type": "json_schema" });
                    if let Some(js) = rf.get("json_schema") {
                        if let Some(name) = js.get("name") {
                            format["name"] = name.clone();
                        }
                        if let Some(strict) = js.get("strict") {
                            format["strict"] = strict.clone();
                        }
                        if let Some(schema) = js.get("schema") {
                            format["schema"] = schema.clone();
                        }
                    }
                    set_text_format(&mut out, format);
                }
                _ => {}
            }
        }
        if let Some(text) = text_settings {
            if let Some(verbosity) = text.get("verbosity") {
                if let Some(obj) = out.get_mut("text") {
                    obj["verbosity"] = verbosity.clone();
                }
            }
        }
    } else if let Some(text) = text_settings {
        if let Some(verbosity) = text.get("verbosity") {
            ensure_text_object(&mut out);
            if let Some(obj) = out.get_mut("text") {
                obj["verbosity"] = verbosity.clone();
            }
        }
    }

    if let Some(tools) = raw.get("tools").and_then(|v| v.as_array()) {
        let mut out_tools: Vec<Value> = Vec::new();
        for t in tools {
            let tool_type = t.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if !tool_type.is_empty() && tool_type != "function" && t.is_object() {
                out_tools.push(t.clone());
                continue;
            }
            if tool_type == "function" {
                let mut item = json!({ "type": "function" });
                if let Some(fn_obj) = t.get("function") {
                    if let Some(name) = fn_obj.get("name").and_then(|v| v.as_str()) {
                        let mut n = name.to_string();
                        if let Some(short) = original_tool_name_map.get(&n) {
                            n = short.clone();
                        } else {
                            n = shorten_name_if_needed(&n);
                        }
                        item["name"] = json!(n);
                    }
                    if let Some(desc) = fn_obj.get("description") {
                        item["description"] = desc.clone();
                    }
                    if let Some(params) = fn_obj.get("parameters") {
                        item["parameters"] = params.clone();
                    }
                    if let Some(strict) = fn_obj.get("strict") {
                        item["strict"] = strict.clone();
                    }
                }
                out_tools.push(item);
            }
        }
        if !out_tools.is_empty() {
            out["tools"] = json!(out_tools);
        }
    }

    if let Some(tool_choice) = raw.get("tool_choice") {
        if tool_choice.is_string() {
            out["tool_choice"] = tool_choice.clone();
        } else if tool_choice.is_object() {
            let tc_type = tool_choice
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if tc_type == "function" {
                let mut name = tool_choice
                    .get("function")
                    .and_then(|v| v.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if !name.is_empty() {
                    if let Some(short) = original_tool_name_map.get(&name) {
                        name = short.clone();
                    } else {
                        name = shorten_name_if_needed(&name);
                    }
                }
                let mut choice = json!({ "type": "function" });
                if !name.is_empty() {
                    choice["name"] = json!(name);
                }
                out["tool_choice"] = choice;
            } else if !tc_type.is_empty() {
                out["tool_choice"] = tool_choice.clone();
            }
        }
    }

    out
}

pub fn openai_responses_to_codex_request(raw: &Value, model: &str) -> Value {
    let mut out = raw.clone();

    if let Some(input_text) = out.get("input").and_then(|v| v.as_str()) {
        out["input"] = json!([{
            "type": "message",
            "role": "user",
            "content": [{
                "type": "input_text",
                "text": input_text
            }]
        }]);
    }

    out["model"] = json!(model);
    out["stream"] = json!(true);
    out["store"] = json!(false);
    out["parallel_tool_calls"] = json!(true);
    out["include"] = json!(["reasoning.encrypted_content"]);

    if let Some(service_tier) = out.get("service_tier").and_then(|v| v.as_str()) {
        if service_tier != "priority" {
            if let Some(obj) = out.as_object_mut() {
                obj.remove("service_tier");
            }
        }
    }

    if let Some(obj) = out.as_object_mut() {
        for key in [
            "max_output_tokens",
            "max_completion_tokens",
            "temperature",
            "top_p",
            "truncation",
            "context_management",
            "user",
        ] {
            obj.remove(key);
        }
    }

    convert_system_role_to_developer(&mut out);
    out
}

pub fn codex_stream_to_openai_chunks(
    response: reqwest::Response,
    original_request: Value,
) -> impl Stream<Item = String> {
    let reverse_map = build_reverse_map_from_original_openai(&original_request);
    async_stream::stream! {
        let mut state = CodexStreamState {
            response_id: String::new(),
            created_at: 0,
            model: String::new(),
            function_call_index: -1,
            has_received_arguments_delta: false,
            has_tool_call_announced: false,
            reverse_tool_names: reverse_map,
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
                for chunk in convert_codex_stream_chunk(data, &mut state) {
                    yield chunk;
                }
            }
        }

        yield "[DONE]".to_string();
    }
}

pub fn codex_stream_to_openai_events(
    response: reqwest::Response,
    original_request: Value,
) -> impl Stream<Item = Result<Event, Infallible>> {
    codex_stream_to_openai_chunks(response, original_request)
        .map(|chunk| Ok(Event::default().data(chunk)))
}

pub fn codex_stream_to_openai_responses_events(
    response: reqwest::Response,
) -> impl Stream<Item = Result<Event, Infallible>> {
    async_stream::stream! {
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
                yield Ok(Event::default().data(data.to_string()));
            }
        }
    }
}

pub async fn collect_non_stream_response(
    response: reqwest::Response,
    original_request: &Value,
) -> Result<Value> {
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();
    let mut completed: Option<Value> = None;

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
                if parsed.get("type").and_then(|v| v.as_str()) == Some("response.completed") {
                    completed = Some(parsed);
                    break;
                }
            }
        }
        if completed.is_some() {
            break;
        }
    }

    let completed = completed.ok_or_else(|| anyhow!("stream closed before response.completed"))?;
    codex_completed_event_to_openai(&completed, original_request)
        .ok_or_else(|| anyhow!("invalid response.completed payload"))
}

pub async fn collect_non_stream_responses_response(response: reqwest::Response) -> Result<Value> {
    let mut buffer = String::new();
    let mut stream = response.bytes_stream();
    let mut completed: Option<Value> = None;

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
                if parsed.get("type").and_then(|v| v.as_str()) == Some("response.completed") {
                    completed = Some(parsed);
                    break;
                }
            }
        }
        if completed.is_some() {
            break;
        }
    }

    let completed = completed.ok_or_else(|| anyhow!("stream closed before response.completed"))?;
    completed
        .get("response")
        .cloned()
        .ok_or_else(|| anyhow!("invalid response.completed payload"))
}

struct CodexStreamState {
    response_id: String,
    created_at: i64,
    model: String,
    function_call_index: i32,
    has_received_arguments_delta: bool,
    has_tool_call_announced: bool,
    reverse_tool_names: HashMap<String, String>,
}

fn convert_codex_stream_chunk(data: &str, state: &mut CodexStreamState) -> Vec<String> {
    let parsed: Value = match serde_json::from_str(data) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let data_type = parsed.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if data_type == "response.created" {
        if let Some(resp) = parsed.get("response") {
            if let Some(id) = resp.get("id").and_then(|v| v.as_str()) {
                state.response_id = id.to_string();
            }
            if let Some(created_at) = resp.get("created_at").and_then(|v| v.as_i64()) {
                state.created_at = created_at;
            }
            if let Some(model) = resp.get("model").and_then(|v| v.as_str()) {
                state.model = model.to_string();
            }
        }
        return Vec::new();
    }

    let mut template = json!({
        "id": "",
        "object": "chat.completion.chunk",
        "created": 12345,
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

    if let Some(model) = parsed.get("model").and_then(|v| v.as_str()) {
        template["model"] = json!(model);
    } else if !state.model.is_empty() {
        template["model"] = json!(state.model.clone());
    }
    template["created"] = json!(state.created_at);
    template["id"] = json!(state.response_id.clone());

    apply_usage_to_openai(
        &mut template,
        parsed.get("response").and_then(|v| v.get("usage")),
    );

    match data_type {
        "response.reasoning_summary_text.delta" => {
            if let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) {
                template["choices"][0]["delta"]["role"] = json!("assistant");
                template["choices"][0]["delta"]["reasoning_content"] = json!(delta);
            }
        }
        "response.reasoning_summary_text.done" => {
            template["choices"][0]["delta"]["role"] = json!("assistant");
            template["choices"][0]["delta"]["reasoning_content"] = json!("\n\n");
        }
        "response.output_text.delta" => {
            if let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) {
                template["choices"][0]["delta"]["role"] = json!("assistant");
                template["choices"][0]["delta"]["content"] = json!(delta);
            }
        }
        "response.completed" => {
            let finish_reason = if state.function_call_index != -1 {
                "tool_calls"
            } else {
                "stop"
            };
            template["choices"][0]["finish_reason"] = json!(finish_reason);
            template["choices"][0]["native_finish_reason"] = json!(finish_reason);
        }
        "response.output_item.added" => {
            let Some(item) = parsed.get("item") else {
                return Vec::new();
            };
            if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
                return Vec::new();
            }

            state.function_call_index += 1;
            state.has_received_arguments_delta = false;
            state.has_tool_call_announced = true;

            let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
            let name = restore_original_tool_name(
                item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                &state.reverse_tool_names,
            );

            template["choices"][0]["delta"]["role"] = json!("assistant");
            template["choices"][0]["delta"]["tool_calls"] = json!([{
                "index": state.function_call_index,
                "id": call_id,
                "type": "function",
                "function": {
                    "name": name,
                    "arguments": ""
                }
            }]);
        }
        "response.function_call_arguments.delta" => {
            if state.function_call_index < 0 {
                return Vec::new();
            }
            let Some(delta) = parsed.get("delta").and_then(|v| v.as_str()) else {
                return Vec::new();
            };
            state.has_received_arguments_delta = true;
            template["choices"][0]["delta"]["tool_calls"] = json!([{
                "index": state.function_call_index,
                "function": {
                    "arguments": delta
                }
            }]);
        }
        "response.function_call_arguments.done" => {
            if state.function_call_index < 0 || state.has_received_arguments_delta {
                return Vec::new();
            }
            let args = parsed
                .get("arguments")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            template["choices"][0]["delta"]["tool_calls"] = json!([{
                "index": state.function_call_index,
                "function": {
                    "arguments": args
                }
            }]);
        }
        "response.output_item.done" => {
            if let Some(item) = parsed.get("item") {
                if item.get("type").and_then(|v| v.as_str()) != Some("function_call") {
                    return Vec::new();
                }
                if state.has_tool_call_announced {
                    state.has_tool_call_announced = false;
                    return Vec::new();
                }
                state.function_call_index += 1;
                template["choices"][0]["delta"]["tool_calls"] = json!([]);
                let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                let name = restore_original_tool_name(
                    item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    &state.reverse_tool_names,
                );
                let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                let tool_call = json!({
                    "index": state.function_call_index,
                    "id": call_id,
                    "type": "function",
                    "function": {
                        "name": name,
                        "arguments": args
                    }
                });
                if let Some(arr) = template["choices"][0]["delta"]["tool_calls"].as_array_mut() {
                    arr.push(tool_call);
                }
                template["choices"][0]["delta"]["role"] = json!("assistant");
            } else {
                return Vec::new();
            }
        }
        _ => return Vec::new(),
    }

    vec![template.to_string()]
}

pub fn codex_completed_event_to_openai(event: &Value, original_request: &Value) -> Option<Value> {
    if event.get("type").and_then(|v| v.as_str()) != Some("response.completed") {
        return None;
    }
    let response = event.get("response")?;
    let mut template = json!({
        "id": "",
        "object": "chat.completion",
        "created": 0,
        "model": "model",
        "choices": [{
            "index": 0,
            "message": {
                "role": "assistant",
                "content": null,
                "reasoning_content": null,
                "tool_calls": null
            },
            "finish_reason": null,
            "native_finish_reason": null
        }]
    });

    if let Some(model) = response.get("model").and_then(|v| v.as_str()) {
        template["model"] = json!(model);
    }
    if let Some(created_at) = response.get("created_at").and_then(|v| v.as_i64()) {
        template["created"] = json!(created_at);
    } else {
        template["created"] = json!(chrono::Utc::now().timestamp());
    }
    if let Some(id) = response.get("id").and_then(|v| v.as_str()) {
        template["id"] = json!(id);
    }

    apply_usage_to_openai(&mut template, response.get("usage"));

    let reverse_map = build_reverse_map_from_original_openai(original_request);
    let mut content_text = String::new();
    let mut reasoning_text = String::new();
    let mut tool_calls: Vec<Value> = Vec::new();

    if let Some(output) = response.get("output").and_then(|v| v.as_array()) {
        for item in output {
            let output_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match output_type {
                "reasoning" => {
                    if let Some(summary) = item.get("summary").and_then(|v| v.as_array()) {
                        for summary_item in summary {
                            if summary_item.get("type").and_then(|v| v.as_str())
                                == Some("summary_text")
                            {
                                if let Some(text) =
                                    summary_item.get("text").and_then(|v| v.as_str())
                                {
                                    reasoning_text = text.to_string();
                                    break;
                                }
                            }
                        }
                    }
                }
                "message" => {
                    if let Some(content) = item.get("content").and_then(|v| v.as_array()) {
                        for content_item in content {
                            if content_item.get("type").and_then(|v| v.as_str())
                                == Some("output_text")
                            {
                                if let Some(text) =
                                    content_item.get("text").and_then(|v| v.as_str())
                                {
                                    content_text = text.to_string();
                                    break;
                                }
                            }
                        }
                    }
                }
                "function_call" => {
                    let call_id = item.get("call_id").and_then(|v| v.as_str()).unwrap_or("");
                    let name = restore_original_tool_name(
                        item.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        &reverse_map,
                    );
                    let args = item.get("arguments").and_then(|v| v.as_str()).unwrap_or("");
                    let tool_call = json!({
                        "id": call_id,
                        "type": "function",
                        "function": {
                            "name": name,
                            "arguments": args
                        }
                    });
                    tool_calls.push(tool_call);
                }
                _ => {}
            }
        }
    }

    if !content_text.is_empty() {
        template["choices"][0]["message"]["content"] = json!(content_text);
        template["choices"][0]["message"]["role"] = json!("assistant");
    }
    if !reasoning_text.is_empty() {
        template["choices"][0]["message"]["reasoning_content"] = json!(reasoning_text);
        template["choices"][0]["message"]["role"] = json!("assistant");
    }
    if !tool_calls.is_empty() {
        template["choices"][0]["message"]["tool_calls"] = json!(tool_calls);
        template["choices"][0]["message"]["role"] = json!("assistant");
    }

    if let Some(status) = response.get("status").and_then(|v| v.as_str()) {
        if status == "completed" {
            let finish_reason = if tool_calls.is_empty() {
                "stop"
            } else {
                "tool_calls"
            };
            template["choices"][0]["finish_reason"] = json!(finish_reason);
            template["choices"][0]["native_finish_reason"] = json!(finish_reason);
        }
    }

    Some(template)
}

fn restore_original_tool_name(name: &str, reverse_map: &HashMap<String, String>) -> String {
    reverse_map
        .get(name)
        .cloned()
        .unwrap_or_else(|| name.to_string())
}

fn apply_usage_to_openai(target: &mut Value, usage: Option<&Value>) {
    let Some(usage) = usage else {
        return;
    };

    if let Some(output_tokens) = usage.get("output_tokens").and_then(|v| v.as_i64()) {
        target["usage"]["completion_tokens"] = json!(output_tokens);
    }
    if let Some(total_tokens) = usage.get("total_tokens").and_then(|v| v.as_i64()) {
        target["usage"]["total_tokens"] = json!(total_tokens);
    }
    if let Some(input_tokens) = usage.get("input_tokens").and_then(|v| v.as_i64()) {
        target["usage"]["prompt_tokens"] = json!(input_tokens);
    }
    if let Some(cached_tokens) = usage
        .get("input_tokens_details")
        .and_then(|v| v.get("cached_tokens"))
        .and_then(|v| v.as_i64())
    {
        target["usage"]["prompt_tokens_details"]["cached_tokens"] = json!(cached_tokens);
    }
    if let Some(reasoning_tokens) = usage
        .get("output_tokens_details")
        .and_then(|v| v.get("reasoning_tokens"))
        .and_then(|v| v.as_i64())
    {
        target["usage"]["completion_tokens_details"]["reasoning_tokens"] = json!(reasoning_tokens);
    }
}

fn convert_system_role_to_developer(out: &mut Value) {
    let Some(items) = out.get_mut("input").and_then(|v| v.as_array_mut()) else {
        return;
    };

    for item in items {
        if item.get("role").and_then(|v| v.as_str()) == Some("system") {
            item["role"] = json!("developer");
        }
    }
}

fn build_reverse_map_from_original_openai(original: &Value) -> HashMap<String, String> {
    let mut names = Vec::new();
    if let Some(tools) = original.get("tools").and_then(|v| v.as_array()) {
        for t in tools {
            if t.get("type").and_then(|v| v.as_str()) != Some("function") {
                continue;
            }
            if let Some(name) = t
                .get("function")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
            {
                names.push(name.to_string());
            }
        }
    }
    let short_map = build_short_name_map(&names);
    let mut rev = HashMap::new();
    for (orig, short) in short_map {
        rev.insert(short, orig);
    }
    rev
}

fn build_short_name_map_from_tools(raw: &Value) -> HashMap<String, String> {
    let mut names = Vec::new();
    if let Some(tools) = raw.get("tools").and_then(|v| v.as_array()) {
        for t in tools {
            if t.get("type").and_then(|v| v.as_str()) != Some("function") {
                continue;
            }
            if let Some(name) = t
                .get("function")
                .and_then(|v| v.get("name"))
                .and_then(|v| v.as_str())
            {
                names.push(name.to_string());
            }
        }
    }
    build_short_name_map(&names)
}

fn shorten_name_if_needed(name: &str) -> String {
    const LIMIT: usize = 64;
    if name.len() <= LIMIT {
        return name.to_string();
    }
    if name.starts_with("mcp__") {
        if let Some(idx) = name.rfind("__") {
            let mut candidate = format!("mcp__{}", &name[idx + 2..]);
            if candidate.len() > LIMIT {
                candidate.truncate(LIMIT);
            }
            return candidate;
        }
    }
    name[..LIMIT].to_string()
}

fn build_short_name_map(names: &[String]) -> HashMap<String, String> {
    const LIMIT: usize = 64;
    let mut used: HashSet<String> = HashSet::new();
    let mut out = HashMap::new();

    let base_candidate = |n: &str| -> String {
        if n.len() <= LIMIT {
            return n.to_string();
        }
        if n.starts_with("mcp__") {
            if let Some(idx) = n.rfind("__") {
                let mut cand = format!("mcp__{}", &n[idx + 2..]);
                if cand.len() > LIMIT {
                    cand.truncate(LIMIT);
                }
                return cand;
            }
        }
        n[..LIMIT].to_string()
    };

    let make_unique = |cand: String, used: &mut HashSet<String>| -> String {
        if !used.contains(&cand) {
            return cand;
        }
        let base = cand;
        for i in 1.. {
            let suffix = format!("_{}", i);
            let allowed = LIMIT.saturating_sub(suffix.len());
            let mut tmp = base.clone();
            if tmp.len() > allowed {
                tmp.truncate(allowed);
            }
            tmp.push_str(&suffix);
            if !used.contains(&tmp) {
                return tmp;
            }
        }
        base
    };

    for name in names {
        let cand = base_candidate(name);
        let uniq = make_unique(cand, &mut used);
        used.insert(uniq.clone());
        out.insert(name.clone(), uniq);
    }

    out
}

fn string_from_json_value(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        _ => serde_json::to_string(value).unwrap_or_default(),
    }
}

fn ensure_text_object(out: &mut Value) {
    if !out.get("text").is_some() {
        out["text"] = json!({});
    }
}

fn set_text_format(out: &mut Value, format: Value) {
    if let Some(text) = out.get_mut("text") {
        text["format"] = format;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_to_codex_request_maps_input_file_parts() {
        let raw = json!({
            "model": "gpt-5-codex",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "file",
                    "file": {
                        "file_data": "data:text/plain;base64,SGVsbG8=",
                        "filename": "hello.txt"
                    }
                }]
            }]
        });

        let result = openai_to_codex_request(&raw, "gpt-5-codex", false);
        let parts = result["input"][0]["content"]
            .as_array()
            .expect("content array");

        assert_eq!(parts[0]["type"], "input_file");
        assert_eq!(parts[0]["file_data"], "data:text/plain;base64,SGVsbG8=");
        assert_eq!(parts[0]["filename"], "hello.txt");
    }

    #[test]
    fn convert_codex_stream_chunk_handles_incremental_tool_calls() {
        let mut state = CodexStreamState {
            response_id: String::new(),
            created_at: 0,
            model: String::new(),
            function_call_index: -1,
            has_received_arguments_delta: false,
            has_tool_call_announced: false,
            reverse_tool_names: HashMap::new(),
        };

        assert!(convert_codex_stream_chunk(
            r#"{"type":"response.created","response":{"id":"resp_1","created_at":123,"model":"gpt-5-codex"}}"#,
            &mut state,
        )
        .is_empty());

        let added = convert_codex_stream_chunk(
            r#"{"type":"response.output_item.added","item":{"type":"function_call","call_id":"call_1","name":"tool_a"}}"#,
            &mut state,
        );
        let added_chunk: Value = serde_json::from_str(&added[0]).expect("valid tool-call chunk");
        assert_eq!(
            added_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["name"],
            "tool_a"
        );
        assert_eq!(
            added_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            ""
        );

        let delta = convert_codex_stream_chunk(
            r#"{"type":"response.function_call_arguments.delta","delta":"{\"foo\":"}"#,
            &mut state,
        );
        let delta_chunk: Value =
            serde_json::from_str(&delta[0]).expect("valid tool-call delta chunk");
        assert_eq!(
            delta_chunk["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"],
            "{\"foo\":"
        );

        assert!(convert_codex_stream_chunk(
            r#"{"type":"response.function_call_arguments.done","arguments":"{\"foo\":\"bar\"}"}"#,
            &mut state,
        )
        .is_empty());

        assert!(convert_codex_stream_chunk(
            r#"{"type":"response.output_item.done","item":{"type":"function_call","call_id":"call_1","name":"tool_a","arguments":"{\"foo\":\"bar\"}"}}"#,
            &mut state,
        )
        .is_empty());

        let completed = convert_codex_stream_chunk(r#"{"type":"response.completed"}"#, &mut state);
        let completed_chunk: Value =
            serde_json::from_str(&completed[0]).expect("valid completion chunk");
        assert_eq!(completed_chunk["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn codex_completed_event_to_openai_preserves_cached_usage_and_tool_finish_reason() {
        let original_request = json!({
            "tools": [{
                "type": "function",
                "function": {
                    "name": "tool_a"
                }
            }]
        });
        let event = json!({
            "type": "response.completed",
            "response": {
                "id": "resp_1",
                "created_at": 123,
                "model": "gpt-5-codex",
                "status": "completed",
                "usage": {
                    "input_tokens": 10,
                    "output_tokens": 5,
                    "total_tokens": 15,
                    "input_tokens_details": {
                        "cached_tokens": 3
                    },
                    "output_tokens_details": {
                        "reasoning_tokens": 2
                    }
                },
                "output": [{
                    "type": "function_call",
                    "call_id": "call_1",
                    "name": "tool_a",
                    "arguments": "{\"ok\":true}"
                }]
            }
        });

        let converted = codex_completed_event_to_openai(&event, &original_request)
            .expect("response.completed should convert");

        assert_eq!(
            converted["usage"]["prompt_tokens_details"]["cached_tokens"],
            3
        );
        assert_eq!(
            converted["usage"]["completion_tokens_details"]["reasoning_tokens"],
            2
        );
        assert_eq!(converted["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(
            converted["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"],
            "{\"ok\":true}"
        );
    }

    #[test]
    fn openai_responses_to_codex_request_compacts_for_codex_upstream() {
        let raw = json!({
            "model": "codex/gpt-5-codex",
            "input": "hello",
            "max_output_tokens": 128,
            "temperature": 0.7,
            "context_management": {
                "compaction": "auto"
            },
            "user": "alice"
        });

        let converted = openai_responses_to_codex_request(&raw, "gpt-5-codex");

        assert_eq!(converted["model"], "gpt-5-codex");
        assert_eq!(converted["stream"], true);
        assert_eq!(converted["store"], false);
        assert_eq!(converted["input"][0]["content"][0]["text"], "hello");
        assert!(converted.get("max_output_tokens").is_none());
        assert!(converted.get("temperature").is_none());
        assert!(converted.get("context_management").is_none());
        assert!(converted.get("user").is_none());
    }

    #[test]
    fn normalize_responses_websocket_request_initializes_create_payload() {
        let raw = json!({
            "type": "response.create",
            "model": "codex/gpt-5-codex"
        });

        let (normalized, next_last_request) =
            normalize_responses_websocket_request(&raw, None, &json!([]), true)
                .expect("response.create should normalize");

        assert_eq!(normalized["stream"], true);
        assert_eq!(normalized["input"], json!([]));
        assert!(normalized.get("type").is_none());
        assert_eq!(next_last_request, normalized);
    }

    #[test]
    fn normalize_responses_websocket_request_merges_append_input_without_previous_response_id() {
        let last_request = json!({
            "model": "gpt-5-codex",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "hello"
                }]
            }],
            "instructions": "be concise"
        });
        let last_output = json!([{
            "type": "message",
            "role": "assistant",
            "content": [{
                "type": "output_text",
                "text": "world"
            }]
        }]);
        let raw = json!({
            "type": "response.append",
            "input": [{
                "type": "message",
                "role": "user",
                "content": [{
                    "type": "input_text",
                    "text": "again"
                }]
            }]
        });

        let (normalized, _) =
            normalize_responses_websocket_request(&raw, Some(&last_request), &last_output, false)
                .expect("response.append should normalize");

        assert_eq!(normalized["model"], "gpt-5-codex");
        assert_eq!(normalized["instructions"], "be concise");
        assert_eq!(
            normalized["input"].as_array().expect("input array").len(),
            3
        );
        assert!(normalized.get("previous_response_id").is_none());
    }
}
