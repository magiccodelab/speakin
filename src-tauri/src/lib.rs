mod ai;
mod asr;
mod audio;
mod filler;
mod hotkey;
mod input;
mod loopback;
mod replacements;
mod storage;

use asr::dashscope::{DashScopeProvider, DashScopeSettings};
use asr::doubao::{DoubaoProvider, DoubaoSettings};
use asr::qwen::{QwenProvider, QwenSettings};
use asr::AsrProvider;
use audio::MicrophoneManager;
use hotkey::{HotkeyEvent, InputMode};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Emitter, Manager};

/// Doubao (豆包) provider settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DoubaoProviderSettings {
    // Credentials are persisted in the OS keyring, NOT in settings.json.
    // We use plain `#[serde(default)]` (not `skip`) so they round-trip
    // correctly through Tauri IPC. The storage layer
    // (storage::save_settings / load_settings) is responsible for keeping
    // them out of the JSON file on disk.
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default = "default_resource_id")]
    pub resource_id: String,
    #[serde(default = "default_asr_mode")]
    pub asr_mode: String,
}

fn default_resource_id() -> String {
    "volc.seedasr.sauc.duration".to_string()
}

/// DashScope (百炼) provider settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashScopeProviderSettings {
    // Persisted in OS keyring; see DoubaoProviderSettings for the rationale.
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_dashscope_model")]
    pub model: String,
    #[serde(default = "default_region")]
    pub region: String,
}

fn default_dashscope_model() -> String {
    "paraformer-realtime-v2".to_string()
}

fn default_region() -> String {
    "beijing".to_string()
}

impl Default for DashScopeProviderSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_dashscope_model(),
            region: default_region(),
        }
    }
}

/// Qwen ASR (千问) provider settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QwenProviderSettings {
    // Persisted in OS keyring; see DoubaoProviderSettings for the rationale.
    #[serde(default)]
    pub api_key: String,
    #[serde(default = "default_qwen_model")]
    pub model: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default = "default_qwen_language")]
    pub language: String,
}

fn default_qwen_model() -> String {
    "qwen3-asr-flash-realtime".to_string()
}

fn default_qwen_language() -> String {
    "zh".to_string()
}

impl Default for QwenProviderSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: default_qwen_model(),
            region: default_region(),
            language: default_qwen_language(),
        }
    }
}

fn default_provider() -> String {
    "doubao".to_string()
}

impl Default for DoubaoProviderSettings {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            access_token: String::new(),
            resource_id: default_resource_id(),
            asr_mode: default_asr_mode(),
        }
    }
}

/// Application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_provider")]
    pub provider: String,
    #[serde(default)]
    pub doubao: DoubaoProviderSettings,
    #[serde(default)]
    pub dashscope: DashScopeProviderSettings,
    #[serde(default)]
    pub qwen: QwenProviderSettings,
    #[serde(default)]
    pub ai_optimize: ai::AiOptimizeSettings,
    pub hotkey: String,
    pub input_mode: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default = "default_audio_source")]
    pub audio_source: String, // "microphone" | "system"
    #[serde(default = "default_output_mode")]
    pub output_mode: String,
    #[serde(default)]
    pub mic_always_on: bool,
    #[serde(default)]
    pub debug_mode: bool,
    #[serde(default = "default_true")]
    pub filler_enabled: bool,
    #[serde(default)]
    pub replacement_enabled: bool,
    #[serde(default)]
    pub replacement_ignore_case: bool,
    #[serde(default = "default_theme_color")]
    pub theme_color: String,
    #[serde(default = "default_true")]
    pub recording_follows_theme: bool,
    #[serde(default = "default_true")]
    pub show_overlay: bool,
    #[serde(default = "default_close_behavior")]
    pub close_behavior: String,
    #[serde(default)]
    pub onboarding_completed: bool,
    #[serde(default)]
    pub copy_to_clipboard: bool,
    #[serde(default = "default_true")]
    pub paste_restore_clipboard: bool,
    #[serde(default)]
    pub system_no_auto_stop: bool,
    #[serde(default = "default_true")]
    pub esc_abort_enabled: bool,
}

fn default_true() -> bool {
    true
}

fn default_audio_source() -> String {
    "microphone".to_string()
}

fn default_theme_color() -> String {
    "blue".to_string()
}

fn default_close_behavior() -> String {
    "ask".to_string()
}

fn default_output_mode() -> String {
    "type".to_string()
}

fn default_asr_mode() -> String {
    "bistream".to_string()
}

fn normalize_input_mode(value: &str) -> String {
    if value == "hold" {
        "hold".to_string()
    } else {
        "toggle".to_string()
    }
}

fn input_mode_from_str(value: &str) -> InputMode {
    if value == "hold" {
        InputMode::Hold
    } else {
        InputMode::Toggle
    }
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            provider: default_provider(),
            doubao: DoubaoProviderSettings::default(),
            dashscope: DashScopeProviderSettings::default(),
            qwen: QwenProviderSettings::default(),
            ai_optimize: ai::AiOptimizeSettings::default(),
            hotkey: "Ctrl+Shift+V".to_string(),
            input_mode: "toggle".to_string(),
            device_name: String::new(),
            audio_source: default_audio_source(),
            // Default to keystroke simulation — works in all input fields
            // including those that block paste (terminals, native widgets).
            output_mode: "type".to_string(),
            // Default OFF: lazy mic init keeps cold-start light. The trade-off
            // is a small first-record latency, which most users won't notice.
            mic_always_on: false,
            debug_mode: false,
            filler_enabled: true,
            replacement_enabled: false,
            replacement_ignore_case: false,
            theme_color: default_theme_color(),
            // Default ON: keep recording indicator color consistent with the
            // user's chosen theme color (no jarring red flash).
            recording_follows_theme: true,
            show_overlay: true,
            close_behavior: "ask".to_string(),
            onboarding_completed: false,
            copy_to_clipboard: false,
            paste_restore_clipboard: true,
            system_no_auto_stop: false,
            esc_abort_enabled: true,
        }
    }
}

