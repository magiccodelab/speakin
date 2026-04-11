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
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{Emitter, Manager};
use windows_sys::Win32::Foundation::SYSTEMTIME;
use windows_sys::Win32::System::SystemInformation::GetLocalTime;

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
    #[serde(default = "default_true")]
    pub show_overlay_subtitle: bool,
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
    #[serde(default = "default_silence_auto_stop_secs")]
    pub silence_auto_stop_secs: u8,
    #[serde(default = "default_vad_sensitivity")]
    pub vad_sensitivity: u8,
    #[serde(default = "default_sound_start")]
    pub sound_preset_start: String,
    #[serde(default = "default_sound_stop")]
    pub sound_preset_stop: String,
    #[serde(default = "default_sound_error")]
    pub sound_preset_error: String,
}

fn default_true() -> bool {
    true
}

fn default_silence_auto_stop_secs() -> u8 {
    6
}

fn default_vad_sensitivity() -> u8 {
    7
}

fn default_sound_start() -> String {
    "default-start".to_string()
}

fn default_sound_stop() -> String {
    "default-stop".to_string()
}

fn default_sound_error() -> String {
    "default-error".to_string()
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
            show_overlay_subtitle: true,
            close_behavior: "ask".to_string(),
            onboarding_completed: false,
            copy_to_clipboard: false,
            paste_restore_clipboard: true,
            system_no_auto_stop: false,
            esc_abort_enabled: true,
            silence_auto_stop_secs: default_silence_auto_stop_secs(),
            vad_sensitivity: default_vad_sensitivity(),
            sound_preset_start: default_sound_start(),
            sound_preset_stop: default_sound_stop(),
            sound_preset_error: default_sound_error(),
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
    /// For `recording: false` emits, indicates whether VAD observed any
    /// speech during the session. `false` → fast-path close on the
    /// frontend (no post-recording flow, no 3s/6s wait). For
    /// `recording: true` emits, always `false` (the field is unused at
    /// start time but kept for schema simplicity).
    #[serde(default)]
    pub had_speech: bool,
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
///
/// **Session lifecycle note (2026-04 refactor)**: there is no more
/// `is_processing` gate. The backend's session ownership ends the moment
/// the ASR task exits — at that point `finalize_session` has already
/// persisted any accumulated text and emitted `session-ended`, so the
/// next recording can start immediately. Any post-session work (AI
/// optimize, text output) runs on the frontend and is session-scoped
/// via generation checks — it cannot block a new session from starting.
struct AppStateInner {
    is_recording: bool,
    recording_generation: u64,
    /// VAD-observed fact for the current session: `true` once
    /// `wait_for_speech` confirms speech start. Cloned into the ASR task
    /// so the provider can flip it lock-free. The provider copies this
    /// into its `SessionOutcome` at exit so `finalize_session` knows
    /// whether to emit `status: "no_speech"`.
    had_speech: Arc<AtomicBool>,
    /// Set to `true` by `abort_current_session_impl` (ESC). The ASR task
    /// reads this at exit to decide whether its `SessionOutcome.aborted`
    /// should be set, which in turn makes `finalize_session` emit
    /// `status: "aborted"` and persist the record with status `aborted`.
    aborted: Arc<AtomicBool>,
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
    settings.ai_optimize.active_provider_id =
        settings.ai_optimize.active_provider_id.trim().to_string();
    settings.ai_optimize.active_prompt_id =
        settings.ai_optimize.active_prompt_id.trim().to_string();
    let input_mode = input_mode_from_str(&settings.input_mode);
    let hotkey = hotkey::validate_hotkey(&settings.hotkey)?;
    settings.hotkey = hotkey.normalized().to_string();
    Ok((settings, hotkey, input_mode))
}

fn append_startup_warning(warning: &mut Option<String>, msg: String) {
    *warning = Some(match warning.take() {
        Some(existing) => format!("{}；{}", existing, msg),
        None => msg,
    });
}

fn has_selected_ai_provider(
    settings: &AppSettings,
    ai_providers: &[ai::providers::AiProvider],
) -> bool {
    !settings.ai_optimize.active_provider_id.is_empty()
        && ai_providers
            .iter()
            .any(|p| p.id == settings.ai_optimize.active_provider_id)
}

fn validate_ai_optimize_settings(
    settings: &AppSettings,
    ai_providers: &[ai::providers::AiProvider],
) -> Result<(), String> {
    if !settings.ai_optimize.enabled {
        return Ok(());
    }
    if settings.ai_optimize.active_provider_id.is_empty() {
        return Err("启用 AI 优化前，请先选择 AI 供应商".to_string());
    }
    if !has_selected_ai_provider(settings, ai_providers) {
        return Err("所选 AI 供应商不存在，请重新选择".to_string());
    }
    Ok(())
}

fn sanitize_loaded_settings(
    mut settings: AppSettings,
    ai_providers: &[ai::providers::AiProvider],
) -> (AppSettings, Option<String>, bool) {
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
            append_startup_warning(
                &mut warning,
                format!("检测到无效热键配置（{}），已回退为 {}", err, settings.hotkey),
            );
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

    // Sanitize VAD settings
    if settings.silence_auto_stop_secs < 3 || settings.silence_auto_stop_secs > 60 {
        settings.silence_auto_stop_secs = default_silence_auto_stop_secs();
        should_persist = true;
    }
    if settings.vad_sensitivity < 1 || settings.vad_sensitivity > 10 {
        settings.vad_sensitivity = default_vad_sensitivity();
        should_persist = true;
    }

    let trimmed_ai_provider = settings.ai_optimize.active_provider_id.trim().to_string();
    if trimmed_ai_provider != settings.ai_optimize.active_provider_id {
        settings.ai_optimize.active_provider_id = trimmed_ai_provider;
        should_persist = true;
    }
    let trimmed_ai_prompt = settings.ai_optimize.active_prompt_id.trim().to_string();
    if trimmed_ai_prompt != settings.ai_optimize.active_prompt_id {
        settings.ai_optimize.active_prompt_id = trimmed_ai_prompt;
        should_persist = true;
    }
    if !settings.ai_optimize.active_provider_id.is_empty()
        && !has_selected_ai_provider(&settings, ai_providers)
    {
        settings.ai_optimize.active_provider_id.clear();
        should_persist = true;
    }
    if settings.ai_optimize.enabled && !has_selected_ai_provider(&settings, ai_providers) {
        settings.ai_optimize.enabled = false;
        should_persist = true;
        append_startup_warning(
            &mut warning,
            "AI 优化已关闭：启用前需要先选择 AI 供应商".to_string(),
        );
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
pub(crate) fn emit_log(app_handle: &tauri::AppHandle, level: &str, msg: &str) {
    let ts = chrono_now();
    let log_entry = serde_json::json!({ "ts": ts, "level": level, "msg": msg });
    let _ = app_handle.emit("network-log", log_entry.to_string());
}

pub(crate) fn emit_app_log(app_handle: &tauri::AppHandle, level: &str, msg: &str) {
    emit_log(app_handle, level, &format!("[APP] {}", msg));
}

fn chrono_now() -> String {
    unsafe {
        let mut now = std::mem::zeroed::<SYSTEMTIME>();
        GetLocalTime(&mut now);
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}",
            now.wYear,
            now.wMonth,
            now.wDay,
            now.wHour,
            now.wMinute,
            now.wSecond,
            now.wMilliseconds
        )
    }
}

fn provider_label(provider: &str) -> &'static str {
    match provider {
        "dashscope" => "百炼",
        "qwen" => "千问",
        _ => "豆包",
    }
}

