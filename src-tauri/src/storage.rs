use crate::{AppSettings, TranscriptRecord, UsageStats};
use keyring::error::Error as KeyringError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_store::StoreExt;

const STORE_FILENAME: &str = "settings.json";
const STATS_FILENAME: &str = "stats.json";
pub(crate) const KEYRING_SERVICE: &str = "com.magiccodelab.speakin";

// ── Generic store helpers ──

/// Load a struct from a tauri-plugin-store file. Returns `Default` on any error.
pub(crate) fn load_store_data<T: DeserializeOwned + Default>(
    app: &AppHandle,
    filename: &str,
    key: &str,
) -> T {
    let Ok(store) = app.store(filename) else {
        return T::default();
    };
    store
        .get(key)
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default()
}

/// Save a struct to a tauri-plugin-store file.
pub(crate) fn save_store_data<T: Serialize>(
    app: &AppHandle,
    filename: &str,
    key: &str,
    data: &T,
) -> Result<(), String> {
    let store = app
        .store(filename)
        .map_err(|e| format!("打开存储失败: {}", e))?;
    let value = serde_json::to_value(data).map_err(|e| format!("序列化失败: {}", e))?;
    store.set(key, value);
    store
        .save()
        .map_err(|e| format!("落盘失败: {}", e))?;
    Ok(())
}

// ── Settings ──

/// Strip credential fields. Used both before persisting to JSON and after
/// loading from JSON, so the on-disk file never contains plaintext secrets
/// regardless of how it got there.
fn strip_credentials(settings: &mut AppSettings) {
    settings.doubao.app_id.clear();
    settings.doubao.access_token.clear();
    settings.dashscope.api_key.clear();
    settings.qwen.api_key.clear();
}

/// Load all settings: general from tauri-plugin-store, credentials from OS keyring.
pub fn load_settings(app: &AppHandle) -> AppSettings {
    // Try new format first (whole-struct under "data" key)
    let has_new_format = app
        .store(STORE_FILENAME)
        .ok()
        .and_then(|s| s.get("data"))
        .is_some();

    let mut settings: AppSettings = if has_new_format {
        load_store_data(app, STORE_FILENAME, "data")
    } else {
        // Migration: try legacy per-field format
        let legacy = load_settings_legacy(app);
        // Persist in new format (ignore errors during migration)
        let _ = save_store_data(app, STORE_FILENAME, "data", &legacy);
        legacy
    };

    // Detect plaintext credentials that may have leaked into the JSON file
    // by an older buggy build. We must check BEFORE stripping in-memory.
    let leaked_plaintext = !settings.doubao.app_id.is_empty()
        || !settings.doubao.access_token.is_empty()
        || !settings.dashscope.api_key.is_empty()
        || !settings.qwen.api_key.is_empty();

    // The OS keyring is the sole source of truth — drop anything from JSON,
    // then overlay from keyring.
    strip_credentials(&mut settings);
    overlay_credentials(&mut settings);

    // If we found leaked plaintext on disk, immediately rewrite the file to
    // remove it. Without this, leaked credentials could sit in the JSON
    // indefinitely, since regular `save_settings` only runs on user actions.
    // We use the in-memory settings AFTER overlay (which has the authoritative
    // keyring values), then strip again before persisting — same pipeline as
    // the normal save path.
    if leaked_plaintext {
        log::warn!(
            "Detected legacy plaintext credentials in {}; cleaning up. \
            The OS keyring is the only persistent store now.",
            STORE_FILENAME
        );
        let mut clean = settings.clone();
        strip_credentials(&mut clean);
        if let Err(e) = save_store_data(app, STORE_FILENAME, "data", &clean) {
            log::warn!("Cleanup of leaked plaintext credentials failed: {}", e);
        }
    }

    settings
}

