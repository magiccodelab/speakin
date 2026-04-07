//! OpenAI-compatible HTTP client for AI text optimization.
//!
//! Supports both streaming (SSE) and non-streaming chat completion requests.
//! Compatible with OpenAI, Claude (via proxy), DeepSeek, Groq, etc.

use super::providers::AiProvider;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

/// Shared HTTP client for connection pooling across AI requests.
/// Created once on first use; reuses TCP/TLS connections to the same host.
static SHARED_CLIENT: OnceLock<reqwest::Client> = OnceLock::new();

pub(crate) fn get_shared_client() -> &'static reqwest::Client {
    SHARED_CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_max_idle_per_host(2)
            .connect_timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to create shared HTTP client")
    })
}

/// Reserved keys that extra_body cannot override (OpenAI protocol).
pub const OPENAI_RESERVED_KEYS: &[&str] = &["model", "messages", "stream"];
/// Reserved keys for Gemini protocol.
pub const GEMINI_RESERVED_KEYS: &[&str] = &["contents", "system_instruction"];

/// Call a chat completion endpoint (OpenAI-compatible).
///
/// In streaming mode, emits `ai-optimize-chunk` events with `{ chunk, session_id }`.
/// Returns the full accumulated response text.
pub async fn call_chat_completion(
    app_handle: &AppHandle,
    provider: &AiProvider,
    api_key: &str,
    system_prompt: &str,
    user_message: &str,
    session_id: u64,
    connect_timeout_secs: u64,
    max_request_secs: u64,
) -> Result<String, String> {
    let url = format!(
        "{}/chat/completions",
        provider.api_endpoint.trim_end_matches('/')
    );
    let start = Instant::now();

    // Build request body with core fields
    let mut body = serde_json::json!({
        "model": &provider.model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": user_message }
        ],
        "stream": provider.stream
    });

    // Merge extra_body into request body (skip reserved keys)
    if let Some(extra) = provider.extra_body.as_object() {
        for (key, value) in extra {
            if !OPENAI_RESERVED_KEYS.contains(&key.as_str()) {
                body[key] = value.clone();
            }
        }
    }
    let pretty_body = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());

    let client = get_shared_client();

    let request_headers = vec![
        ("authorization", format!("Bearer {}", mask_secret(api_key))),
        ("content-type", "application/json".to_string()),
        (
            "accept",
            if provider.stream {
                "text/event-stream"
            } else {
                "application/json"
            }
            .to_string(),
        ),
    ];

    emit_ai_log(
        app_handle,
        "send",
        &format!(
            "POST {}\n{}\n\n{}",
            url,
            format_header_lines(&request_headers),
            pretty_body
        ),
    );

    let response = client
        .post(&url)
        .timeout(Duration::from_secs(max_request_secs))
        .header("Authorization", format!("Bearer {}", api_key))
        .header("Content-Type", "application/json")
        .header(
            "Accept",
            if provider.stream {
                "text/event-stream"
            } else {
                "application/json"
            },
        )
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("AI API 请求超时（{}秒）", max_request_secs)
            } else if e.is_connect() {
                format!("AI API 连接超时（{}秒）: {}", connect_timeout_secs, e)
            } else {
                format!("AI API 请求失败: {}", e)
            }
        })?;

    // Check HTTP status before processing body
    let status = response.status();
    let elapsed_to_headers = start.elapsed();
    emit_ai_log(
        app_handle,
        "recv",
        &format!(
            "HTTP {} {} ms\n{}",
            status.as_u16(),
            elapsed_to_headers.as_millis(),
            format_reqwest_headers(response.headers())
        ),
    );
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        let error_msg = parse_api_error(&error_body).unwrap_or_else(|| error_body.clone());
        emit_ai_log(app_handle, "error", &format!("错误响应体:\n{}", error_body));
        return Err(format!("AI API 错误 ({}): {}", status.as_u16(), error_msg));
    }

    if provider.stream {
        handle_streaming_response(app_handle, response, session_id, start, connect_timeout_secs).await
    } else {
        handle_non_streaming_response(app_handle, response, start).await
    }
}