fn audio_source_label(audio_source: &str) -> &'static str {
    match audio_source {
        "system" => "系统声音",
        _ => "麦克风",
    }
}

fn input_mode_label(input_mode: &str) -> &'static str {
    match input_mode {
        "hold" => "按住说话",
        _ => "按键切换",
    }
}

fn output_mode_label(mode: input::OutputMode) -> &'static str {
    match mode {
        input::OutputMode::Paste => "粘贴",
        input::OutputMode::Type => "模拟输入",
        input::OutputMode::None => "仅保留应用内",
    }
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
    let ai_providers = state.cached_providers.lock().providers.clone();
    validate_ai_optimize_settings(&settings, &ai_providers)?;
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

/// Measure microphone input level over a fixed window (default 3000ms, max 10000ms).
/// Returns peak/avg dBFS so the user can verify their mic gain in advanced settings.
/// Refuses to run while a recording session is active to avoid overlapping streams.
#[tauri::command]
async fn measure_microphone_level(
    state: tauri::State<'_, AppState>,
    device_name: Option<String>,
    duration_ms: Option<u64>,
) -> Result<audio::LevelStats, String> {
    if state.inner.lock().is_recording {
        return Err("录音进行中，无法测试麦克风电平".to_string());
    }
    let dur = duration_ms.unwrap_or(3000).min(10_000);
    tokio::task::spawn_blocking(move || {
        audio::measure_input_level(device_name.as_deref(), dur)
    })
    .await
    .map_err(|e| format!("测量任务执行失败: {}", e))?
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
    if !processed.trim().is_empty() {
        emit_app_log(
            &app_handle,
            "info",
            &format!(
                "已输出文字 [{}，{} 字{}]",
                output_mode_label(mode),
                processed.chars().count(),
                if copy {
                    if restore {
                        "，已同步剪贴板并恢复"
                    } else {
                        "，已同步剪贴板"
                    }
                } else {
                    ""
                }
            ),
        );
    }
    do_output(&processed, mode, copy, restore);
}