/// Save all settings: credentials to keyring, rest to store.
pub fn save_settings(app: &AppHandle, settings: &AppSettings) -> Result<(), String> {
    // Save credentials first — if this fails, don't update the store
    save_credential("app_id", &settings.doubao.app_id)?;
    save_credential("access_token", &settings.doubao.access_token)?;
    save_credential("dashscope_api_key", &settings.dashscope.api_key)?;
    save_credential("qwen_api_key", &settings.qwen.api_key)?;

    // Persist a credential-stripped clone to JSON. The credential fields
    // are now plain `#[serde(default)]` (not `skip`), so we can no longer
    // rely on serde to omit them — we strip explicitly here.
    let mut json_copy = settings.clone();
    strip_credentials(&mut json_copy);
    save_store_data(app, STORE_FILENAME, "data", &json_copy)
}

/// Overlay credentials from OS keyring onto settings.
fn overlay_credentials(settings: &mut AppSettings) {
    match load_credential("app_id") {
        Ok(v) => settings.doubao.app_id = v,
        Err(KeyringError::NoEntry) => {}
        Err(e) => log::warn!("读取 app_id 凭据失败（保留默认值）: {}", e),
    }
    match load_credential("access_token") {
        Ok(v) => settings.doubao.access_token = v,
        Err(KeyringError::NoEntry) => {}
        Err(e) => log::warn!("读取 access_token 凭据失败（保留默认值）: {}", e),
    }
    match load_credential("dashscope_api_key") {
        Ok(v) => settings.dashscope.api_key = v,
        Err(KeyringError::NoEntry) => {}
        Err(e) => log::warn!("读取 dashscope_api_key 凭据失败（保留默认值）: {}", e),
    }
    match load_credential("qwen_api_key") {
        Ok(v) => settings.qwen.api_key = v,
        Err(KeyringError::NoEntry) => {}
        Err(e) => log::warn!("读取 qwen_api_key 凭据失败（保留默认值）: {}", e),
    }
}

