//! ASR provider abstraction layer.
//!
//! Defines the `AsrProvider` trait and shared types/utilities.
//! Provider implementations live in submodules (e.g. `doubao`).

pub mod dashscope;
pub mod doubao;
pub mod qwen;
mod protocol;

use crate::audio::AudioFrame;
use std::borrow::Cow;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};

/// Transcription result emitted to the frontend. Shared by all providers.
///
/// `generation` carries the current `recording_generation` from the backend
/// so the frontend can filter out late-arriving events from previous sessions.
/// See the "self-healing filter" in App.tsx's transcription-update listener.
#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptUpdate {
    pub text: String,
    pub is_final: bool,
    pub generation: u64,
}

/// ASR provider trait — each cloud ASR service implements this.
pub trait AsrProvider: Send + Sync {
    /// Run a single ASR session: consume audio frames, emit transcription events.
    ///
    /// `generation` is the recording_generation counter at session start —
    /// every emitted `TranscriptUpdate` must carry this value so the frontend
    /// can discard late events from a stale session.
    fn run_session(
        &self,
        app_handle: AppHandle,
        generation: u64,
        audio_rx: mpsc::UnboundedReceiver<AudioFrame>,
        stop_rx: oneshot::Receiver<()>,
    ) -> impl std::future::Future<Output = Result<(), String>> + Send;
}

// ── Shared utilities for all providers ──

/// Emit a network log event to the frontend and write to Rust log.
pub(crate) fn net_log(app: &AppHandle, level: &str, msg: &str) {
    crate::emit_log(app, level, msg);
    match level {
        "error" => log::error!("{}", msg),
        "warn" => log::warn!("{}", msg),
        _ => log::info!("{}", msg),
    }
}

/// Truncate a string for log display, preserving UTF-8 boundaries.
pub(crate) fn truncate_for_log(s: &str, max_len: usize) -> Cow<'_, str> {
    if s.len() > max_len {
        let end = s.floor_char_boundary(max_len.saturating_sub(3));
        Cow::Owned(format!("{}...", &s[..end]))
    } else {
        Cow::Borrowed(s)
    }
}

/// Check if the current session should skip the no-speech timeout.
/// Returns true when audio source is "system" and user enabled "no auto stop".
pub(crate) fn should_skip_timeout(app: &AppHandle) -> bool {
    use tauri::Manager;
    if let Some(state) = app.try_state::<crate::AppState>() {
        let inner = state.inner.lock();
        inner.settings.audio_source == "system" && inner.settings.system_no_auto_stop
    } else {
        false
    }
}

// ── Pre-speech buffering shared by all providers ──

/// Max pre-speech buffer size: 1 second @ 16kHz 16-bit mono = 32000 bytes.
pub(crate) const PRE_SPEECH_BUFFER_MAX: usize = 32000;

/// Result of waiting for speech detection.
pub(crate) enum WaitForSpeechResult {
    /// Speech detected — returns the pre-buffered PCM data.
    Speech(Vec<u8>),
    /// User stopped before speech was detected.
    Stopped,
    /// Audio channel was closed (mic stopped).
    ChannelClosed,
}

/// Max wait time for first speech before auto-cancelling (seconds).
const NO_SPEECH_TIMEOUT_SECS: u64 = 30;

/// Wait for the first speech frame, buffering audio for lookback.
/// Emits `audio-level` events to the frontend during the wait.
/// Times out after 30 seconds of no speech to avoid leaving the session stuck.
/// If `no_timeout` is true, waits indefinitely (for system audio long recordings).
pub(crate) async fn wait_for_speech(
    app: &AppHandle,
    audio_rx: &mut mpsc::UnboundedReceiver<AudioFrame>,
    stop_rx: &mut oneshot::Receiver<()>,
    no_timeout: bool,
) -> WaitForSpeechResult {
    let mut pre_buffer: Vec<u8> = Vec::with_capacity(PRE_SPEECH_BUFFER_MAX * 2);
    // 24 hours effectively means "no timeout" while avoiding Instant overflow
    let timeout_secs = if no_timeout { 86400 } else { NO_SPEECH_TIMEOUT_SECS };
    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(timeout_secs);

    loop {
        tokio::select! {
            biased;
            _ = &mut *stop_rx => {
                return WaitForSpeechResult::Stopped;
            }
            _ = tokio::time::sleep_until(deadline) => {
                let msg = if no_timeout {
                    "长时间未检测到语音，自动取消"
                } else {
                    "30 秒未检测到语音，自动取消"
                };
                net_log(app, "info", msg);
                return WaitForSpeechResult::Stopped;
            }
            frame = audio_rx.recv() => {
                match frame {
                    Some(frame) => {
                        let _ = app.emit("audio-level", frame.level);
                        pre_buffer.extend_from_slice(&frame.pcm);

                        if frame.has_speech {
                            return WaitForSpeechResult::Speech(pre_buffer);
                        }

                        // Prevent unbounded growth
                        if pre_buffer.len() > PRE_SPEECH_BUFFER_MAX * 2 {
                            let drain_to = pre_buffer.len() - PRE_SPEECH_BUFFER_MAX;
                            pre_buffer.drain(..drain_to);
                        }
                    }
                    None => {
                        return WaitForSpeechResult::ChannelClosed;
                    }
                }
            }
        }
    }
}
