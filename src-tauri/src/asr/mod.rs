//! ASR provider abstraction layer.
//!
//! Defines the `AsrProvider` trait and shared types/utilities.
//! Provider implementations live in submodules (e.g. `doubao`).

pub mod dashscope;
pub mod doubao;
pub mod qwen;
mod protocol;

use crate::audio::{AudioFrame, HOTKEY_NOISE_GUARD_MS, SPEECH_START_MIN_HITS};
use std::borrow::Cow;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

/// Session-ended payload — the authoritative "this session is done" event.
///
/// Emitted by `finalize_session` exactly once per ASR session (success,
/// error, abort, or no-speech). Replaces the old multi-event shuffle
/// (`asr-error` + `recording-status(false)` + frontend `mark_session_idle`),
/// which left the backend waiting on the frontend to release its
/// `is_processing` gate and could stall for up to 65 seconds when the
/// frontend hit any exception path.
///
/// With `session-ended`, the backend owns the entire lifecycle:
///   1. accumulates `definite=true` finals during the session
///   2. on session exit, persists accumulated text as a TranscriptRecord
///   3. emits this payload with status + record_id + localized error text
///
/// The frontend just reacts: render status + trigger AI optimize + output.
/// Next recording can start immediately — no backend gate to release.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SessionEndedPayload {
    pub generation: u64,
    /// Join of all `definite=true` finals accumulated during the session.
    /// Empty string when no speech was ever transcribed.
    pub final_text: String,
    /// `"ok" | "no_speech" | "error" | "aborted"`
    pub status: String,
    /// Localized short error message. Only set when `status == "error"`.
    pub error_reason: Option<String>,
    /// Raw error detail (for debug mode display). Only when `status == "error"`.
    pub error_detail: Option<String>,
    /// Recording duration in ms, used by the frontend to update usage stats.
    pub duration_ms: u64,
    /// Id of the persisted TranscriptRecord. `None` when final_text was
    /// empty (nothing to persist) or persist failed.
    pub record_id: Option<String>,
}

/// Terminal state of an ASR session, produced by the provider's run_session
/// and consumed by `finalize_session`.
pub(crate) struct SessionOutcome {
    /// Each `definite=true` sentence collected during the session.
    pub finals: Vec<String>,
    /// Set iff the session ended with an error. `(localized_reason, raw_detail)`.
    pub error: Option<(String, String)>,
    /// Set iff the session was aborted by the user (ESC).
    pub aborted: bool,
    /// VAD ever observed speech.
    pub had_speech: bool,
    /// ms from session start to end.
    pub duration_ms: u64,
}

impl SessionOutcome {
    pub fn new() -> Self {
        Self {
            finals: Vec::new(),
            error: None,
            aborted: false,
            had_speech: false,
            duration_ms: 0,
        }
    }

    /// Derive the `status` string for the session-ended event.
    ///
    /// Priority order: aborted > error > no_speech > ok. A session with
    /// `had_speech=true` but no accumulated finals (e.g. connected to
    /// ASR, spoke briefly, stopped before any definite sentence arrived)
    /// is treated as `no_speech` — there's nothing for the frontend to
    /// render anyway, and the UX should be "silent close" rather than
    /// pretending a successful empty transcription.
    pub fn status(&self) -> &'static str {
        if self.aborted {
            "aborted"
        } else if self.error.is_some() {
            "error"
        } else if self.finals.is_empty() {
            "no_speech"
        } else {
            "ok"
        }
    }
}

