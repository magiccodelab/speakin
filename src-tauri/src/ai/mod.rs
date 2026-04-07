//! AI text optimization module.
//!
//! Provides optional AI-powered text optimization as a middleware between
//! ASR transcription and text output. Supports any OpenAI-compatible API.
//! Users can configure multiple AI provider instances with different URLs,
//! models, and custom parameters.

pub mod client;
pub mod prompts;
pub mod providers;

use providers::{AiProvider, AiProvidersFile, ApiProtocol};
use serde::{Deserialize, Serialize};
use tauri::AppHandle;

/// AI optimization settings (stored in AppSettings).
/// Only contains the enable toggle and active selection IDs.
/// Provider details and prompts are in separate files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiOptimizeSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub active_provider_id: String,
    #[serde(default)]
    pub active_prompt_id: String,
    /// Initial TCP/TLS connection timeout (seconds).
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout_secs: u64,
    /// Maximum total request duration including streaming (seconds).
    #[serde(default = "default_max_request")]
    pub max_request_secs: u64,
}

fn default_connect_timeout() -> u64 {
    5
}
fn default_max_request() -> u64 {
    60
}

impl Default for AiOptimizeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            active_provider_id: String::new(),
            active_prompt_id: String::new(),
            connect_timeout_secs: default_connect_timeout(),
            max_request_secs: default_max_request(),
        }
    }
}

/// Refresh cached providers in AppState after a mutation.
fn refresh_providers_cache(app_handle: &AppHandle, data: &AiProvidersFile) {
    use tauri::Manager;
    if let Some(state) = app_handle.try_state::<crate::AppState>() {
        *state.cached_providers.lock() = data.clone();
    }
}

/// Refresh cached prompts in AppState after a mutation.
fn refresh_prompts_cache(app_handle: &AppHandle, data: &prompts::PromptsFile) {
    use tauri::Manager;
    if let Some(state) = app_handle.try_state::<crate::AppState>() {
        *state.cached_prompts.lock() = data.clone();
    }
}

// ── AI Provider CRUD Commands ──

#[tauri::command]
pub fn get_ai_providers(app_handle: AppHandle) -> AiProvidersFile {
    providers::load_providers(&app_handle)
}

/// Add a new AI provider instance. Backend generates the UUID.
#[tauri::command]
pub fn add_ai_provider(app_handle: AppHandle, provider: AiProvider) -> Result<AiProvider, String> {
    let mut new_provider = provider;
    new_provider.id = uuid::Uuid::new_v4().to_string();
    new_provider.name = new_provider.name.trim().to_string();
    new_provider.api_endpoint = new_provider.api_endpoint.trim().to_string();
    new_provider.model = new_provider.model.trim().to_string();

    providers::validate_provider(&new_provider)?;

    let mut data = providers::load_providers(&app_handle);
    data.providers.push(new_provider.clone());
    providers::save_providers(&app_handle, &data)?;
    refresh_providers_cache(&app_handle, &data);

    Ok(new_provider)
}

/// Update an existing AI provider instance by ID.
#[tauri::command]
pub fn update_ai_provider(app_handle: AppHandle, provider: AiProvider) -> Result<(), String> {
    let mut updated = provider;
    updated.name = updated.name.trim().to_string();
    updated.api_endpoint = updated.api_endpoint.trim().to_string();
    updated.model = updated.model.trim().to_string();

    providers::validate_provider(&updated)?;

    let mut data = providers::load_providers(&app_handle);
    let idx = data
        .providers
        .iter()
        .position(|p| p.id == updated.id)
        .ok_or_else(|| format!("供应商 ID 不存在: {}", updated.id))?;

    data.providers[idx] = updated;
    providers::save_providers(&app_handle, &data)?;
    refresh_providers_cache(&app_handle, &data);

    Ok(())
}

/// Delete an AI provider instance. Also clears its keyring entry.
/// If the deleted provider was active, disables AI optimization.
/// Returns the updated AiOptimizeSettings so the frontend can sync its form state.
#[tauri::command]
pub fn delete_ai_provider(
    app_handle: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    id: String,
) -> Result<AiOptimizeSettings, String> {
    let mut data = providers::load_providers(&app_handle);
    let before_len = data.providers.len();
    data.providers.retain(|p| p.id != id);

    if data.providers.len() == before_len {
        return Err(format!("供应商 ID 不存在: {}", id));
    }

    providers::save_providers(&app_handle, &data)?;
    refresh_providers_cache(&app_handle, &data);

    // Clean up keyring (ignore errors — key may not exist)
    let _ = providers::clear_provider_key(&id);

    // If deleted provider was active, disable AI optimization
    {
        let mut inner = state.inner.lock();
        if inner.settings.ai_optimize.active_provider_id == id {
            let mut settings = (*inner.settings).clone();
            settings.ai_optimize.active_provider_id = String::new();
            settings.ai_optimize.enabled = false;
            let _ = crate::storage::save_settings(&app_handle, &settings);
            inner.settings = std::sync::Arc::new(settings);
        }
    }

    // Return current ai_optimize settings so frontend can sync its form
    let current = state.inner.lock().settings.ai_optimize.clone();
    Ok(current)
}