/// Usage statistics (independent of settings).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageStats {
    pub total_sessions: u64,
    pub total_recording_duration_ms: u64,
    pub total_characters: u64,
    pub total_chinese_chars: u64,
}

/// Default status for TranscriptRecord; `serde(default)` uses this when
/// reading legacy records written before the `status` field existed.
fn default_transcript_status() -> String {
    "done".to_string()
}

/// Payload for the `recording-status` frontend event. Previously this was
/// a bare `bool`; carrying `generation` alongside lets the frontend sync its
/// `backendGenerationRef` so that `transcription-update` events from stale
/// sessions can be filtered out.
#[derive(Debug, Clone, Serialize)]
pub struct RecordingStatusPayload {
    pub recording: bool,
    pub generation: u64,
}

/// Payload for the `session-force-abort` event. Carries the backend
/// `recording_generation` of the session being aborted, so the frontend
/// can filter out stale aborts that arrive after the user has already
/// started a new session. See Codex Check 6.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionAbortPayload {
    pub generation: u64,
    pub reason: String,
}

/// A recent transcript record for recovery/backup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptRecord {
    pub id: String,
    pub timestamp: u64,
    pub original: String,
    pub final_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub optimized: Option<String>,
    pub duration_ms: u64,
    /// Completion state: "done" | "partial" | "aborted".
    /// - done: normal completion (all data received, AI optimize succeeded if enabled)
    /// - partial: incomplete (ASR timeout, AI optimize failed, or rescue-path save)
    /// - aborted: user explicitly cancelled (reserved for Batch 2 ESC handling)
    #[serde(default = "default_transcript_status")]
    pub status: String,
}

/// Count CJK characters in text (covers CJK Unified Ideographs + extensions).
fn count_chinese_chars(text: &str) -> u64 {
    text.chars()
        .filter(|c| {
            matches!(*c,
                '\u{4E00}'..='\u{9FFF}'   | // CJK 基本
                '\u{3400}'..='\u{4DBF}'   | // CJK 扩展 A
                '\u{20000}'..='\u{2A6DF}' | // CJK 扩展 B
                '\u{2A700}'..='\u{2CEAF}' | // CJK 扩展 C-F
                '\u{F900}'..='\u{FAFF}'     // 兼容汉字
            )
        })
        .count() as u64
}

/// Shared application state.
struct AppStateInner {
    is_recording: bool,
    /// True from the moment `do_stop_recording_impl` signals stop until the
    /// frontend's `doAutoInput` pipeline finishes (signaled via `mark_session_idle`)
    /// or the 60s safety timeout fires. During this window, new recording
    /// sessions are rejected — protects against the "user hits hotkey again
    /// while previous session's ASR FINAL / AI optimize is still pending" race.
    is_processing: bool,
    recording_generation: u64,
    settings: Arc<AppSettings>,
    mic_manager: Option<MicrophoneManager>,
    loopback: Option<loopback::LoopbackCapture>,
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    pending_settings_warning: Option<String>,
}

struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
    stats: Mutex<UsageStats>,
    /// Cached replacements, providers, prompts — avoid disk I/O on hot paths.
    cached_replacements: Mutex<replacements::TextReplacementsFile>,
    cached_providers: Mutex<ai::providers::AiProvidersFile>,
    cached_prompts: Mutex<ai::prompts::PromptsFile>,
}

fn normalize_settings_for_save(
    mut settings: AppSettings,
) -> Result<(AppSettings, hotkey::ValidatedHotkey, InputMode), String> {
    settings.input_mode = normalize_input_mode(&settings.input_mode);
    let input_mode = input_mode_from_str(&settings.input_mode);
    let hotkey = hotkey::validate_hotkey(&settings.hotkey)?;
    settings.hotkey = hotkey.normalized().to_string();
    Ok((settings, hotkey, input_mode))
}

fn sanitize_loaded_settings(mut settings: AppSettings) -> (AppSettings, Option<String>, bool) {
    let mut should_persist = false;
    let mut warning = None;

    let normalized_input_mode = normalize_input_mode(&settings.input_mode);
    if normalized_input_mode != settings.input_mode {
        settings.input_mode = normalized_input_mode;
        should_persist = true;
    }

    match hotkey::validate_hotkey(&settings.hotkey) {
        Ok(validated) => {
            let normalized = validated.normalized().to_string();
            if normalized != settings.hotkey {
                settings.hotkey = normalized;
                should_persist = true;
            }
        }
        Err(err) => {
            let default_hotkey = hotkey::validate_hotkey(&AppSettings::default().hotkey)
                .expect("default hotkey must be valid");
            settings.hotkey = default_hotkey.normalized().to_string();
            should_persist = true;
            warning = Some(format!(
                "检测到无效热键配置（{}），已回退为 {}",
                err, settings.hotkey
            ));
        }
    }

    // Sanitize provider value
    let valid_providers = ["doubao", "dashscope", "qwen"];
    if !valid_providers.contains(&settings.provider.as_str()) {
        settings.provider = default_provider();
        should_persist = true;
    }

    // Sanitize region values
    let valid_regions = ["beijing", "singapore"];
    if !valid_regions.contains(&settings.dashscope.region.as_str()) {
        settings.dashscope.region = default_region();
        should_persist = true;
    }
    if !valid_regions.contains(&settings.qwen.region.as_str()) {
        settings.qwen.region = default_region();
        should_persist = true;
    }

    (settings, warning, should_persist)
}