/// Legacy loader: reads the old per-field flat-key format from settings.json.
/// Used for one-time migration to the new whole-struct format.
fn load_settings_legacy(app: &AppHandle) -> AppSettings {
    let mut settings = AppSettings::default();

    let Ok(store) = app.store(STORE_FILENAME) else {
        return settings;
    };

    if let Some(v) = store.get("provider").and_then(|v| v.as_str().map(String::from)) {
        if !v.is_empty() {
            settings.provider = v;
        }
    }
    if let Some(v) = store
        .get("resource_id")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.doubao.resource_id = v;
        }
    }
    if let Some(v) = store
        .get("asr_mode")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.doubao.asr_mode = v;
        }
    }
    if let Some(v) = store
        .get("hotkey")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.hotkey = v;
        }
    }
    if let Some(v) = store
        .get("input_mode")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.input_mode = v;
        }
    }
    if let Some(v) = store
        .get("device_name")
        .and_then(|v| v.as_str().map(String::from))
    {
        settings.device_name = v;
    }
    if let Some(v) = store
        .get("audio_source")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.audio_source = v;
        }
    }
    if let Some(v) = store
        .get("output_mode")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.output_mode = v;
        }
    }
    if let Some(v) = store.get("mic_always_on").and_then(|v| v.as_bool()) {
        settings.mic_always_on = v;
    }
    if let Some(v) = store.get("debug_mode").and_then(|v| v.as_bool()) {
        settings.debug_mode = v;
    }
    if let Some(v) = store.get("filler_enabled").and_then(|v| v.as_bool()) {
        settings.filler_enabled = v;
    }
    if let Some(v) = store
        .get("replacement_enabled")
        .and_then(|v| v.as_bool())
    {
        settings.replacement_enabled = v;
    }
    if let Some(v) = store
        .get("replacement_ignore_case")
        .and_then(|v| v.as_bool())
    {
        settings.replacement_ignore_case = v;
    }
    if let Some(v) = store
        .get("theme_color")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.theme_color = v;
        }
    }
    if let Some(v) = store
        .get("recording_follows_theme")
        .and_then(|v| v.as_bool())
    {
        settings.recording_follows_theme = v;
    }
    if let Some(v) = store.get("show_overlay").and_then(|v| v.as_bool()) {
        settings.show_overlay = v;
    }
    if let Some(v) = store
        .get("close_behavior")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.close_behavior = v;
        }
    }
    if let Some(v) = store
        .get("onboarding_completed")
        .and_then(|v| v.as_bool())
    {
        settings.onboarding_completed = v;
    }
    if let Some(v) = store
        .get("copy_to_clipboard")
        .and_then(|v| v.as_bool())
    {
        settings.copy_to_clipboard = v;
    }
    if let Some(v) = store
        .get("paste_restore_clipboard")
        .and_then(|v| v.as_bool())
    {
        settings.paste_restore_clipboard = v;
    }
    if let Some(v) = store
        .get("system_no_auto_stop")
        .and_then(|v| v.as_bool())
    {
        settings.system_no_auto_stop = v;
    }
    if let Some(v) = store
        .get("esc_abort_enabled")
        .and_then(|v| v.as_bool())
    {
        settings.esc_abort_enabled = v;
    }
    if let Some(v) = store
        .get("silence_auto_stop_secs")
        .and_then(|v| v.as_u64())
    {
        settings.silence_auto_stop_secs = v.clamp(3, 60) as u8;
    }
    if let Some(v) = store
        .get("vad_sensitivity")
        .and_then(|v| v.as_u64())
    {
        settings.vad_sensitivity = v.clamp(1, 10) as u8;
    }
    // DashScope
    if let Some(v) = store
        .get("dashscope_model")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.dashscope.model = v;
        }
    }
    if let Some(v) = store
        .get("dashscope_region")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.dashscope.region = v;
        }
    }
    // Qwen
    if let Some(v) = store
        .get("qwen_model")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.qwen.model = v;
        }
    }
    if let Some(v) = store
        .get("qwen_region")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.qwen.region = v;
        }
    }
    if let Some(v) = store
        .get("qwen_language")
        .and_then(|v| v.as_str().map(String::from))
    {
        if !v.is_empty() {
            settings.qwen.language = v;
        }
    }
    // AI optimize
    if let Some(v) = store
        .get("ai_optimize_enabled")
        .and_then(|v| v.as_bool())
    {
        settings.ai_optimize.enabled = v;
    }
    if let Some(v) = store
        .get("ai_optimize_active_provider_id")
        .and_then(|v| v.as_str().map(String::from))
    {
        settings.ai_optimize.active_provider_id = v;
    }
    if let Some(v) = store
        .get("ai_optimize_active_prompt_id")
        .and_then(|v| v.as_str().map(String::from))
    {
        settings.ai_optimize.active_prompt_id = v;
    }

    settings
}

// ── Usage Statistics ──

pub fn load_usage_stats(app: &AppHandle) -> UsageStats {
    // Try new format first
    let has_new_format = app
        .store(STATS_FILENAME)
        .ok()
        .and_then(|s| s.get("data"))
        .is_some();

    if has_new_format {
        return load_store_data(app, STATS_FILENAME, "data");
    }

    // Migration: try legacy flat keys in settings.json
    let stats = load_usage_stats_legacy(app);
    if stats.total_sessions > 0 || stats.total_characters > 0 {
        let _ = save_store_data(app, STATS_FILENAME, "data", &stats);
    }
    stats
}

pub fn save_usage_stats(app: &AppHandle, stats: &UsageStats) -> Result<(), String> {
    save_store_data(app, STATS_FILENAME, "data", stats)
}

/// Legacy loader for stats from old settings.json flat keys.
fn load_usage_stats_legacy(app: &AppHandle) -> UsageStats {
    let mut stats = UsageStats::default();
    let Ok(store) = app.store(STORE_FILENAME) else {
        return stats;
    };
    if let Some(v) = store
        .get("stats_total_sessions")
        .and_then(|v| v.as_u64())
    {
        stats.total_sessions = v;
    }
    if let Some(v) = store
        .get("stats_total_recording_duration_ms")
        .and_then(|v| v.as_u64())
    {
        stats.total_recording_duration_ms = v;
    }
    if let Some(v) = store
        .get("stats_total_characters")
        .and_then(|v| v.as_u64())
    {
        stats.total_characters = v;
    }
    if let Some(v) = store
        .get("stats_total_chinese_chars")
        .and_then(|v| v.as_u64())
    {
        stats.total_chinese_chars = v;
    }
    stats
}