/// Finalize a session: persist accumulated text (if any) as a TranscriptRecord
/// and emit the `session-ended` event. Call this exactly once per session,
/// from the provider's `run_session` exit path, regardless of how the session
/// ended.
///
/// **Important**: on error/aborted paths, the raw ASR text is persisted
/// as-is (no filler cleanup, no text replacements). Those post-processing
/// rules only apply when the session completes successfully and the text
/// flows through `send_text_input` on the frontend. The reasoning:
///   - user may need to see the raw ASR output to diagnose an error
///   - replacement rules may depend on complete context that a truncated
///     error-path transcript doesn't have
pub(crate) fn finalize_session(
    app: &AppHandle,
    generation: u64,
    mut outcome: SessionOutcome,
) {
    // [Codex Q7 fix] Promote "spoke but got nothing back" to an error.
    // If VAD observed speech, there's no explicit error, no abort, and
    // yet finals is empty, the session effectively failed silently —
    // typically a final-wait timeout, a WebSocket close with no last
    // frame, or a finish-task write that failed. Without this promotion,
    // `status()` would return `no_speech` and the UI would close
    // silently, masking the real failure from the user.
    if outcome.error.is_none()
        && !outcome.aborted
        && outcome.had_speech
        && outcome.finals.is_empty()
    {
        outcome.error = Some((
            "未收到识别结果".to_string(),
            "had_speech=true but finals empty at session exit".to_string(),
        ));
    }

    let status = outcome.status().to_string();
    let final_text = outcome.finals.join("\n");

    // Persist if we have any text (errors + aborts still save what we got —
    // the user's speech isn't worth throwing away over a network blip).
    let record_id = if !final_text.trim().is_empty() {
        let record_status = match status.as_str() {
            "ok" => "done",
            "aborted" => "aborted",
            _ => "partial",
        }
        .to_string();

        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let id = format!("{}-{}", now_ms, generation);

        let record = crate::TranscriptRecord {
            id: id.clone(),
            timestamp: now_ms,
            original: final_text.clone(),
            final_text: final_text.clone(),
            optimized: None,
            duration_ms: outcome.duration_ms,
            status: record_status,
        };

        match crate::storage::append_transcript_record(app, record) {
            Ok(()) => Some(id),
            Err(e) => {
                log::error!("finalize_session: persist failed: {}", e);
                None
            }
        }
    } else {
        None
    };

    let (error_reason, error_detail) = outcome
        .error
        .map(|(reason, detail)| (Some(reason), Some(detail)))
        .unwrap_or((None, None));

    let payload = SessionEndedPayload {
        generation,
        final_text,
        status,
        error_reason,
        error_detail,
        duration_ms: outcome.duration_ms,
        record_id,
    };

    if let Err(e) = app.emit("session-ended", &payload) {
        log::error!("failed to emit session-ended: {}", e);
    }
}

/// Map a raw provider error string to a localized user-facing reason.
/// Kept here (not in lib.rs) so all providers share the same mapping.
pub(crate) fn classify_error(raw: &str) -> String {
    let lower = raw.to_ascii_lowercase();
    if lower.contains("401") || lower.contains("403") || lower.contains("鉴权") || lower.contains("unauthorized") {
        "语音服务鉴权失败".to_string()
    } else if lower.contains("45000081") || lower.contains("timeout") || lower.contains("超时") {
        "语音服务响应超时".to_string()
    } else if lower.contains("45000151") || lower.contains("音频格式") {
        "音频格式不正确".to_string()
    } else if lower.contains("连接") || lower.contains("websocket") || lower.contains("network") || lower.contains("io error") {
        "语音服务连接失败，检查网络后重试".to_string()
    } else {
        "语音识别出错".to_string()
    }
}