/// Handle a non-streaming response.
async fn handle_non_streaming_response(
    app_handle: &AppHandle,
    response: reqwest::Response,
    start: Instant,
) -> Result<String, String> {
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "AI 响应格式异常：缺少 content 字段".to_string())?;

    let model = body["model"].as_str().unwrap_or("unknown");
    emit_ai_log(
        app_handle,
        "recv",
        &format!(
            "[AI] 非流式响应完成 ({} ms), model={}, content: {} 字",
            start.elapsed().as_millis(),
            model,
            content.chars().count()
        ),
    );

    Ok(content)
}

enum SseResult {
    /// Event processed, had actual content delta.
    Content,
    /// Event processed, but no content (e.g. reasoning_content, role delta, empty).
    Ignored,
    /// JSON parse failure on this event.
    ParseError,
    /// Stream complete ([DONE] marker).
    Done,
    /// API returned an error in the stream.
    Error(String),
}

/// Process a single SSE event block. Extracts delta content and emits chunk events.
fn process_sse_event(
    event_str: &str,
    accumulated: &mut String,
    app_handle: &AppHandle,
    session_id: u64,
) -> SseResult {
    let mut data_parts = Vec::new();
    for line in event_str.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data == "[DONE]" {
                return SseResult::Done;
            }
            if !data.is_empty() {
                data_parts.push(data.to_string());
            }
        }
    }

    if data_parts.is_empty() {
        return SseResult::Ignored;
    }

    let data_json = data_parts.join("");
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&data_json) else {
        // Log first parse failure for diagnostics (truncated to 200 chars)
        let sample = if data_json.len() > 200 { &data_json[..200] } else { &data_json };
        log::warn!("SSE JSON 解析失败: {}", sample);
        return SseResult::ParseError;
    };

    if let Some(error) = json.get("error") {
        let msg = error["message"]
            .as_str()
            .unwrap_or("流式响应中收到错误");
        return SseResult::Error(format!("AI API 流式错误: {}", msg));
    }

    if let Some(content) = json["choices"][0]["delta"]["content"].as_str() {
        if !content.is_empty() {
            accumulated.push_str(content);
            let chunk_event = serde_json::json!({
                "chunk": content,
                "session_id": session_id
            });
            let _ = app_handle.emit("ai-optimize-chunk", chunk_event);
            return SseResult::Content;
        }
    }

    SseResult::Ignored
}