// ── Per-provider API Key Commands ──

#[tauri::command]
pub fn has_ai_provider_key(id: String) -> Result<bool, String> {
    providers::has_provider_key(&id)
}

#[tauri::command]
pub fn set_ai_provider_key(id: String, key: String) -> Result<(), String> {
    providers::save_provider_key(&id, &key)
}

#[tauri::command]
pub fn clear_ai_provider_key(id: String) -> Result<(), String> {
    providers::clear_provider_key(&id)
}

// ── Prompt Commands ──

#[tauri::command]
pub fn get_prompts(app_handle: AppHandle) -> prompts::PromptsFile {
    prompts::load_prompts(&app_handle)
}

#[tauri::command]
pub fn save_prompts(
    app_handle: AppHandle,
    prompts_data: prompts::PromptsFile,
) -> Result<(), String> {
    let mut seen_ids = std::collections::HashSet::new();
    for prompt in &prompts_data.prompts {
        if prompt.name.trim().is_empty() {
            return Err("提示词名称不能为空".to_string());
        }
        if !prompt.user_prompt_template.contains("{{text}}") {
            return Err(format!(
                "提示词「{}」的用户提示词模板必须包含 {{{{text}}}} 占位符",
                prompt.name
            ));
        }
        if !seen_ids.insert(&prompt.id) {
            return Err(format!("提示词 ID 重复: {}", prompt.id));
        }
    }

    let existing = prompts::load_prompts(&app_handle);
    for builtin in existing.prompts.iter().filter(|p| p.is_builtin) {
        if !prompts_data.prompts.iter().any(|p| p.id == builtin.id) {
            return Err(format!("不能删除内置提示词「{}」", builtin.name));
        }
    }

    prompts::save_prompts(&app_handle, &prompts_data)?;
    refresh_prompts_cache(&app_handle, &prompts_data);
    Ok(())
}

// ── Test Command ──

/// Test an AI provider configuration with a simple request.
/// Returns detailed result including response text, timing, and model info.
#[tauri::command]
pub async fn test_ai_provider(
    app_handle: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    provider: providers::AiProvider,
    api_key: String,
) -> Result<serde_json::Value, String> {
    providers::validate_provider(&provider)?;

    if api_key.trim().is_empty() {
        return Err("API Key 不能为空".to_string());
    }

    let start = std::time::Instant::now();
    let base = provider.api_endpoint.trim_end_matches('/');
    let is_gemini = provider.protocol == ApiProtocol::Gemini;

    // Build protocol-specific request
    let (url, body, auth_header_name, auth_header_value) = if is_gemini {
        let model = provider.model.trim_start_matches("models/");
        let url = format!("{}/models/{}:generateContent", base, model);
        let mut body = serde_json::json!({
            "contents": [{
                "parts": [{ "text": "请回复「测试成功」两个字。" }]
            }]
        });
        if let Some(extra) = provider.extra_body.as_object() {
            for (key, value) in extra {
                if !client::GEMINI_RESERVED_KEYS.contains(&key.as_str()) {
                    body[key] = value.clone();
                }
            }
        }
        (url, body, "x-goog-api-key", api_key.clone())
    } else {
        let url = format!("{}/chat/completions", base);
        let mut body = serde_json::json!({
            "model": &provider.model,
            "messages": [
                { "role": "user", "content": "请回复「测试成功」两个字。" }
            ],
            "stream": false
        });
        if let Some(extra) = provider.extra_body.as_object() {
            for (key, value) in extra {
                if !client::OPENAI_RESERVED_KEYS.contains(&key.as_str()) {
                    body[key] = value.clone();
                }
            }
        }
        (url, body, "Authorization", format!("Bearer {}", api_key))
    };

    crate::emit_log(&app_handle, "info", &format!("[AI] 测试请求: POST {} [{}]", url, if is_gemini { "Gemini" } else { "OpenAI" }));

    let ai_settings = {
        let inner = state.inner.lock();
        inner.settings.ai_optimize.clone()
    };

    let http_client = client::get_shared_client();

    let response = http_client
        .post(&url)
        .timeout(std::time::Duration::from_secs(ai_settings.max_request_secs))
        .header(auth_header_name, &auth_header_value)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                format!("请求超时（{}秒）", ai_settings.max_request_secs)
            } else if e.is_connect() {
                format!("连接超时（{}秒）: {}", ai_settings.connect_timeout_secs, e)
            } else {
                format!("请求失败: {}", e)
            }
        })?;

    let status = response.status().as_u16();
    let headers_time = start.elapsed().as_millis();

    // Check HTTP status before JSON parsing to preserve non-JSON error bodies
    if status < 200 || status >= 300 {
        let error_body = response.text().await.unwrap_or_default();
        let error_msg = serde_json::from_str::<serde_json::Value>(&error_body)
            .ok()
            .and_then(|j| j["error"]["message"].as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| error_body.clone());
        return Err(format!("HTTP {} — {}", status, error_msg));
    }

    let response_body: serde_json::Value = response
        .json()
        .await
        .map_err(|e| format!("解析响应失败: {}", e))?;

    let total_time = start.elapsed().as_millis();

    // Parse response based on protocol
    let (content, model, prompt_tokens, completion_tokens, total_tokens) = if is_gemini {
        let content = client::extract_gemini_text_pub(&response_body);
        let model = response_body["modelVersion"]
            .as_str()
            .unwrap_or(&provider.model)
            .to_string();
        let usage = &response_body["usageMetadata"];
        let pt = usage["promptTokenCount"].as_u64().unwrap_or(0);
        let ct = usage["candidatesTokenCount"].as_u64().unwrap_or(0);
        let tt = usage["totalTokenCount"].as_u64().unwrap_or(0);
        (content, model, pt, ct, tt)
    } else {
        let content = response_body["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let model = response_body["model"]
            .as_str()
            .unwrap_or(&provider.model)
            .to_string();
        let usage = &response_body["usage"];
        let pt = usage["prompt_tokens"].as_u64().unwrap_or(0);
        let ct = usage["completion_tokens"].as_u64().unwrap_or(0);
        let tt = usage["total_tokens"].as_u64().unwrap_or(0);
        (content, model, pt, ct, tt)
    };

    crate::emit_log(
        &app_handle,
        "info",
        &format!("[AI] 测试完成: {} ms, model={}", total_time, model),
    );

    Ok(serde_json::json!({
        "content": content,
        "model": model,
        "status": status,
        "headers_time_ms": headers_time,
        "total_time_ms": total_time,
        "prompt_tokens": prompt_tokens,
        "completion_tokens": completion_tokens,
        "total_tokens": total_tokens,
    }))
}