/// Validate that the current provider has required credentials configured.
fn validate_provider_credentials(settings: &AppSettings) -> Result<(), String> {
    match settings.provider.as_str() {
        "doubao" => {
            if settings.doubao.app_id.is_empty() || settings.doubao.access_token.is_empty() {
                return Err("请先配置豆包的 App ID 和 Access Token".to_string());
            }
        }
        "dashscope" => {
            if settings.dashscope.api_key.is_empty() {
                return Err("请先配置百炼语音识别的 API Key".to_string());
            }
        }
        "qwen" => {
            if settings.qwen.api_key.is_empty() {
                return Err("请先配置千问语音识别的 API Key".to_string());
            }
        }
        _ => return Err(format!("未知供应商: {}", settings.provider)),
    }
    Ok(())
}

/// Emit a network log event to the frontend.
fn emit_log(app_handle: &tauri::AppHandle, level: &str, msg: &str) {
    let ts = chrono_now();
    let log_entry = serde_json::json!({ "ts": ts, "level": level, "msg": msg });
    let _ = app_handle.emit("network-log", log_entry.to_string());
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs() % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    let ms = now.subsec_millis();
    format!("{:02}:{:02}:{:02}.{:03}", h, m, s, ms)
}

// ── Tauri Commands ──

#[tauri::command]
fn get_settings(state: tauri::State<'_, AppState>) -> AppSettings {
    (*state.inner.lock().settings).clone()
}

#[tauri::command]
fn save_settings(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let (settings, validated_hotkey, input_mode) = normalize_settings_for_save(settings)?;
    storage::save_settings(&app_handle, &settings)?;

    let should_stop_recording = {
        let inner = state.inner.lock();
        inner.is_recording
            && (inner.settings.hotkey != settings.hotkey
                || inner.settings.input_mode != settings.input_mode)
    };
    if should_stop_recording {
        do_stop_recording_impl(&app_handle, &state.inner);
    }

    hotkey::update_config(&validated_hotkey, input_mode);
    hotkey::update_escape_abort_config(settings.esc_abort_enabled);

    let mut inner = state.inner.lock();
    let old_device = inner.settings.device_name.clone();
    let old_mic_always_on = inner.settings.mic_always_on;

    // Handle mic_always_on toggle
    if settings.mic_always_on != old_mic_always_on {
        if settings.mic_always_on {
            // Switching ON: create persistent mic manager
            if inner.mic_manager.is_none() {
                let dev = if settings.device_name.is_empty() {
                    None
                } else {
                    Some(settings.device_name.as_str())
                };
                match MicrophoneManager::new(dev) {
                    Ok(m) => {
                        inner.mic_manager = Some(m);
                    }
                    Err(e) => log::error!("Failed to init mic: {}", e),
                }
            }
        } else {
            // Switching OFF: tear down persistent mic manager (if not recording)
            if !inner.is_recording {
                inner.mic_manager = None;
            }
        }
    } else if settings.device_name != old_device {
        // Device changed, same mode
        if let Some(ref mic) = inner.mic_manager {
            let dev = if settings.device_name.is_empty() {
                None
            } else {
                Some(settings.device_name.as_str())
            };
            mic.switch_device(dev);
        }
    }

    inner.settings = Arc::new(settings.clone());
    Ok(settings)
}

#[tauri::command]
fn emit_pending_settings_warning(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let warning = state.inner.lock().pending_settings_warning.take();
    if let Some(msg) = warning {
        let _ = app_handle.emit("settings-warning", msg);
    }
    Ok(())
}

/// Get usage statistics.
#[tauri::command]
fn get_usage_stats(state: tauri::State<'_, AppState>) -> UsageStats {
    state.stats.lock().clone()
}

/// Update usage statistics after a recording session.
#[tauri::command]
fn update_usage_stats(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    session_duration_ms: u64,
    text: String,
) -> Result<(), String> {
    let mut stats = state.stats.lock();
    stats.total_sessions += 1;
    stats.total_recording_duration_ms += session_duration_ms;
    stats.total_characters += text.chars().count() as u64;
    stats.total_chinese_chars += count_chinese_chars(&text);
    storage::save_usage_stats(&app_handle, &stats)
}

/// Reset usage statistics.
#[tauri::command]
fn reset_usage_stats(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let mut stats = state.stats.lock();
    *stats = UsageStats::default();
    storage::save_usage_stats(&app_handle, &stats)
}

/// List available audio input devices.
#[tauri::command]
fn list_audio_devices() -> Vec<String> {
    audio::list_input_devices()
}


/// Get text replacement pairs.
#[tauri::command]
fn get_replacements(app_handle: tauri::AppHandle) -> replacements::TextReplacementsFile {
    replacements::load_replacements(&app_handle)
}

/// Save text replacement pairs with validation.
#[tauri::command]
fn save_replacements(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    data: replacements::TextReplacementsFile,
) -> Result<(), String> {
    const MAX_REPLACEMENTS: usize = 200;
    if data.replacements.len() > MAX_REPLACEMENTS {
        return Err(format!("替换规则不能超过 {} 条", MAX_REPLACEMENTS));
    }
    for r in &data.replacements {
        if r.from.trim().is_empty() {
            return Err("替换词的原文不能为空".to_string());
        }
        if r.from == r.to {
            return Err(format!("替换词「{}」的原文和替换内容相同", r.from));
        }
    }
    replacements::save_replacements(&app_handle, &data)?;
    *state.cached_replacements.lock() = data;
    Ok(())
}

