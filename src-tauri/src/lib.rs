mod asr;
mod audio;
mod hotkey;
mod input;
mod protocol;

use asr::AsrSettings;
use audio::MicrophoneManager;
use hotkey::{HotkeyEvent, InputMode};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;

/// Application settings, persisted to .env file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub app_id: String,
    pub access_token: String,
    pub resource_id: String,
    pub hotkey: String,
    pub input_mode: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
    pub output_mode: String,
    #[serde(default = "default_true")]
    pub mic_always_on: bool,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            app_id: String::new(),
            access_token: String::new(),
            resource_id: "volc.bigasr.sauc.duration".to_string(),
            hotkey: "Ctrl+Shift+V".to_string(),
            input_mode: "toggle".to_string(),
            device_name: String::new(),
            output_mode: "none".to_string(),
            mic_always_on: true,
        }
    }
}

/// Shared application state.
struct AppStateInner {
    is_recording: bool,
    settings: Arc<AppSettings>,
    mic_manager: Option<MicrophoneManager>,
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

struct AppState {
    inner: Arc<Mutex<AppStateInner>>,
}

/// Get the .env file path.
fn env_file_path() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    let env_path = cwd.join(".env");
    if env_path.exists() {
        return env_path;
    }
    if let Some(parent) = cwd.parent() {
        let env_path = parent.join(".env");
        if env_path.exists() {
            return env_path;
        }
    }
    cwd.join(".env")
}

/// Load settings from .env file using dotenvy for robust parsing.
fn load_settings() -> AppSettings {
    let env_path = env_file_path();
    let mut settings = AppSettings::default();

    let iter = match dotenvy::from_path_iter(&env_path) {
        Ok(iter) => iter,
        Err(_) => return settings,
    };

    for item in iter {
        let Ok((key, value)) = item else { continue };
        match key.as_str() {
            "DOUBAO_APP_ID" => settings.app_id = value,
            "DOUBAO_ACCESS_TOKEN" => settings.access_token = value,
            "DOUBAO_RESOURCE_ID" if !value.is_empty() => settings.resource_id = value,
            "HOTKEY" if !value.is_empty() => settings.hotkey = value,
            "INPUT_MODE" if !value.is_empty() => settings.input_mode = value,
            "DEVICE_NAME" => settings.device_name = value,
            "OUTPUT_MODE" if !value.is_empty() => settings.output_mode = value,
            "MIC_ALWAYS_ON" => settings.mic_always_on = value != "false",
            _ => {}
        }
    }

    settings
}

/// Save settings to .env file.
fn save_settings_to_file(settings: &AppSettings) -> Result<(), String> {
    let env_path = env_file_path();
    let content = format!(
        "DOUBAO_APP_ID={}\nDOUBAO_ACCESS_TOKEN={}\nDOUBAO_RESOURCE_ID={}\nHOTKEY={}\nINPUT_MODE={}\nDEVICE_NAME={}\nOUTPUT_MODE={}\nMIC_ALWAYS_ON={}\n",
        settings.app_id, settings.access_token, settings.resource_id,
        settings.hotkey, settings.input_mode, settings.device_name, settings.output_mode,
        settings.mic_always_on,
    );
    std::fs::write(&env_path, content).map_err(|e| format!("保存设置失败: {}", e))
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
fn save_settings(state: tauri::State<'_, AppState>, settings: AppSettings) -> Result<(), String> {
    save_settings_to_file(&settings)?;
    let input_mode = match settings.input_mode.as_str() {
        "hold" => InputMode::Hold,
        _ => InputMode::Toggle,
    };
    hotkey::update_config(&settings.hotkey, input_mode);

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

    inner.settings = Arc::new(settings);
    Ok(())
}

/// List available audio input devices.
#[tauri::command]
fn list_audio_devices() -> Vec<String> {
    audio::list_input_devices()
}

/// Send text to the focused input field using the configured output mode.
#[tauri::command]
fn send_text_input(state: tauri::State<'_, AppState>, text: String) {
    let mode = {
        let inner = state.inner.lock();
        input::OutputMode::from_str(&inner.settings.output_mode)
    };
    if mode != input::OutputMode::None && !text.is_empty() {
        // Run on a separate thread to avoid blocking
        std::thread::spawn(move || {
            input::send_text(&text, mode);
        });
    }
}

#[tauri::command]
async fn start_recording(
    app_handle: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let settings = {
        let inner = state.inner.lock();
        if inner.settings.app_id.is_empty() || inner.settings.access_token.is_empty() {
            return Err("请先配置 App ID 和 Access Token".to_string());
        }
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

/// Shared recording start logic (used by both Tauri command and hotkey).
fn do_start_recording_impl(
    app_handle: &tauri::AppHandle,
    state: &Arc<Mutex<AppStateInner>>,
    settings: &Arc<AppSettings>,
) -> Result<(), String> {
    // 7B: Check state and set is_recording first, then init mic outside lock
    // to avoid blocking the entire AppState for up to 3 seconds.
    let needs_mic = {
        let mut inner = state.lock();
        if inner.is_recording {
            return Ok(());
        }
        inner.is_recording = true;
        inner.mic_manager.is_none()
    };

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

    let (stop_tx, stop_rx) = tokio::sync::oneshot::channel();
    let (audio_tx, audio_rx) = tokio::sync::mpsc::unbounded_channel();

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
                // Called from audio thread when prolonged silence detected after speech
                emit_log(&app_for_silence, "info", "检测到长时间静音，自动停止录音");
                do_stop_recording_impl(&app_for_silence, &state_for_silence);
            }),
        );
    }

    let _ = app_handle.emit("recording-status", true);

    let state_clone = state.clone();
    let app_handle_clone = app_handle.clone();
    let asr_settings = AsrSettings {
        app_id: settings.app_id.clone(),
        access_token: settings.access_token.clone(),
        resource_id: settings.resource_id.clone(),
    };

    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            asr::run_session(app_handle_clone.clone(), asr_settings, audio_rx, stop_rx).await
        {
            log::error!("ASR session error: {}", e);
            let _ = app_handle_clone.emit("asr-error", e);
        }

        let mut inner = state_clone.lock();
        if inner.is_recording {
            inner.is_recording = false;
            if let Some(ref mic) = inner.mic_manager {
                mic.stop_forwarding();
            }
            // Release mic if not always-on
            if !inner.settings.mic_always_on {
                inner.mic_manager = None;
            }
            let _ = app_handle_clone.emit("recording-status", false);
        }
    });

    Ok(())
}