// ── Recent Transcript Records ──

const TRANSCRIPTS_FILENAME: &str = "recent_transcripts.json";
const MAX_RECORDS: usize = 30;
const EXPIRE_MS: u64 = 7 * 24 * 60 * 60 * 1000; // 7 days

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Clean up expired and excess records. Returns whether any were removed.
fn cleanup_records(records: &mut Vec<TranscriptRecord>) -> bool {
    let cutoff = now_ms().saturating_sub(EXPIRE_MS);
    let before = records.len();
    records.retain(|r| r.timestamp >= cutoff);
    // Sort newest first, then truncate
    records.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    records.truncate(MAX_RECORDS);
    records.len() != before
}

pub fn load_transcript_records(app: &AppHandle) -> Vec<TranscriptRecord> {
    let Ok(store) = app.store(TRANSCRIPTS_FILENAME) else {
        return Vec::new();
    };
    let mut records: Vec<TranscriptRecord> = store
        .get("records")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    if cleanup_records(&mut records) {
        store.set("records", serde_json::json!(&records));
        let _ = store.save();
    }
    records
}

pub fn append_transcript_record(app: &AppHandle, record: TranscriptRecord) -> Result<(), String> {
    let store = app
        .store(TRANSCRIPTS_FILENAME)
        .map_err(|e| format!("打开转录记录存储失败: {}", e))?;

    let mut records: Vec<TranscriptRecord> = store
        .get("records")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    records.push(record);
    cleanup_records(&mut records);

    store.set("records", serde_json::json!(&records));
    store
        .save()
        .map_err(|e| format!("转录记录落盘失败: {}", e))?;
    Ok(())
}

/// Update an existing record's `optimized` field (and promote status from
/// "partial" → "done" if applicable). Used when AI optimize completes after
/// the raw record was already saved. Returns Err if no record with the given
/// id exists (e.g. it was evicted by cleanup).
pub fn update_transcript_optimized_by_id(
    app: &AppHandle,
    id: &str,
    optimized: String,
) -> Result<(), String> {
    let store = app
        .store(TRANSCRIPTS_FILENAME)
        .map_err(|e| format!("打开转录记录存储失败: {}", e))?;

    let mut records: Vec<TranscriptRecord> = store
        .get("records")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    let Some(record) = records.iter_mut().find(|r| r.id == id) else {
        return Err(format!("record not found: {}", id));
    };
    record.optimized = Some(optimized);
    // Promote partial → done: the partial status was a placeholder waiting
    // for AI optimize to complete. Now that it has, the record is fully done.
    if record.status == "partial" {
        record.status = "done".to_string();
    }

    store.set("records", serde_json::json!(&records));
    store
        .save()
        .map_err(|e| format!("转录记录落盘失败: {}", e))?;
    Ok(())
}

pub fn clear_transcript_records(app: &AppHandle) -> Result<(), String> {
    let store = app
        .store(TRANSCRIPTS_FILENAME)
        .map_err(|e| format!("打开转录记录存储失败: {}", e))?;
    store.set("records", serde_json::json!([]));
    store
        .save()
        .map_err(|e| format!("转录记录清空失败: {}", e))?;
    Ok(())
}

// ── Generic keyring helpers ──

pub(crate) fn load_credential(key: &str) -> Result<String, KeyringError> {
    let entry = keyring::Entry::new(KEYRING_SERVICE, key)?;
    entry.get_password()
}