/// Shared text processing pipeline: filler clean → text replacement.
/// Returns (processed_text, output_mode, copy_to_clipboard, restore_clipboard).
fn process_text(
    app_handle: &tauri::AppHandle,
    state: &Arc<Mutex<AppStateInner>>,
    text: String,
    has_ai_result: bool,
) -> (String, input::OutputMode, bool, bool) {
    let (mode, should_filter, should_replace, ignore_case, copy_to_clipboard, restore_clipboard) = {
        let inner = state.lock();
        let s = &inner.settings;
        (
            input::OutputMode::from_str(&s.output_mode),
            // Skip filler cleaning if AI already processed the text
            s.filler_enabled && !has_ai_result,
            s.replacement_enabled,
            s.replacement_ignore_case,
            s.copy_to_clipboard,
            s.paste_restore_clipboard,
        )
    };
    let text = if should_filter {
        filler::clean_pure_fillers(&text)
    } else {
        text
    };
    let text = if should_replace {
        let cached = app_handle
            .try_state::<AppState>()
            .map(|s| s.cached_replacements.lock().clone());
        if let Some(file) = cached {
            if file.replacements.is_empty() {
                text
            } else {
                replacements::apply_replacements(&text, &file.replacements, ignore_case)
            }
        } else {
            text
        }
    } else {
        text
    };
    (text, mode, copy_to_clipboard, restore_clipboard)
}

/// Execute text output to the focused window.
fn do_output(text: &str, mode: input::OutputMode, copy_to_clipboard: bool, restore_clipboard: bool) {
    if (mode != input::OutputMode::None || copy_to_clipboard) && !text.is_empty() {
        let text = text.to_string();
        std::thread::spawn(move || {
            input::send_text(&text, mode, copy_to_clipboard, restore_clipboard);
        });
    }
}

/// Maximum text length accepted by send commands (100K chars, far exceeds normal ASR output).
const MAX_TEXT_LEN: usize = 100_000;

/// Send text to the focused input field using the configured output mode.
/// Pipeline: filler clean → text replacement → output.
#[tauri::command]
fn send_text_input(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    text: String,
) {
    if text.len() > MAX_TEXT_LEN {
        log::warn!("文本过长 ({} bytes)，已丢弃", text.len());
        return;
    }
    let (processed, mode, copy, restore) = process_text(&app_handle, &state.inner, text, false);
    do_output(&processed, mode, copy, restore);
}

/// Persist a transcript record to the history store WITHOUT any text output.
/// Returns the record id so the frontend can later call `update_transcript_optimized`
/// to fill in the AI-optimized version once it arrives.
///
/// This is the "先保文本" primitive — called at the start of `doAutoInput` and
/// also from the rescue path in the new-session handler. Separated from
/// `send_text_input` so the rescue path can persist pending text without
/// ghost-typing it into whatever window currently has focus.
#[tauri::command]
fn save_transcript_record(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    original: String,
    optimized: Option<String>,
    duration_ms: u64,
    status: String,
) -> Result<String, String> {
    if original.len() > MAX_TEXT_LEN
        || optimized.as_ref().map_or(false, |s| s.len() > MAX_TEXT_LEN)
    {
        return Err("text_too_long".to_string());
    }
    let has_ai = optimized.is_some();
    let input_text = optimized.clone().unwrap_or_else(|| original.clone());
    let (final_text, _mode, _copy, _restore) =
        process_text(&app_handle, &state.inner, input_text, has_ai);

    if final_text.trim().is_empty() {
        return Err("empty_text".to_string());
    }

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    // [修订 R1] id 同时携带时间戳和 generation，避免同毫秒冲突（rescue + doAutoInput）
    let generation = state.inner.lock().recording_generation;
    let id = format!("{}-{}", now_ms, generation);

    let record = TranscriptRecord {
        id: id.clone(),
        timestamp: now_ms,
        original,
        final_text,
        optimized,
        duration_ms,
        status,
    };

    storage::append_transcript_record(&app_handle, record)
        .map_err(|e| format!("persist failed: {}", e))?;

    Ok(id)
}

/// Update the `optimized` field of a previously saved record, and promote
/// its status from "partial" → "done" if applicable. Called when AI optimize
/// completes after the raw record was already persisted.
#[tauri::command]
fn update_transcript_optimized(
    app_handle: tauri::AppHandle,
    id: String,
    optimized: String,
) -> Result<(), String> {
    storage::update_transcript_optimized_by_id(&app_handle, &id, optimized)
}

/// Update an existing record's `status` field. Used by the frontend ESC
/// abort path when the user cancels during AI optimize: the record was
/// already saved as "partial" by `doAutoInput` Step 1, and we want to
/// promote it to "aborted" to reflect the user's actual intent.
#[tauri::command]
fn update_transcript_status(
    app_handle: tauri::AppHandle,
    id: String,
    status: String,
) -> Result<(), String> {
    storage::update_transcript_status_by_id(&app_handle, &id, status)
}