/// Update the `optimized` field of a previously saved record, and promote
/// its status from "partial" → "done" if applicable. Called by the frontend
/// after AI optimize completes to append the optimized version onto the
/// record that `finalize_session` already persisted.
#[tauri::command]
fn update_transcript_optimized(
    app_handle: tauri::AppHandle,
    id: String,
    optimized: String,
) -> Result<(), String> {
    if optimized.len() > MAX_TEXT_LEN {
        return Err("text_too_long".to_string());
    }
    let char_count = optimized.chars().count();
    storage::update_transcript_optimized_by_id(&app_handle, &id, optimized)?;
    emit_app_log(
        &app_handle,
        "info",
        &format!("已写入 AI 优化结果 [{} 字]", char_count),
    );
    Ok(())
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

/// Mark a window as non-activatable (WS_EX_NOACTIVATE) so it never
/// steals keyboard focus when clicked.  Used for the recording overlay
/// to prevent hotkey original-action leaking through the WebView.
#[cfg(windows)]
#[tauri::command]
fn set_window_no_activate(app_handle: tauri::AppHandle, label: String) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongW, SetWindowLongW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
    };

    let window = app_handle
        .get_webview_window(&label)
        .ok_or_else(|| format!("窗口 '{}' 不存在", label))?;
    let hwnd = window.hwnd().map_err(|e| format!("获取 HWND 失败: {}", e))?;
    let raw: windows_sys::Win32::Foundation::HWND = hwnd.0 as _;
    unsafe {
        let ex_style = GetWindowLongW(raw, GWL_EXSTYLE);
        SetWindowLongW(raw, GWL_EXSTYLE, ex_style | WS_EX_NOACTIVATE as i32);
    }
    Ok(())
}