/// Handle a streaming SSE response.
///
/// SSE protocol: events are separated by `\n\n`.
/// Each event may have multiple `data:` lines which are concatenated.
/// Special: `data: [DONE]` signals end of stream.
async fn handle_streaming_response(
    app_handle: &AppHandle,
    response: reqwest::Response,
    session_id: u64,
    start: Instant,
    connect_timeout_secs: u64,
) -> Result<String, String> {
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut accumulated = String::new();
    let idle_secs = connect_timeout_secs.max(5);
    let idle_timeout = Duration::from_secs(idle_secs);

    // Transport counters
    let mut chunk_count: u32 = 0;
    let mut total_bytes: usize = 0;
    let mut first_chunk_logged = false;

    // Diagnostic counters (replace per-chunk logging)
    let mut content_events: u32 = 0;
    let mut ignored_events: u32 = 0;
    let mut parse_failures: u32 = 0;
    let mut first_content_ms: Option<u128> = None;

    loop {
        let chunk_result =
            tokio::time::timeout(idle_timeout, stream.next()).await;

        match chunk_result {
            Err(_) => {
                return Err(format!("AI API 流式响应超时（{}秒无数据）", idle_secs));
            }
            Ok(None) => {
                // Stream ended — flush remaining buffer and handle its result
                let remaining = buffer.trim().to_string();
                if !remaining.is_empty() {
                    match process_sse_event(&remaining, &mut accumulated, app_handle, session_id) {
                        SseResult::Content => {
                            content_events += 1;
                            if first_content_ms.is_none() {
                                first_content_ms = Some(start.elapsed().as_millis());
                            }
                        }
                        SseResult::Ignored => { ignored_events += 1; }
                        SseResult::ParseError => { parse_failures += 1; }
                        SseResult::Done => {
                            emit_ai_log(app_handle, "info", &format!(
                                "流式完成: {} chunks, {} bytes, 总耗时 {} ms | 首个 content: {} ms | content: {} | 忽略: {} | 解析失败: {}",
                                chunk_count, total_bytes, start.elapsed().as_millis(),
                                first_content_ms.map_or("N/A".to_string(), |ms| ms.to_string()),
                                content_events, ignored_events, parse_failures,
                            ));
                            return Ok(accumulated);
                        }
                        SseResult::Error(e) => return Err(e),
                    }
                }
                emit_ai_log(app_handle, "info", &format!(
                    "流式结束（无 [DONE] 标记）: {} chunks, {} bytes, 总耗时 {} ms | 首个 content: {} ms | content: {} | 忽略: {} | 解析失败: {}",
                    chunk_count, total_bytes, start.elapsed().as_millis(),
                    first_content_ms.map_or("N/A".to_string(), |ms| ms.to_string()),
                    content_events, ignored_events, parse_failures,
                ));
                break;
            }
            Ok(Some(Err(e))) => {
                return Err(format!("AI 流式读取失败: {}", e));
            }
            Ok(Some(Ok(bytes))) => {
                chunk_count += 1;
                total_bytes += bytes.len();

                if !first_chunk_logged {
                    first_chunk_logged = true;
                    emit_ai_log(
                        app_handle,
                        "info",
                        &format!("流式首包到达: {} ms", start.elapsed().as_millis()),
                    );
                }

                let text = String::from_utf8_lossy(&bytes);
                buffer.push_str(&text);

                // Normalize \r\n to \n for cross-platform SSE compatibility
                if buffer.contains('\r') {
                    buffer = buffer.replace("\r\n", "\n").replace('\r', "\n");
                }

                // Process complete SSE events (separated by \n\n)
                while let Some(event_end) = buffer.find("\n\n") {
                    let event_str = buffer[..event_end].to_string();
                    buffer = buffer[event_end + 2..].to_string();

                    match process_sse_event(&event_str, &mut accumulated, app_handle, session_id) {
                        SseResult::Content => {
                            content_events += 1;
                            if first_content_ms.is_none() {
                                first_content_ms = Some(start.elapsed().as_millis());
                            }
                        }
                        SseResult::Ignored => {
                            ignored_events += 1;
                        }
                        SseResult::ParseError => {
                            parse_failures += 1;
                        }
                        SseResult::Done => {
                            emit_ai_log(app_handle, "info", &format!(
                                "流式完成: {} chunks, {} bytes, 总耗时 {} ms | 首个 content: {} ms | content: {} | 忽略: {} | 解析失败: {}",
                                chunk_count, total_bytes, start.elapsed().as_millis(),
                                first_content_ms.map_or("N/A".to_string(), |ms| ms.to_string()),
                                content_events, ignored_events, parse_failures,
                            ));
                            return Ok(accumulated);
                        }
                        SseResult::Error(e) => return Err(e),
                    }
                }
            }
        }
    }

    Ok(accumulated)
}

fn emit_ai_log(app_handle: &AppHandle, level: &str, msg: &str) {
    // AI 日志可能包含用户转写内容和 API 请求/响应体，仅 debug 模式下发射到前端
    use tauri::Manager;
    let debug = app_handle
        .try_state::<crate::AppState>()
        .map(|s| s.inner.lock().settings.debug_mode)
        .unwrap_or(false);
    if debug {
        crate::emit_log(app_handle, level, &format!("[AI] {}", msg));
    }
}

fn mask_secret(value: &str) -> String {
    let count = value.chars().count();
    if count <= 8 {
        "*".repeat(count.max(1))
    } else {
        let prefix: String = value.chars().take(4).collect();
        let suffix: String = value
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect();
        format!("{}***{}", prefix, suffix)
    }
}