pub(crate) fn save_credential(key: &str, value: &str) -> Result<(), String> {
    let entry =
        keyring::Entry::new(KEYRING_SERVICE, key).map_err(|e| format!("凭据存储错误: {}", e))?;
    if value.is_empty() {
        match entry.delete_credential() {
            Ok(()) => {}
            Err(KeyringError::NoEntry) => {}
            Err(e) => return Err(format!("删除凭据失败: {}", e)),
        }
    } else {
        entry
            .set_password(value)
            .map_err(|e| format!("保存凭据失败: {}", e))?;
    }
    Ok(())
}

// ── Uninstall cleanup ──

/// Fixed credential keys this app writes to the OS keyring. Mirrors the
/// `save_credential` call sites in `save_settings` — keep in sync.
const FIXED_CREDENTIAL_KEYS: &[&str] = &[
    "app_id",
    "access_token",
    "dashscope_api_key",
    "qwen_api_key",
];

/// Best-effort removal of EVERY credential this app has ever written to the
/// OS keyring. Invoked via the hidden `--uninstall-cleanup` CLI flag from
/// the NSIS uninstaller's PreUninstall hook only after the user checks
/// "delete app data", BEFORE installed files are removed (so the binary is
/// still present to run).
///
/// ## Safety contract
///
/// This function uses the SAME `keyring::Entry::new(service, key)` API that
/// wrote each credential, with the SAME `KEYRING_SERVICE` constant. This
/// makes it mathematically impossible to delete any credential that does
/// not belong to this application — no wildcards, no pattern matching, no
/// enumeration of the Credential Manager.
///
/// Keys deleted:
/// - `FIXED_CREDENTIAL_KEYS` — hardcoded list of ASR provider keys
/// - `ai_provider_<id>` — dynamic per-provider keys, with `<id>` values read
///   directly from our own `ai_providers.json` file in the app data dir
///
/// All errors (including `NoEntry`) are swallowed. Uninstall must never
/// fail because of cleanup; the worst case is a stray credential left in
/// Credential Manager, which is the status quo without this function.
#[cfg(windows)]
pub fn uninstall_cleanup() {
    for key in FIXED_CREDENTIAL_KEYS {
        delete_credential_best_effort(key);
    }
    for id in enumerate_ai_provider_ids_for_cleanup() {
        delete_credential_best_effort(&format!("ai_provider_{}", id));
    }
}

#[cfg(windows)]
fn delete_credential_best_effort(key: &str) {
    match keyring::Entry::new(KEYRING_SERVICE, key) {
        Ok(entry) => match entry.delete_credential() {
            Ok(()) => {}
            Err(KeyringError::NoEntry) => {}
            Err(_) => {}
        },
        Err(_) => {}
    }
}

/// Read our own `ai_providers.json` directly from `%APPDATA%\<identifier>\`
/// and extract the list of provider IDs. Cannot use tauri-plugin-store here
/// because no Tauri runtime is initialized in cleanup mode.
///
/// The folder name is `KEYRING_SERVICE` by design: it is the tauri.conf.json
/// `identifier`, which also drives Tauri's `app_data_dir()` on Windows, so
/// both point at the same reverse-DNS folder.
#[cfg(windows)]
fn enumerate_ai_provider_ids_for_cleanup() -> Vec<String> {
    let Some(appdata) = std::env::var_os("APPDATA") else {
        return Vec::new();
    };
    let path = std::path::PathBuf::from(appdata)
        .join(KEYRING_SERVICE)
        .join("ai_providers.json");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return Vec::new();
    };
    // tauri-plugin-store wraps the payload under the store key used when
    // calling `store.set(key, value)` — here "data" (see providers.rs).
    // Legacy files written via std::fs used `{"providers": [...]}` at the
    // top level; support both for safety.
    let providers_array = json
        .get("data")
        .and_then(|d| d.get("providers"))
        .or_else(|| json.get("providers"));

    let Some(arr) = providers_array.and_then(|v| v.as_array()) else {
        return Vec::new();
    };

    arr.iter()
        .filter_map(|p| p.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect()
}