/// Clear the `is_processing` flag to allow new recording sessions to start.
/// Called by the frontend at the end of every `doAutoInput` terminal path
/// (success, AI failure, non-AI). Also auto-cleared by the 65s safety
/// timeout in `do_stop_recording_impl` as a fallback.
///
/// `generation` must match the current `recording_generation` — otherwise
/// this call came from a stale session (e.g. an AI request that hung past
/// the 65s safety timeout, then returned after the user already started a
/// new session). Without this check, a stale call could wrongly clear the
/// gate of the currently-processing session, letting a third session slip
/// in while the current one is still wrapping up.
#[tauri::command]
fn mark_session_idle(state: tauri::State<'_, AppState>, generation: u64) {
    let mut inner = state.inner.lock();
    if !inner.is_processing {
        return;
    }
    if inner.recording_generation != generation {
        log::info!(
            "[mark_session_idle] stale call from generation {} ignored (current is {})",
            generation, inner.recording_generation
        );
        return;
    }
    inner.is_processing = false;
    log::info!(
        "[mark_session_idle] cleared processing for generation {}",
        generation
    );
}

#[tauri::command]
fn get_transcript_records(app_handle: tauri::AppHandle) -> Vec<TranscriptRecord> {
    storage::load_transcript_records(&app_handle)
}

#[tauri::command]
fn clear_transcript_records(app_handle: tauri::AppHandle) -> Result<(), String> {
    storage::clear_transcript_records(&app_handle)
}

/// Quit the application (used by frontend close behavior).
#[tauri::command]
fn quit_app(app_handle: tauri::AppHandle) {
    app_handle.exit(0);
}

#[tauri::command]
async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let settings = {
        let inner = state.inner.lock();
        // Reject while the previous session is still wrapping up — same
        // gate as the global hotkey path, keeps the two entry points in sync.
        if inner.is_processing {
            let _ = app_handle.emit("session-busy", "仍在处理中，请稍候");
            return Err("session busy".to_string());
        }
        validate_provider_credentials(&inner.settings)?;
        Arc::clone(&inner.settings)
    };

    do_start_recording_impl(&app_handle, &state.inner, &settings)
}

#[tauri::command]
async fn stop_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    do_stop_recording_impl(&app_handle, &state.inner);
    Ok(())
}

#[tauri::command]
fn abort_current_session(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    abort_current_session_impl(&app_handle, &state.inner);
    Ok(())
}

#[tauri::command]
fn set_escape_abort_active(active: bool) {
    hotkey::set_escape_abort_active(active);
}