fn format_header_lines(headers: &[(&str, String)]) -> String {
    headers
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_reqwest_headers(headers: &reqwest::header::HeaderMap) -> String {
    headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("<non-utf8>");
            format!("{}: {}", name.as_str(), value)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Try to extract a human-readable error message from an API error response body.
/// Both OpenAI and Gemini use the same error.message JSON path.
fn parse_api_error(body: &str) -> Option<String> {
    let json: serde_json::Value = serde_json::from_str(body).ok()?;
    json["error"]["message"].as_str().map(|s| s.to_string())
}

// ══════════════════════════════════════════════════════════════════
// Gemini native API client
// ══════════════════════════════════════════════════════════════════

/// Call a Gemini native generateContent endpoint.
///
/// In streaming mode, emits `ai-optimize-chunk` events.
/// Returns the full accumulated response text.
pub async fn call_gemini_completion(
    app_handle: &AppHandle,
    provider: &AiProvider,
    api_key: &str,
    system_prompt: &str,
    user_message: &str,
    session_id: u64,
    connect_timeout_secs: u64,
    max_request_secs: u64,
) -> Result<String, String> {
    // Normalize model name (strip "models/" prefix if user added it)
    let model = provider.model.trim_start_matches("models/");
    let base = provider.api_endpoint.trim_end_matches('/');

    let url = if provider.stream {
        format!("{}/models/{}:streamGenerateContent?alt=sse", base, model)
    } else {
        format!("{}/models/{}:generateContent", base, model)
    };

    let start = Instant::now();

    // Build Gemini request body
    let mut body = serde_json::json!({
        "system_instruction": {
            "parts": [{ "text": system_prompt }]
        },
        "contents": [{
            "parts": [{ "text": user_message }]
        }]
    });

    // Merge extra_body (skip reserved keys)
    if let Some(extra) = provider.extra_body.as_object() {
        for (key, value) in extra {
            if !GEMINI_RESERVED_KEYS.contains(&key.as_str()) {
                body[key] = value.clone();
            }
        }
    }

    let pretty_body = serde_json::to_string_pretty(&body).unwrap_or_else(|_| body.to_string());

    let client = get_shared_client();

    emit_ai_log(
        app_handle,
        "send",
        &format!(
            "POST {} [Gemini]\nx-goog-api-key: {}\n\n{}",
            url,
            mask_secret(api_key),
            pretty_body,
        ),
    );

    let response = client
        .post(&url)
        .timeout(Duration::from_secs(max_request_secs))
        .header("x-goog-api-key", api_key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("Gemini API 请求超时（{}秒）", max_request_secs)
            } else if e.is_connect() {
                format!("Gemini API 连接超时（{}秒）: {}", connect_timeout_secs, e)
            } else {
                format!("Gemini API 请求失败: {}", e)
            }
        })?;

    let status = response.status();
    let elapsed_to_headers = start.elapsed();
    emit_ai_log(
        app_handle,
        "recv",
        &format!(
            "HTTP {} {} ms\n{}",
            status.as_u16(),
            elapsed_to_headers.as_millis(),
            format_reqwest_headers(response.headers())
        ),
    );
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        let error_msg = parse_api_error(&error_body).unwrap_or_else(|| error_body.clone());
        emit_ai_log(app_handle, "error", &format!("错误响应体:\n{}", error_body));
        return Err(format!("Gemini API 错误 ({}): {}", status.as_u16(), error_msg));
    }

    if provider.stream {
        handle_gemini_streaming(app_handle, response, session_id, start, connect_timeout_secs).await
    } else {
        handle_gemini_non_streaming(app_handle, response, start).await
    }
}

/// Extract text from Gemini response parts, filtering out thought parts.
pub fn extract_gemini_text_pub(json: &serde_json::Value) -> String {
    extract_gemini_text(json)
}

fn extract_gemini_text(json: &serde_json::Value) -> String {
    let mut text = String::new();
    if let Some(parts) = json["candidates"][0]["content"]["parts"].as_array() {
        for part in parts {
            // Skip thought/reasoning parts
            if part["thought"].as_bool().unwrap_or(false) {
                continue;
            }
            if let Some(t) = part["text"].as_str() {
                text.push_str(t);
            }
        }
    }
    text
}

/// Handle a non-streaming Gemini response.
async fn handle_gemini_non_streaming(
    app_handle: &AppHandle,
    response: reqwest::Response,
    start: Instant,
) -> Result<String, String> {
    let body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析 Gemini 响应失败: {}", e))?;

    let content = extract_gemini_text(&body);
    if content.is_empty() {
        return Err("Gemini 响应格式异常：未找到文本内容".to_string());
    }

    let model = body["modelVersion"].as_str().unwrap_or("unknown");
    emit_ai_log(
        app_handle,
        "recv",
        &format!(
            "[AI] Gemini 非流式响应完成 ({} ms), model={}, content: {} 字",
            start.elapsed().as_millis(),
            model,
            content.chars().count()
        ),
    );

    Ok(content)
}