// ── AI Optimization Command ──

/// Optimize text using the active AI provider.
#[tauri::command]
pub async fn ai_optimize_text(
    app_handle: AppHandle,
    state: tauri::State<'_, crate::AppState>,
    text: String,
    session_id: u64,
) -> Result<String, String> {
    let settings = {
        let inner = state.inner.lock();
        inner.settings.ai_optimize.clone()
    };

    if !settings.enabled {
        return Ok(text);
    }

    if settings.active_provider_id.is_empty() {
        return Err("未选择 AI 供应商".to_string());
    }

    // Load provider config from cache
    let provider = {
        use tauri::Manager;
        let cached = app_handle
            .try_state::<crate::AppState>()
            .map(|s| s.cached_providers.lock().clone())
            .unwrap_or_else(|| providers::load_providers(&app_handle));
        cached
            .providers
            .into_iter()
            .find(|p| p.id == settings.active_provider_id)
            .ok_or_else(|| "所选 AI 供应商不存在，请在设置中重新选择".to_string())?
    };

    // Load API key from keyring
    let api_key = providers::load_provider_key(&provider.id)
        .ok_or_else(|| format!("AI 供应商「{}」的 API Key 未配置", provider.name))?;

    // Load prompt template from cache
    let prompts_file = {
        use tauri::Manager;
        app_handle
            .try_state::<crate::AppState>()
            .map(|s| s.cached_prompts.lock().clone())
            .unwrap_or_else(|| prompts::load_prompts(&app_handle))
    };
    let prompt = if settings.active_prompt_id.is_empty() {
        prompts_file.prompts.first()
    } else {
        prompts_file
            .prompts
            .iter()
            .find(|p| p.id == settings.active_prompt_id)
            .or_else(|| prompts_file.prompts.first())
    };

    let prompt = prompt.ok_or_else(|| "未找到可用的提示词模板".to_string())?;

    let system_prompt = &prompt.system_prompt;
    let user_message = prompt.user_prompt_template.replace("{{text}}", &text);

    let protocol_label = match provider.protocol {
        ApiProtocol::Gemini => "Gemini",
        ApiProtocol::Openai => "OpenAI",
    };
    crate::emit_log(&app_handle, "info", &format!("[AI] → 开始 AI 文本优化 [{}]...", protocol_label));

    let result = match provider.protocol {
        ApiProtocol::Gemini => {
            client::call_gemini_completion(
                &app_handle, &provider, &api_key, system_prompt, &user_message, session_id,
                settings.connect_timeout_secs, settings.max_request_secs,
            ).await?
        }
        ApiProtocol::Openai => {
            client::call_chat_completion(
                &app_handle, &provider, &api_key, system_prompt, &user_message, session_id,
                settings.connect_timeout_secs, settings.max_request_secs,
            ).await?
        }
    };

    crate::emit_log(
        &app_handle,
        "info",
        &format!("[AI] ← AI 优化完成 ({} 字)", result.chars().count()),
    );

    Ok(result)
}