/// Shared recording start logic (used by both Tauri command and hotkey).
fn do_start_recording_impl(
    app_handle: &tauri::AppHandle,
    state: &Arc<Mutex<AppStateInner>>,
    settings: &Arc<AppSettings>,
) -> Result<(), String> {
    // Validate credentials before any state changes
    validate_provider_credentials(settings)?;

    // Check state and set is_recording first, then init outside lock
    // to avoid blocking the entire AppState for up to 3 seconds.
    let use_loopback = settings.audio_source == "system";
    let (needs_mic, generation) = {
        let mut inner = state.lock();
        if inner.is_recording {
            return Ok(());
        }
        inner.is_recording = true;
        inner.recording_generation += 1;
        (!use_loopback && inner.mic_manager.is_none(), inner.recording_generation)
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();

    if use_loopback {
        // System audio (WASAPI loopback) — no auto-stop
        emit_log(app_handle, "info", "启动录音 (系统声音捕获)");
        match loopback::LoopbackCapture::start(audio_tx) {
            Ok(lb) => {
                let mut inner = state.lock();
                inner.stop_tx = Some(stop_tx);
                inner.loopback = Some(lb);
            }
            Err(e) => {
                state.lock().is_recording = false;
                return Err(format!("系统声音捕获失败: {}", e));
            }
        }
    } else {
        // Microphone mode (existing logic)
        if needs_mic {
            let dev = if settings.device_name.is_empty() {
                None
            } else {
                Some(settings.device_name.as_str())
            };
            match MicrophoneManager::new(dev) {
                Ok(m) => {
                    state.lock().mic_manager = Some(m);
                }
                Err(e) => {
                    state.lock().is_recording = false;
                    return Err(e);
                }
            }
        }

        emit_log(app_handle, "info", "启动录音 (麦克风已就绪)");

        // Start forwarding audio with silence auto-stop callback.
        // IMPORTANT: store stop_tx BEFORE start_forwarding, so that if the silence
        // callback fires immediately, do_stop_recording_impl can find stop_tx.
        {
            let state_for_silence = state.clone();
            let app_for_silence = app_handle.clone();
            let mut inner = state.lock();
            inner.stop_tx = Some(stop_tx);
            inner.mic_manager.as_ref().unwrap().start_forwarding(
                audio_tx,
                Some(move || {
                    emit_log(&app_for_silence, "info", "检测到长时间静音，自动停止录音");
                    do_stop_recording_impl(&app_for_silence, &state_for_silence);
                }),
            );
        }
    }

    let _ = app_handle.emit(
        "recording-status",
        RecordingStatusPayload {
            recording: true,
            generation,
        },
    );

    let app_handle_clone = app_handle.clone();
    let state_clone = state.clone();

    use asr::doubao::DoubaoMode;
    match settings.provider.as_str() {
        "dashscope" => {
            let provider = DashScopeProvider {
                settings: DashScopeSettings {
                    api_key: settings.dashscope.api_key.clone(),
                    model: settings.dashscope.model.clone(),
                    region: settings.dashscope.region.clone(),
                },
            };
            spawn_asr_task(provider, app_handle_clone, audio_rx, stop_rx, state_clone, generation);
        }
        "qwen" => {
            let provider = QwenProvider {
                settings: QwenSettings {
                    api_key: settings.qwen.api_key.clone(),
                    model: settings.qwen.model.clone(),
                    region: settings.qwen.region.clone(),
                    language: settings.qwen.language.clone(),
                },
            };
            spawn_asr_task(provider, app_handle_clone, audio_rx, stop_rx, state_clone, generation);
        }
        _ => {
            // Default: Doubao
            let provider = DoubaoProvider {
                settings: DoubaoSettings {
                    app_id: settings.doubao.app_id.clone(),
                    access_token: settings.doubao.access_token.clone(),
                    resource_id: settings.doubao.resource_id.clone(),
                    mode: match settings.doubao.asr_mode.as_str() {
                        "nostream" => DoubaoMode::NoStream,
                        _ => DoubaoMode::BiStream,
                    },
                },
            };
            spawn_asr_task(provider, app_handle_clone, audio_rx, stop_rx, state_clone, generation);
        }
    }

    Ok(())
}

fn abort_current_session_impl(app_handle: &tauri::AppHandle, state: &Arc<Mutex<AppStateInner>>) {
    // Capture the generation we're aborting BEFORE any state mutation —
    // see Codex Check 6: abort releases the gate immediately, so the user
    // could start a new session before the `session-force-abort` event
    // is delivered. The frontend uses this `generation` to filter out
    // stale aborts and only act on the matching session.
    let aborting_generation = state.lock().recording_generation;
    let is_recording = state.lock().is_recording;
    if is_recording {
        let _ = app_handle.emit("recording-cancelled", "escape");
        do_stop_recording_impl(app_handle, state);
        {
            let mut inner = state.lock();
            if inner.is_processing {
                inner.is_processing = false;
                log::info!(
                    "[abort] cleared is_processing immediately after stop for generation {}",
                    inner.recording_generation
                );
            }
        }
        let _ = app_handle.emit(
            "session-force-abort",
            SessionAbortPayload {
                generation: aborting_generation,
                reason: "escape".to_string(),
            },
        );
    } else {
        {
            let mut inner = state.lock();
            if inner.is_processing {
                inner.is_processing = false;
                log::info!(
                    "[abort] cleared is_processing while session was post-processing for generation {}",
                    inner.recording_generation
                );
            }
        }
        let _ = app_handle.emit(
            "session-force-abort",
            SessionAbortPayload {
                generation: aborting_generation,
                reason: "escape".to_string(),
            },
        );
    }
}

/// Spawn an ASR session task for any provider. Generic over the provider type
/// to avoid dynamic dispatch (AsrProvider uses RPITIT, not object-safe).
fn spawn_asr_task<P: AsrProvider + 'static>(
    provider: P,
    app_handle: tauri::AppHandle,
    audio_rx: tokio::sync::mpsc::UnboundedReceiver<crate::audio::AudioFrame>,
    stop_rx: tokio::sync::oneshot::Receiver<()>,
    state: Arc<Mutex<AppStateInner>>,
    generation: u64,
) {
    tauri::async_runtime::spawn(async move {
        if let Err(e) = provider
            .run_session(app_handle.clone(), generation, audio_rx, stop_rx)
            .await
        {
            log::error!("ASR session error: {}", e);
            let _ = app_handle.emit("connection-status", false);
            let _ = app_handle.emit("asr-error", e);
        }

        let loopback = {
            let mut inner = state.lock();
            let lb = if inner.is_recording && inner.recording_generation == generation {
                // ASR session ended abnormally (not via user stop).
                // Only clean up if this is still OUR session (generation matches).
                inner.is_recording = false;
                // Note: do NOT set is_processing=true here. Abnormal exit means
                // the frontend will receive `asr-error` and close the overlay
                // without running doAutoInput — there's no pending work to
                // protect, and the frontend won't call `mark_session_idle`.
                let lb = inner.loopback.take();
                if let Some(ref mic) = inner.mic_manager {
                    mic.stop_forwarding();
                }
                let _ = app_handle.emit(
                    "recording-status",
                    RecordingStatusPayload {
                        recording: false,
                        generation,
                    },
                );
                lb
            } else {
                None
            };
            // Release mic in on-demand mode after session ends (covers both normal and abnormal exits)
            if !inner.settings.mic_always_on && !inner.is_recording {
                inner.mic_manager = None;
            }
            lb
        };
        // Stop loopback outside lock to avoid blocking state access
        if let Some(mut lb) = loopback {
            lb.stop();
        }
    });
}

/// Shared recording stop logic.
fn do_stop_recording_impl(app_handle: &tauri::AppHandle, state: &Arc<Mutex<AppStateInner>>) {
    let generation;
    let loopback = {
        let mut inner = state.lock();
        if !inner.is_recording {
            return;
        }

        // Signal ASR loop to stop FIRST — this lets the loop exit via the stop_rx
        // path and send the final packet before the audio channel closes.
        if let Some(stop_tx) = inner.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        // Take loopback out of lock — stop() calls thread::join() which may block
        let lb = inner.loopback.take();
        // Then stop forwarding audio (closes the sender, stream stays open)
        if let Some(ref mic) = inner.mic_manager {
            mic.stop_forwarding();
        }
        // Atomic transition in the same critical section:
        //   is_recording: true → false
        //   is_processing: false → true   ← blocks new sessions from hotkey/button
        // until the frontend's doAutoInput pipeline calls `mark_session_idle`.
        inner.is_recording = false;
        inner.is_processing = true;
        generation = inner.recording_generation;
        lb
    }; // lock released here

    // Stop loopback AFTER releasing lock to avoid blocking state access
    if let Some(mut lb) = loopback {
        lb.stop();
    }

    // Safety timeout: if the frontend never calls `mark_session_idle`
    // (JS crash, AI optimize hang, etc.), force-clear `is_processing`
    // after 65 seconds. This is slightly longer than the AI optimize
    // default `max_request_secs=60` to avoid racing with a late success.
    // Only clears if the generation still matches — i.e., the user hasn't
    // already started a new session that was approved by some other path.
    {
        let state_clone = state.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(65)).await;
            let mut inner = state_clone.lock();
            if inner.is_processing && inner.recording_generation == generation {
                inner.is_processing = false;
                log::warn!(
                    "[is_processing safety timeout] force-cleared for generation {}",
                    generation
                );
            }
        });
    }

    emit_log(app_handle, "info", "录音已停止");
    let _ = app_handle.emit(
        "recording-status",
        RecordingStatusPayload {
            recording: false,
            generation,
        },
    );
}