/// Shared recording stop logic.
fn do_stop_recording_impl(app_handle: &tauri::AppHandle, state: &Arc<Mutex<AppStateInner>>) {
    let mut inner = state.lock();
    if !inner.is_recording {
        return;
    }

    // Signal ASR loop to stop FIRST — this lets the loop exit via the stop_rx
    // path and send the final packet before the audio channel closes.
    if let Some(stop_tx) = inner.stop_tx.take() {
        let _ = stop_tx.send(());
    }
    // Then stop forwarding audio (closes the sender, stream stays open)
    if let Some(ref mic) = inner.mic_manager {
        mic.stop_forwarding();
    }
    inner.is_recording = false;
    drop(inner);

    emit_log(app_handle, "info", "录音已停止");
    let _ = app_handle.emit("recording-status", false);
}

// ── App Setup ──

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let settings = load_settings();
    let hotkey_name = settings.hotkey.clone();
    let input_mode = match settings.input_mode.as_str() {
        "hold" => InputMode::Hold,
        _ => InputMode::Toggle,
    };

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

    let state = AppState {
        inner: Arc::new(Mutex::new(AppStateInner {
            is_recording: false,
            settings: Arc::new(settings),
            mic_manager,
            stop_tx: None,
        })),
    };

    let state_inner = state.inner.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            start_recording,
            stop_recording,
            list_audio_devices,
            send_text_input,
        ])
        .setup(move |app| {
            let app_handle = app.handle().clone();
            let state_for_hotkey = state_inner.clone();

            let hotkey_rx = hotkey::start_listener(&hotkey_name, input_mode);
            std::thread::spawn(move || {
                while let Ok(event) = hotkey_rx.recv() {
                    let app_handle = app_handle.clone();
                    let state = state_for_hotkey.clone();

                    match event {
                        HotkeyEvent::ShortPress => {
                            let (is_recording, settings) = {
                                let inner = state.lock();
                                (inner.is_recording, Arc::clone(&inner.settings))
                            };
                            if is_recording {
                                do_stop_recording_impl(&app_handle, &state);
                            } else if let Err(e) =
                                do_start_recording_impl(&app_handle, &state, &settings)
                            {
                                let _ = app_handle.emit("asr-error", format!("启动失败: {}", e));
                            }
                        }
                        HotkeyEvent::HoldStart => {
                            let (is_recording, settings) = {
                                let inner = state.lock();
                                (inner.is_recording, Arc::clone(&inner.settings))
                            };
                            if !is_recording {
                                if let Err(e) =
                                    do_start_recording_impl(&app_handle, &state, &settings)
                                {
                                    let _ =
                                        app_handle.emit("asr-error", format!("启动失败: {}", e));
                                }
                            }
                        }
                        HotkeyEvent::HoldEnd => {
                            do_stop_recording_impl(&app_handle, &state);
                        }
                    }
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