/// Handle a streaming Gemini SSE response.
/// Gemini SSE: each `data:` payload is a full GenerateContentResponse (not delta).
/// No `[DONE]` sentinel — stream ends normally.
async fn handle_gemini_streaming(
    app_handle: &AppHandle,
    response: reqwest::Response,
    session_id: u64,
    start: Instant,
    connect_timeout_secs: u64,
) -> Result<String, String> {
    use futures_util::StreamExt;

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();
    let mut accumulated = String::new();
    let idle_secs = connect_timeout_secs.max(5);
    let idle_timeout = Duration::from_secs(idle_secs);

    let mut chunk_count: u32 = 0;
    let mut total_bytes: usize = 0;
    let mut content_events: u32 = 0;
    let mut first_content_ms: Option<u128> = None;
    let mut first_chunk_logged = false;

    loop {
        let chunk_result = tokio::time::timeout(idle_timeout, stream.next()).await;

        match chunk_result {
            Err(_) => {
                return Err(format!("Gemini API 流式响应超时（{}秒无数据）", idle_secs));
            }
            Ok(None) => {
                // Stream ended — flush remaining buffer
                let remaining = buffer.trim().to_string();
                if !remaining.is_empty() {
                    let text = process_gemini_sse_data(&remaining);
                    if !text.is_empty() {
                        accumulated.push_str(&text);
                        content_events += 1;
                        if first_content_ms.is_none() {
                            first_content_ms = Some(start.elapsed().as_millis());
                        }
                        let chunk_event = serde_json::json!({
                            "chunk": text,
                            "session_id": session_id
                        });
                        let _ = app_handle.emit("ai-optimize-chunk", chunk_event);
                    }
                }
                emit_ai_log(app_handle, "info", &format!(
                    "Gemini 流式完成: {} chunks, {} bytes, 总耗时 {} ms | 首个 content: {} ms | content events: {}",
                    chunk_count, total_bytes, start.elapsed().as_millis(),
                    first_content_ms.map_or("N/A".to_string(), |ms| ms.to_string()),
                    content_events,
                ));
                break;
            }
            Ok(Some(Err(e))) => {
                return Err(format!("Gemini 流式读取失败: {}", e));
            }
            Ok(Some(Ok(bytes))) => {
                chunk_count += 1;
                total_bytes += bytes.len();

                if !first_chunk_logged {
                    first_chunk_logged = true;
                    emit_ai_log(
                        app_handle,
                        "info",
                        &format!("Gemini 流式首包到达: {} ms", start.elapsed().as_millis()),
                    );
                }

                let text = String::from_utf8_lossy(&bytes);
                buffer.push_str(&text);

                if buffer.contains('\r') {
                    buffer = buffer.replace("\r\n", "\n").replace('\r', "\n");
                }

                // Process complete SSE events (separated by \n\n)
                while let Some(event_end) = buffer.find("\n\n") {
                    let event_str = buffer[..event_end].to_string();
                    buffer = buffer[event_end + 2..].to_string();

                    let content = process_gemini_sse_data(&event_str);
                    if !content.is_empty() {
                        accumulated.push_str(&content);
                        content_events += 1;
                        if first_content_ms.is_none() {
                            first_content_ms = Some(start.elapsed().as_millis());
                        }
                        let chunk_event = serde_json::json!({
                            "chunk": content,
                            "session_id": session_id
                        });
                        let _ = app_handle.emit("ai-optimize-chunk", chunk_event);
                    }
                }
            }
        }
    }

    Ok(accumulated)
}

/// Process a single Gemini SSE event block. Extract text content from `data:` lines.
fn process_gemini_sse_data(event_str: &str) -> String {
    let mut data_parts = Vec::new();
    for line in event_str.lines() {
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if !data.is_empty() {
                data_parts.push(data.to_string());
            }
        }
    }

    if data_parts.is_empty() {
        return String::new();
    }

    let data_json = data_parts.join("");
    match serde_json::from_str::<serde_json::Value>(&data_json) {
        Ok(json) => extract_gemini_text(&json),
        Err(e) => {
            log::warn!("Gemini SSE JSON 解析失败: {}", e);
            String::new()
        }
    }
}