// ── App Setup ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            emit_pending_settings_warning,
            start_recording,
            stop_recording,
            abort_current_session,
            set_escape_abort_active,
            list_audio_devices,
            send_text_input,
            save_transcript_record,
            update_transcript_optimized,
            update_transcript_status,
            mark_session_idle,
            get_transcript_records,
            clear_transcript_records,
            get_usage_stats,
            update_usage_stats,
            reset_usage_stats,
            get_replacements,
            save_replacements,
            quit_app,
            ai::get_ai_providers,
            ai::add_ai_provider,
            ai::update_ai_provider,
            ai::delete_ai_provider,
            ai::has_ai_provider_key,
            ai::set_ai_provider_key,
            ai::clear_ai_provider_key,
            ai::test_ai_provider,
            ai::ai_optimize_text,
            ai::get_prompts,
            ai::save_prompts,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Load and sanitize settings (requires AppHandle for store plugin)
            let (settings, mut startup_warning, should_persist) =
                sanitize_loaded_settings(storage::load_settings(&app_handle));
            if should_persist {
                if let Err(err) = storage::save_settings(&app_handle, &settings) {
                    let msg = format!("启动时规范化设置失败: {}", err);
                    log::error!("{}", msg);
                    startup_warning = Some(match startup_warning {
                        Some(existing) => format!("{}；{}", existing, msg),
                        None => msg,
                    });
                }
            }

            let input_mode = input_mode_from_str(&settings.input_mode);
            let hotkey =
                hotkey::validate_hotkey(&settings.hotkey).expect("sanitized hotkey must be valid");
            hotkey::update_escape_abort_config(settings.esc_abort_enabled);
            hotkey::set_escape_abort_active(false);

            // Initialize microphone (only if always-on mode)
            let mic_manager = if settings.mic_always_on {
                let device_name = if settings.device_name.is_empty() {
                    None
                } else {
                    Some(settings.device_name.as_str())
                };
                match MicrophoneManager::new(device_name) {
                    Ok(m) => {
                        log::info!("Microphone manager initialized (always-on)");
                        Some(m)
                    }
                    Err(e) => {
                        log::error!("Failed to init microphone: {}", e);
                        None
                    }
                }
            } else {
                log::info!("Microphone on-demand mode (not pre-initialized)");
                None
            };

            let usage_stats = storage::load_usage_stats(&app_handle);

            let state = AppState {
                inner: Arc::new(Mutex::new(AppStateInner {
                    is_recording: false,
                    is_processing: false,
                    recording_generation: 0,
                    settings: Arc::new(settings.clone()),
                    mic_manager,
                    loopback: None,
                    stop_tx: None,
                    pending_settings_warning: startup_warning,
                })),
                stats: Mutex::new(usage_stats),
                cached_replacements: Mutex::new(replacements::load_replacements(&app_handle)),
                cached_providers: Mutex::new(ai::providers::load_providers(&app_handle)),
                cached_prompts: Mutex::new(ai::prompts::load_prompts(&app_handle)),
            };

            let state_inner = state.inner.clone();
            app.manage(state);

            // ── System Tray ──
            {
                use tauri::menu::{
                    CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
                };
                use tauri::tray::{
                    MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
                };

                let show_item =
                    MenuItemBuilder::with_id("tray_show", "打开窗口").build(app)?;
                let ai_item = CheckMenuItemBuilder::with_id("tray_ai", "AI 优化")
                    .checked(settings.ai_optimize.enabled)
                    .build(app)?;
                let sep = PredefinedMenuItem::separator(app)?;
                let about_item =
                    MenuItemBuilder::with_id("tray_about", "关于 SpeakIn").build(app)?;
                let quit_item =
                    MenuItemBuilder::with_id("tray_quit", "退出").build(app)?;
                let menu = MenuBuilder::new(app)
                    .items(&[&show_item, &ai_item, &sep, &about_item, &quit_item])
                    .build()?;

                let tray_state = state_inner.clone();
                let tray_app = app_handle.clone();

                // Tray icon: load a small PNG (32x32) directly so Windows
                // doesn't have to downscale a 512x512 master at runtime.
                // See `scripts/gen_icons.py` for how this asset is generated.
                let tray_icon = tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray.png"
                ))?;

                TrayIconBuilder::new()
                    .icon(tray_icon)
                    .tooltip("SpeakIn 声入")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(move |app, event| {
                        match event.id().as_ref() {
                            "tray_show" => {
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                            }
                            "tray_ai" => {
                                // Toggle AI optimize and persist.
                                // Clone settings and drop lock BEFORE save (keyring I/O).
                                let new_settings = {
                                    let mut inner = tray_state.lock();
                                    let mut s = (*inner.settings).clone();
                                    s.ai_optimize.enabled = !s.ai_optimize.enabled;
                                    inner.settings = Arc::new(s.clone());
                                    s
                                };
                                let _ = storage::save_settings(&tray_app, &new_settings);
                                let _ = tray_app.emit("settings-changed", ());
                            }
                            "tray_about" => {
                                // Show window and emit about event to frontend
                                if let Some(w) = app.get_webview_window("main") {
                                    let _ = w.show();
                                    let _ = w.unminimize();
                                    let _ = w.set_focus();
                                }
                                let _ = app.emit("show-about", ());
                            }
                            "tray_quit" => {
                                app.exit(0);
                            }
                            _ => {}
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            if let Some(w) = tray.app_handle().get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.unminimize();
                                let _ = w.set_focus();
                            }
                        }
                    })
                    .build(app)?;
            }

            // Override the window icon with the simplified tray-style PNG
            // (64x64). Windows displays the taskbar icon at 24~40px, where the
            // detailed "airy" icon.svg loses its fine elements anyway. Using
            // the simplified solid-blue version reads better next to other
            // colored taskbar icons (Discord/VSCode/Telegram style), and is
            // visually consistent with the system tray icon.
            //
            // We deliberately reuse `tray@2x.png` here instead of adding a
            // separate `window.png`: it's the same 64x64 vector render of
            // tray.svg, so reusing it keeps the icon system to ONE source of
            // truth (tray.svg) for everything small. The "@2x" name is a bit
            // misleading now — it's both the retina tray icon AND the window
            // icon source. See `scripts/gen_icons.py`.
            if let Some(window) = app.get_webview_window("main") {
                if let Ok(icon) = tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray@2x.png"
                )) {
                    let _ = window.set_icon(icon);
                }
            }

            let hotkey_rx = hotkey::start_listener(&hotkey, input_mode);
            std::thread::spawn(move || {
                let mut last_event_time = std::time::Instant::now()
                    .checked_sub(std::time::Duration::from_secs(1))
                    .unwrap_or_else(std::time::Instant::now);
                const HOTKEY_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(300);
                const MIN_HOLD_DURATION: std::time::Duration = std::time::Duration::from_millis(300);
                let mut hold_start_instant: Option<std::time::Instant> = None;

                while let Ok(event) = hotkey_rx.recv() {
                    let app_handle = app_handle.clone();
                    let state = state_inner.clone();

                    match event {
                        HotkeyEvent::ShortPress => {
                            // Cooldown: ignore rapid consecutive start/toggle events
                            let now = std::time::Instant::now();
                            if now.duration_since(last_event_time) < HOTKEY_COOLDOWN {
                                log::debug!("ShortPress ignored (cooldown)");
                                continue;
                            }
                            last_event_time = now;
                            // ⚠️ Mutex safety: must drop guard before calling
                            // do_stop/do_start which re-lock the same mutex.
                            let (is_recording, is_processing, settings) = {
                                let inner = state.lock();
                                (
                                    inner.is_recording,
                                    inner.is_processing,
                                    Arc::clone(&inner.settings),
                                )
                            };
                            if is_recording {
                                do_stop_recording_impl(&app_handle, &state);
                            } else if is_processing {
                                // Previous session still wrapping up (ASR final /
                                // AI optimize). Reject and notify frontend.
                                last_event_time -= HOTKEY_COOLDOWN; // don't burn the cooldown
                                let _ = app_handle.emit(
                                    "session-busy",
                                    "仍在处理中，请稍候",
                                );
                            } else if let Err(e) =
                                do_start_recording_impl(&app_handle, &state, &settings)
                            {
                                let _ = app_handle.emit("asr-error", format!("启动失败: {}", e));
                            }
                        }
                        HotkeyEvent::HoldStart => {
                            // Cooldown: ignore rapid consecutive start events
                            let now = std::time::Instant::now();
                            if now.duration_since(last_event_time) < HOTKEY_COOLDOWN {
                                log::debug!("HoldStart ignored (cooldown)");
                                hold_start_instant = None;
                                continue;
                            }
                            last_event_time = now;

                            let (is_recording, is_processing, settings) = {
                                let inner = state.lock();
                                (
                                    inner.is_recording,
                                    inner.is_processing,
                                    Arc::clone(&inner.settings),
                                )
                            };
                            if is_recording {
                                // Already recording — ignore (hold mode shouldn't re-enter)
                            } else if is_processing {
                                // Previous session still wrapping up. Reject and notify.
                                last_event_time -= HOTKEY_COOLDOWN;
                                hold_start_instant = None;
                                let _ = app_handle.emit(
                                    "session-busy",
                                    "仍在处理中，请稍候",
                                );
                            } else if let Err(e) =
                                do_start_recording_impl(&app_handle, &state, &settings)
                            {
                                let _ =
                                    app_handle.emit("asr-error", format!("启动失败: {}", e));
                            } else {
                                hold_start_instant = Some(std::time::Instant::now());
                            }
                        }
                        HotkeyEvent::HoldEnd => {
                            // Check for mistouch: if held < MIN_HOLD_DURATION, cancel instead of stop
                            let is_mistouch = hold_start_instant
                                .map(|t| t.elapsed() < MIN_HOLD_DURATION)
                                .unwrap_or(false);
                            hold_start_instant = None;

                            if is_mistouch {
                                log::info!("Hold duration too short, treating as mistouch — cancelling");
                                // Emit cancel BEFORE stop so frontend sees it before recording-status(false)
                                let _ = app_handle.emit("recording-cancelled", "mistouch");
                                do_stop_recording_impl(&app_handle, &state);
                            } else {
                                do_stop_recording_impl(&app_handle, &state);
                            }
                        }
                        HotkeyEvent::AbortSession => {
                            hold_start_instant = None;
                            abort_current_session_impl(&app_handle, &state);
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