/// ASR provider trait — each cloud ASR service implements this.
pub trait AsrProvider: Send + Sync {
    /// Run a single ASR session: consume audio frames, accumulate finals,
    /// and return a `SessionOutcome` describing how the session ended.
    ///
    /// `generation` is the recording_generation counter at session start —
    /// every emitted `TranscriptUpdate` must carry this value so the frontend
    /// can discard late events from a stale session.
    ///
    /// `had_speech` is a shared atomic flag flipped to `true` by
    /// `wait_for_speech` once speech-start confirmation passes the
    /// hotkey guard and consecutive-hit threshold.
    ///
    /// `aborted` is checked by the provider when the session is stopped —
    /// if set, the outcome's `aborted` field is populated so
    /// `finalize_session` emits `status: "aborted"`.
    ///
    /// **Providers must NOT emit `asr-error` or `session-ended` themselves**.
    /// The orchestrator (spawn_asr_task) calls `finalize_session` with the
    /// returned `SessionOutcome` to guarantee exactly-once session
    /// termination.
    fn run_session(
        &self,
        app_handle: AppHandle,
        generation: u64,
        audio_rx: mpsc::UnboundedReceiver<AudioFrame>,
        stop_rx: oneshot::Receiver<()>,
        had_speech: Arc<AtomicBool>,
        aborted: Arc<AtomicBool>,
    ) -> impl std::future::Future<Output = SessionOutcome> + Send;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpeechStartObservation {
    Guarded,
    Pending,
    Confirmed,
}

fn observe_speech_start(
    consecutive_speech_hits: &mut u32,
    frame_has_speech: bool,
    elapsed: std::time::Duration,
) -> SpeechStartObservation {
    if frame_has_speech {
        if elapsed < std::time::Duration::from_millis(HOTKEY_NOISE_GUARD_MS) {
            *consecutive_speech_hits = 0;
            return SpeechStartObservation::Guarded;
        }
        *consecutive_speech_hits += 1;
    } else {
        *consecutive_speech_hits = 0;
    }

    if *consecutive_speech_hits >= SPEECH_START_MIN_HITS {
        SpeechStartObservation::Confirmed
    } else {
        SpeechStartObservation::Pending
    }
}

/// Wait for the first speech frame, buffering audio for lookback.
/// Emits `audio-level` events to the frontend during the wait.
/// Times out after 30 seconds of no speech to avoid leaving the session stuck.
/// If `no_timeout` is true, waits indefinitely (for system audio long recordings).
pub(crate) async fn wait_for_speech(
    app: &AppHandle,
    audio_rx: &mut mpsc::UnboundedReceiver<AudioFrame>,
    stop_rx: &mut oneshot::Receiver<()>,
    no_timeout: bool,
    had_speech: &Arc<AtomicBool>,
) -> WaitForSpeechResult {
    let mut pre_buffer: Vec<u8> = Vec::with_capacity(PRE_SPEECH_BUFFER_MAX * 2);
    let started_at = tokio::time::Instant::now();
    let mut consecutive_speech_hits: u32 = 0;
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

                        match observe_speech_start(
                            &mut consecutive_speech_hits,
                            frame.has_speech,
                            started_at.elapsed(),
                        ) {
                            SpeechStartObservation::Guarded => {
                                log::debug!(
                                    "[wait_for_speech] ignored early spike during hotkey guard (rms={:.1})",
                                    frame.rms
                                );
                            }
                            SpeechStartObservation::Pending => {}
                            SpeechStartObservation::Confirmed => {
                                // Publish the VAD observation before returning.
                                // `do_stop_recording_impl` reads this flag on a
                                // different thread (hotkey/UI) under the state
                                // mutex. The state mutex does NOT automatically
                                // synchronize our atomic access because the
                                // store happens outside it — we need an
                                // explicit happens-before. Release on store +
                                // Acquire on load gives us that on ARM; on
                                // x86-TSO it compiles to the same instructions
                                // as Relaxed so there's no overhead on the
                                // current target, and we're correct if we ever
                                // run on weakly-ordered hardware.
                                had_speech.store(true, Ordering::Release);
                                log::info!(
                                    "[wait_for_speech] speech start confirmed after {} hits (rms={:.1}, elapsed={}ms)",
                                    consecutive_speech_hits,
                                    frame.rms,
                                    started_at.elapsed().as_millis()
                                );
                                return WaitForSpeechResult::Speech(pre_buffer);
                            }
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

#[cfg(test)]
mod tests {
    use super::{observe_speech_start, SpeechStartObservation};
    use crate::audio::{HOTKEY_NOISE_GUARD_MS, SPEECH_START_MIN_HITS};
    use std::time::Duration;

    #[test]
    fn early_spikes_inside_guard_do_not_accumulate_hits() {
        let mut hits = 0;
        let elapsed = Duration::from_millis(HOTKEY_NOISE_GUARD_MS.saturating_sub(1));

        assert_eq!(
            observe_speech_start(&mut hits, true, elapsed),
            SpeechStartObservation::Guarded
        );
        assert_eq!(hits, 0);
    }

    #[test]
    fn requires_consecutive_hits_after_guard_to_confirm_speech() {
        let mut hits = 0;
        let elapsed = Duration::from_millis(HOTKEY_NOISE_GUARD_MS + 1);

        for _ in 0..(SPEECH_START_MIN_HITS - 1) {
            assert_eq!(
                observe_speech_start(&mut hits, true, elapsed),
                SpeechStartObservation::Pending
            );
        }

        assert_eq!(
            observe_speech_start(&mut hits, true, elapsed),
            SpeechStartObservation::Confirmed
        );
    }

    #[test]
    fn silence_resets_consecutive_hits() {
        let mut hits = 0;
        let elapsed = Duration::from_millis(HOTKEY_NOISE_GUARD_MS + 1);

        assert_eq!(
            observe_speech_start(&mut hits, true, elapsed),
            SpeechStartObservation::Pending
        );
        assert_eq!(
            observe_speech_start(&mut hits, true, elapsed),
            SpeechStartObservation::Pending
        );
        assert_eq!(
            observe_speech_start(&mut hits, false, elapsed),
            SpeechStartObservation::Pending
        );
        assert_eq!(hits, 0);
        assert_eq!(
            observe_speech_start(&mut hits, true, elapsed),
            SpeechStartObservation::Pending
        );
    }
}