#[tauri::command]
async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // With the 2026-04 session-lifecycle refactor there is no more
    // `is_processing` gate. The only thing that prevents a new session from
    // starting is an *actively recording* session — and even that is handled
    // by `do_start_recording_impl` itself (idempotent early-return when
    // `is_recording == true`). Any post-session work (AI optimize, text
    // output) runs on the frontend and cannot block a new recording.
    let settings = {
        let inner = state.inner.lock();
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
    let (needs_mic, generation, had_speech_flag, aborted_flag) = {
        let mut inner = state.lock();
        if inner.is_recording {
            return Ok(());
        }
        inner.is_recording = true;
        inner.recording_generation += 1;
        // [Codex Q4 fix] Create FRESH atomics per session. Previously we
        // reset the existing `Arc<AtomicBool>` to false — but that same
        // Arc was still held by any in-flight old session's ASR task.
        // Resetting it would erase e.g. an ESC abort flag for the old
        // session, causing `finalize_session` to emit "ok/no_speech"
        // instead of "aborted". By allocating new Arcs here, the old
        // session's task keeps observing its own original flag values.
        inner.had_speech = Arc::new(AtomicBool::new(false));
        inner.aborted = Arc::new(AtomicBool::new(false));
        let had_speech_flag = Arc::clone(&inner.had_speech);
        let aborted_flag = Arc::clone(&inner.aborted);
        (
            !use_loopback && inner.mic_manager.is_none(),
            inner.recording_generation,
            had_speech_flag,
            aborted_flag,
        )
    };

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();

    emit_app_log(
        app_handle,
        "info",
        &format!(
            "已开始录音 [{} / {} / {}，会话 {}]",
            audio_source_label(&settings.audio_source),
            provider_label(&settings.provider),
            input_mode_label(&settings.input_mode),
            generation
        ),
    );

    if use_loopback {
        // System audio (WASAPI loopback) — no auto-stop
        emit_log(app_handle, "info", "启动录音 (系统声音捕获)");
        match loopback::LoopbackCapture::start(audio_tx, settings.vad_sensitivity) {
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
            // Apply user-configured VAD settings before this session
            inner.mic_manager.as_ref().unwrap().set_vad_config(
                settings.vad_sensitivity,
                settings.silence_auto_stop_secs,
            );
            inner.mic_manager.as_ref().unwrap().start_forwarding(
                audio_tx,
                Some(move || {
                    emit_app_log(&app_for_silence, "info", "检测到长时间静音，自动结束本次录音");
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
            had_speech: false,
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
            spawn_asr_task(
                provider,
                app_handle_clone,
                audio_rx,
                stop_rx,
                state_clone,
                generation,
                had_speech_flag,
                aborted_flag,
            );
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
            spawn_asr_task(
                provider,
                app_handle_clone,
                audio_rx,
                stop_rx,
                state_clone,
                generation,
                had_speech_flag,
                aborted_flag,
            );
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
            spawn_asr_task(
                provider,
                app_handle_clone,
                audio_rx,
                stop_rx,
                state_clone,
                generation,
                had_speech_flag,
                aborted_flag,
            );
        }
    }

    Ok(())
}

/// Abort the currently-recording session (ESC handler).
///
/// Sets the `aborted` flag so the ASR task's `SessionOutcome` will report
/// `aborted=true`, which in turn makes `finalize_session` emit
/// `session-ended { status: "aborted" }` and persist any already-received
/// finals as an `aborted` record (preserving the user's intent: stop this
/// session, but keep what's already been heard).
///
/// The abort path is now a thin wrapper around `do_stop_recording_impl` —
/// there's no separate "processing" state to deal with because
/// `is_processing` no longer exists. If the session has already moved on
/// to the AI-optimize phase on the frontend, the frontend still listens
/// for `session-force-abort` to clean up its UI state.
fn abort_current_session_impl(app_handle: &tauri::AppHandle, state: &Arc<Mutex<AppStateInner>>) {
    let (aborting_generation, is_recording) = {
        let inner = state.lock();
        (inner.recording_generation, inner.is_recording)
    };

    if is_recording {
        emit_app_log(
            app_handle,
            "info",
            &format!("收到 ESC 中止请求 [录音中，会话 {}]", aborting_generation),
        );
        // Flip the abort flag BEFORE sending stop_tx so the ASR task sees
        // it when it runs through its wrap-up code. Release ordering pairs
        // with the Acquire load in the providers.
        state.lock().aborted.store(true, Ordering::Release);
        let _ = app_handle.emit("recording-cancelled", "escape");
        do_stop_recording_impl(app_handle, state);
    } else {
        emit_app_log(
            app_handle,
            "info",
            &format!("收到 ESC 中止请求 [非录音中，会话 {}]", aborting_generation),
        );
    }

    let _ = app_handle.emit(
        "session-force-abort",
        SessionAbortPayload {
            generation: aborting_generation,
            reason: "escape".to_string(),
        },
    );
}

/// Spawn an ASR session task for any provider. Generic over the provider type
/// to avoid dynamic dispatch (AsrProvider uses RPITIT, not object-safe).
///
/// The provider returns a `SessionOutcome` describing how the session ended.
/// This function is the SINGLE place that calls `finalize_session`, so the
/// `session-ended` event is guaranteed to fire exactly once per spawned task,
/// regardless of which exit path the provider took. It also handles cleanup
/// of the mic/loopback/audio devices and clears `is_recording` if the
/// provider exited without a matching `do_stop_recording_impl` call.
fn spawn_asr_task<P: AsrProvider + 'static>(
    provider: P,
    app_handle: tauri::AppHandle,
    audio_rx: tokio::sync::mpsc::UnboundedReceiver<crate::audio::AudioFrame>,
    stop_rx: tokio::sync::oneshot::Receiver<()>,
    state: Arc<Mutex<AppStateInner>>,
    generation: u64,
    had_speech: Arc<AtomicBool>,
    aborted: Arc<AtomicBool>,
) {
    tauri::async_runtime::spawn(async move {
        let outcome = provider
            .run_session(
                app_handle.clone(),
                generation,
                audio_rx,
                stop_rx,
                had_speech.clone(),
                aborted.clone(),
            )
            .await;

        // [Codex Q3c fix] Do NOT emit `connection-status: false` here.
        // This task runs for the session we spawned; when a new session
        // has already started before we reach this point, emitting false
        // would clear the connected indicator of the active new session.
        // The frontend's `session-ended` handler owns the visual
        // "disconnected" transition, and it's session-scoped.

        // ── Clean up audio devices and clear is_recording if still set ──
        //
        // Two paths arrive here:
        //   (a) user called `do_stop_recording_impl` → is_recording was
        //       already flipped to false there, audio devices stopped there
        //   (b) provider exited on its own (error, FINAL arrived, etc.)
        //       without a user stop → we need to clean up here
        //
        // In both cases we also emit `recording-status: false` so the UI
        // waveform stops. The source of truth for "session is done" is the
        // `session-ended` event emitted below by `finalize_session`.
        let loopback = {
            let mut inner = state.lock();
            let was_still_recording =
                inner.is_recording && inner.recording_generation == generation;
            let lb = if was_still_recording {
                inner.is_recording = false;
                let lb = inner.loopback.take();
                if let Some(ref mic) = inner.mic_manager {
                    mic.stop_forwarding();
                }
                lb
            } else {
                None
            };
            if !inner.settings.mic_always_on && !inner.is_recording {
                inner.mic_manager = None;
            }
            lb
        };
        if let Some(mut lb) = loopback {
            lb.stop();
        }

        // Emit recording-status(false) so the frontend waveform/overlay
        // stops. Carries had_speech for legacy compat with frontend code
        // that still reads it (will be cleaned up in Phase 5).
        let _ = app_handle.emit(
            "recording-status",
            RecordingStatusPayload {
                recording: false,
                generation,
                had_speech: outcome.had_speech,
            },
        );

        // ── The authoritative end-of-session event ──
        // Persists any accumulated text + emits `session-ended`. Exactly
        // once per session, unconditionally.
        asr::finalize_session(&app_handle, generation, outcome);
    });
}

/// Shared recording stop logic.
///
/// **2026-04 refactor**: no more `is_processing` gate, no more 65s safety
/// timeout, no `recording-status(false)` emitted from here. All of that
/// moved into `spawn_asr_task`, which owns the session-ended moment. This
/// function is now purely "stop audio capture devices and signal the ASR
/// task to wrap up" — the ASR task will drive the rest of the lifecycle.
fn do_stop_recording_impl(app_handle: &tauri::AppHandle, state: &Arc<Mutex<AppStateInner>>) {
    let (generation, had_speech, loopback) = {
        let mut inner = state.lock();
        if !inner.is_recording {
            return;
        }

        // Acquire pairs with the Release in `wait_for_speech` to establish
        // the happens-before needed on weakly-ordered architectures.
        let had_speech = inner.had_speech.load(Ordering::Acquire);

        // Signal ASR loop to stop FIRST — this lets the loop exit via the
        // stop_rx path and send the final packet before the audio channel
        // closes.
        if let Some(stop_tx) = inner.stop_tx.take() {
            let _ = stop_tx.send(());
        }
        // Take loopback out of lock — stop() calls thread::join() which
        // may block.
        let lb = inner.loopback.take();
        // Stop forwarding audio (closes the sender, stream stays open).
        if let Some(ref mic) = inner.mic_manager {
            mic.stop_forwarding();
        }
        // Flip is_recording=false immediately. The ASR task's wrap-up
        // (sending final packet, waiting for FINAL) continues in the
        // background and, critically, **does not block a new recording**
        // from starting. The next `do_start_recording_impl` will
        // increment generation and spin up a fresh session.
        inner.is_recording = false;
        let generation = inner.recording_generation;
        (generation, had_speech, lb)
    }; // lock released here

    // Stop loopback AFTER releasing lock to avoid blocking state access.
    if let Some(mut lb) = loopback {
        lb.stop();
    }

    let stop_message = if had_speech {
        format!("已结束录音，等待识别结果 [会话 {}]", generation)
    } else {
        format!("已结束录音，未检测到有效语音 [会话 {}]", generation)
    };
    emit_app_log(app_handle, "info", &stop_message);
    emit_log(
        app_handle,
        "info",
        if had_speech {
            "录音已停止"
        } else {
            "录音已停止 (未检测到语音)"
        },
    );
    // NOTE: `recording-status(false)` is emitted by spawn_asr_task once the
    // ASR task actually exits — that's the honest moment to tell the UI
    // "the session is really done", and it's paired with `session-ended`
    // which carries the full result. Emitting it here would be a lie
    // (the ASR task may still be sending the final audio packet).
}

// ── System Tray Menu ──

/// Build the full tray right-click menu from current settings, prompts,
/// and AI providers. Called at startup and whenever any of these change.
fn build_tray_menu(
    app: &tauri::AppHandle,
    settings: &AppSettings,
    prompts: &[ai::prompts::PromptTemplate],
    ai_providers: &[ai::providers::AiProvider],
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{
        CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, PredefinedMenuItem, SubmenuBuilder,
    };

    let show_item = MenuItemBuilder::with_id("tray_show", "打开窗口").build(app)?;
    let sep1 = PredefinedMenuItem::separator(app)?;

    // ── AI 优化区 ──
    let can_enable_ai = has_selected_ai_provider(settings, ai_providers);
    let ai_item = CheckMenuItemBuilder::with_id("tray_ai", "AI 优化")
        .checked(settings.ai_optimize.enabled && can_enable_ai)
        .enabled(settings.ai_optimize.enabled || can_enable_ai)
        .build(app)?;

    // 提示词子菜单
    let mut prompt_sub = SubmenuBuilder::with_id(app, "tray_prompt_sub", "提示词");
    if prompts.is_empty() {
        prompt_sub = prompt_sub.item(
            &MenuItemBuilder::with_id("tray_prompt_empty", "(无提示词)")
                .enabled(false)
                .build(app)?,
        );
    } else {
        for p in prompts {
            prompt_sub = prompt_sub.item(
                &CheckMenuItemBuilder::with_id(format!("tray_prompt_{}", p.id), &p.name)
                    .checked(settings.ai_optimize.active_prompt_id == p.id)
                    .build(app)?,
            );
        }
    }
    let prompt_submenu = prompt_sub.build()?;

    // AI 供应商子菜单
    let mut aiprov_sub = SubmenuBuilder::with_id(app, "tray_aiprov_sub", "AI 供应商");
    if ai_providers.is_empty() {
        aiprov_sub = aiprov_sub.item(
            &MenuItemBuilder::with_id("tray_aiprov_empty", "(未配置)")
                .enabled(false)
                .build(app)?,
        );
    } else {
        for p in ai_providers {
            aiprov_sub = aiprov_sub.item(
                &CheckMenuItemBuilder::with_id(format!("tray_aiprov_{}", p.id), &p.name)
                    .checked(settings.ai_optimize.active_provider_id == p.id)
                    .build(app)?,
            );
        }
    }
    let aiprov_submenu = aiprov_sub.build()?;

    let sep2 = PredefinedMenuItem::separator(app)?;

    // ── 识别/音频/输出区 ──
    let asr_submenu = SubmenuBuilder::with_id(app, "tray_asr_sub", "识别供应商")
        .item(
            &CheckMenuItemBuilder::with_id("tray_asr_doubao", "豆包")
                .checked(settings.provider == "doubao")
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("tray_asr_dashscope", "百炼")
                .checked(settings.provider == "dashscope")
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("tray_asr_qwen", "千问")
                .checked(settings.provider == "qwen")
                .build(app)?,
        )
        .build()?;

    let audio_submenu = SubmenuBuilder::with_id(app, "tray_audio_sub", "音频来源")
        .item(
            &CheckMenuItemBuilder::with_id("tray_audio_microphone", "麦克风")
                .checked(settings.audio_source == "microphone")
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("tray_audio_system", "系统声音")
                .checked(settings.audio_source == "system")
                .build(app)?,
        )
        .build()?;

    let output_submenu = SubmenuBuilder::with_id(app, "tray_output_sub", "输出方式")
        .item(
            &CheckMenuItemBuilder::with_id("tray_output_type", "打字模拟")
                .checked(settings.output_mode == "type")
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("tray_output_paste", "粘贴输入")
                .checked(settings.output_mode == "paste")
                .build(app)?,
        )
        .item(
            &CheckMenuItemBuilder::with_id("tray_output_none", "仅识别")
                .checked(settings.output_mode == "none")
                .build(app)?,
        )
        .build()?;

    let sep3 = PredefinedMenuItem::separator(app)?;

    // ── 底部区 ──
    let stats_item = MenuItemBuilder::with_id("tray_stats", "统计").build(app)?;
    let about_item = MenuItemBuilder::with_id("tray_about", "关于 SpeakIn声入").build(app)?;
    let quit_item = MenuItemBuilder::with_id("tray_quit", "退出").build(app)?;

    MenuBuilder::new(app)
        .items(&[
            &show_item,
            &sep1,
            &ai_item,
            &prompt_submenu,
            &aiprov_submenu,
            &sep2,
            &asr_submenu,
            &audio_submenu,
            &output_submenu,
            &sep3,
            &stats_item,
            &about_item,
            &quit_item,
        ])
        .build()
}

/// Rebuild the tray menu from current AppState. Safe to call from any
/// thread — reads state, builds a fresh menu, and swaps it in.
fn rebuild_tray_menu(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();
    let settings = state.inner.lock().settings.clone();
    let prompts = state.cached_prompts.lock().prompts.clone();
    let providers = state.cached_providers.lock().providers.clone();

    match build_tray_menu(app, &settings, &prompts, &providers) {
        Ok(menu) => {
            if let Some(tray) = app.tray_by_id("main") {
                let _ = tray.set_menu(Some(menu));
            }
        }
        Err(e) => log::error!("Failed to rebuild tray menu: {}", e),
    }
}

/// Modify a setting field, persist, notify frontend, and refresh tray.
///
/// NOTE: This bypasses `normalize_settings_for_save` and the side-effect
/// handling in the `save_settings` Tauri command (hotkey re-registration,
/// mic manager toggle, etc.) because the tray menu only mutates fields
/// that don't require those steps (provider, audio_source, output_mode,
/// ai_optimize.*).  If the tray ever gains controls for hotkey or
/// mic_always_on, route those through the full save_settings path instead.
fn tray_update_setting(app: &tauri::AppHandle, f: impl FnOnce(&mut AppSettings)) {
    let state = app.state::<AppState>();
    let new_settings = {
        let mut inner = state.inner.lock();
        let mut s = (*inner.settings).clone();
        f(&mut s);
        inner.settings = Arc::new(s.clone());
        s
    };
    let _ = storage::save_settings(app, &new_settings);
    let _ = app.emit("settings-changed", ());
    rebuild_tray_menu(app);
}

/// Show the main window and bring it to focus.
fn show_main_window(app: &tauri::AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// Tauri command: rebuild tray menu after frontend settings/prompts/providers change.
#[tauri::command]
fn rebuild_tray_menu_cmd(app: tauri::AppHandle) {
    rebuild_tray_menu(&app);
}

// ── App Setup ──

/// Uninstall-time entry point. Invoked by `main.rs` when the binary is
/// started with the hidden `--uninstall-cleanup` flag from the NSIS
/// PreUninstall hook after the user checks "delete app data". Does NOT start
/// Tauri — only removes OS keyring credentials this app wrote. See
/// `storage::uninstall_cleanup` for the full safety contract.
#[cfg(windows)]
pub fn run_uninstall_cleanup() {
    storage::uninstall_cleanup();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // 第二个实例启动时，聚焦已有窗口
            log::info!("检测到第二个实例，聚焦已有窗口");
            show_main_window(app);
        }))
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
            measure_microphone_level,
            send_text_input,
            update_transcript_optimized,
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
            rebuild_tray_menu_cmd,
            set_window_no_activate,
        ])
        .setup(|app| {
            let app_handle = app.handle().clone();

            // Load and sanitize settings (requires AppHandle for store plugin)
            let providers_file = ai::providers::load_providers(&app_handle);
            let prompts_file = ai::prompts::load_prompts(&app_handle);
            let (settings, mut startup_warning, should_persist) = sanitize_loaded_settings(
                storage::load_settings(&app_handle),
                &providers_file.providers,
            );
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
                    recording_generation: 0,
                    had_speech: Arc::new(AtomicBool::new(false)),
                    aborted: Arc::new(AtomicBool::new(false)),
                    settings: Arc::new(settings.clone()),
                    mic_manager,
                    loopback: None,
                    stop_tx: None,
                    pending_settings_warning: startup_warning,
                })),
                stats: Mutex::new(usage_stats),
                cached_replacements: Mutex::new(replacements::load_replacements(&app_handle)),
                cached_providers: Mutex::new(providers_file),
                cached_prompts: Mutex::new(prompts_file),
            };

            let state_inner = state.inner.clone();
            app.manage(state);

            // ── System Tray ──
            {
                use tauri::tray::{
                    MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent,
                };

                let app_state = app_handle.state::<AppState>();
                let prompts = app_state.cached_prompts.lock().prompts.clone();
                let providers = app_state.cached_providers.lock().providers.clone();
                let menu = build_tray_menu(
                    &app_handle,
                    &settings,
                    &prompts,
                    &providers,
                )?;

                let tray_icon = tauri::image::Image::from_bytes(include_bytes!(
                    "../icons/tray.png"
                ))?;

                TrayIconBuilder::with_id("main")
                    .icon(tray_icon)
                    .tooltip("SpeakIn声入")
                    .menu(&menu)
                    .show_menu_on_left_click(false)
                    .on_menu_event(|app, event| {
                        let id = event.id().as_ref().to_string();
                        match id.as_str() {
                            "tray_show" => show_main_window(app),

                            "tray_ai" => {
                                let can_toggle = {
                                    let state = app.state::<AppState>();
                                    let settings = state.inner.lock().settings.clone();
                                    let providers = state.cached_providers.lock().providers.clone();
                                    settings.ai_optimize.enabled
                                        || has_selected_ai_provider(&settings, &providers)
                                };
                                if can_toggle {
                                    tray_update_setting(app, |s| {
                                        s.ai_optimize.enabled = !s.ai_optimize.enabled;
                                    });
                                } else {
                                    show_main_window(app);
                                    let _ = app.emit(
                                        "settings-warning",
                                        "启用 AI 优化前，请先选择 AI 供应商",
                                    );
                                }
                            }

                            "tray_about" => {
                                show_main_window(app);
                                let _ = app.emit("show-about", ());
                            }

                            "tray_stats" => {
                                show_main_window(app);
                                let _ = app.emit("show-stats", ());
                            }

                            "tray_quit" => app.exit(0),

                            // ASR 供应商
                            "tray_asr_doubao" => {
                                tray_update_setting(app, |s| {
                                    s.provider = "doubao".into();
                                });
                            }
                            "tray_asr_dashscope" => {
                                tray_update_setting(app, |s| {
                                    s.provider = "dashscope".into();
                                });
                            }
                            "tray_asr_qwen" => {
                                tray_update_setting(app, |s| {
                                    s.provider = "qwen".into();
                                });
                            }

                            // 音频来源
                            "tray_audio_microphone" => {
                                tray_update_setting(app, |s| {
                                    s.audio_source = "microphone".into();
                                });
                            }
                            "tray_audio_system" => {
                                tray_update_setting(app, |s| {
                                    s.audio_source = "system".into();
                                });
                            }

                            // 输出方式
                            "tray_output_type" => {
                                tray_update_setting(app, |s| {
                                    s.output_mode = "type".into();
                                });
                            }
                            "tray_output_paste" => {
                                tray_update_setting(app, |s| {
                                    s.output_mode = "paste".into();
                                });
                            }
                            "tray_output_none" => {
                                tray_update_setting(app, |s| {
                                    s.output_mode = "none".into();
                                });
                            }

                            _ => {
                                // 动态 ID：提示词 / AI 供应商
                                if let Some(prompt_id) = id.strip_prefix("tray_prompt_") {
                                    let pid = prompt_id.to_string();
                                    tray_update_setting(app, |s| {
                                        s.ai_optimize.active_prompt_id = pid;
                                    });
                                } else if let Some(prov_id) = id.strip_prefix("tray_aiprov_") {
                                    let pid = prov_id.to_string();
                                    tray_update_setting(app, |s| {
                                        s.ai_optimize.active_provider_id = pid;
                                    });
                                }
                            }
                        }
                    })
                    .on_tray_icon_event(|tray, event| {
                        if let TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        } = event
                        {
                            show_main_window(tray.app_handle());
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

                // Disable Windows 11's automatic window corner rounding so
                // the CSS-side `border-radius` on #root is the sole source
                // of visible curvature.
                //
                // Without this, DWM clips the window to its default ~8px
                // radius while our CSS clips content to 14px. At each
                // corner the 8–14px annulus ends up "inside the OS window
                // but outside our painted content" and, because the
                // window is also `transparent: true`, that annulus shows
                // the desktop through — producing a visible seam between
                // the OS's rounding and ours. Switching DWMWA_WINDOW_CORNER_
                // PREFERENCE to DWMWCP_DONOTROUND makes the OS window a
                // sharp rectangle so our CSS shape takes over cleanly.
                #[cfg(windows)]
                if let Ok(hwnd) = window.hwnd() {
                    use windows_sys::Win32::Graphics::Dwm::{
                        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE,
                        DWMWCP_DONOTROUND,
                    };
                    let raw: windows_sys::Win32::Foundation::HWND = hwnd.0 as _;
                    let pref = DWMWCP_DONOTROUND;
                    unsafe {
                        DwmSetWindowAttribute(
                            raw,
                            DWMWA_WINDOW_CORNER_PREFERENCE as u32,
                            &pref as *const _ as *const _,
                            std::mem::size_of_val(&pref) as u32,
                        );
                    }
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
                            let (is_recording, settings) = {
                                let inner = state.lock();
                                (inner.is_recording, Arc::clone(&inner.settings))
                            };
                            if is_recording {
                                emit_app_log(&app_handle, "info", "全局热键：请求结束录音");
                                do_stop_recording_impl(&app_handle, &state);
                            } else if let Err(e) =
                                {
                                    emit_app_log(&app_handle, "info", "全局热键：请求开始录音");
                                    do_start_recording_impl(&app_handle, &state, &settings)
                                }
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

                            let (is_recording, settings) = {
                                let inner = state.lock();
                                (inner.is_recording, Arc::clone(&inner.settings))
                            };
                            if is_recording {
                                // Already recording — ignore (hold mode shouldn't re-enter)
                            } else if let Err(e) =
                                {
                                    emit_app_log(&app_handle, "info", "全局热键按下：开始录音");
                                    do_start_recording_impl(&app_handle, &state, &settings)
                                }
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
                                emit_app_log(&app_handle, "info", "全局热键释放：判定为误触，取消本次录音");
                                // Emit cancel BEFORE stop so frontend sees it before recording-status(false)
                                let _ = app_handle.emit("recording-cancelled", "mistouch");
                                do_stop_recording_impl(&app_handle, &state);
                            } else {
                                emit_app_log(&app_handle, "info", "全局热键释放：请求结束录音");
                                do_stop_recording_impl(&app_handle, &state);
                            }
                        }
                        HotkeyEvent::AbortSession => {
                            hold_start_instant = None;
                            emit_app_log(&app_handle, "info", "全局 ESC：请求中止当前会话");
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
