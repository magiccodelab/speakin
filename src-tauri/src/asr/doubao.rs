//! Doubao (豆包/火山引擎) ASR provider implementation.
//!
//! Uses the bigmodel_async endpoint (optimized bidirectional streaming)
//! with the Doubao binary WebSocket protocol.

use super::protocol::{self, ServerMessage};
use super::{
    classify_error, net_log, truncate_for_log, wait_for_speech, AsrProvider, SessionOutcome,
    TranscriptUpdate, WaitForSpeechResult,
};
use crate::audio::AudioFrame;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// Messages that the reader task sends back to the main ASR loop.
/// Lets the reader signal end-of-stream or error without sharing state.
enum ReaderMsg {
    /// A new `definite=true` final sentence. Pushed into `accumulated_finals`.
    Final(String),
    /// WebSocket dropped or server sent an error. Carries the raw detail.
    Error(String),
    /// Server sent the last-flag response — normal completion.
    Done,
}

/// Doubao ASR mode — determines endpoint and capabilities.
#[derive(Debug, Clone, PartialEq)]
pub enum DoubaoMode {
    /// Bidirectional streaming (optimized) — real-time, Chinese/English only.
    /// Endpoint: bigmodel_async
    BiStream,
    /// Streaming input — sentence-level return, supports dialects & 25 languages.
    /// Endpoint: bigmodel_nostream
    NoStream,
}

/// Doubao-specific settings (App ID, Access Token, Resource ID, Mode).
pub struct DoubaoSettings {
    pub app_id: String,
    pub access_token: String,
    pub resource_id: String,
    pub mode: DoubaoMode,
}

/// Doubao ASR provider.
pub struct DoubaoProvider {
    pub settings: DoubaoSettings,
}

// Typed deserialization structs for Doubao JSON responses
#[derive(Deserialize)]
struct AsrResponse {
    result: Option<AsrResultInner>,
}

#[derive(Deserialize)]
struct AsrResultInner {
    text: Option<String>,
    utterances: Option<Vec<Utterance>>,
}

#[derive(Deserialize)]
struct Utterance {
    text: String,
    #[serde(default)]
    definite: bool,
}

impl AsrProvider for DoubaoProvider {
    async fn run_session(
        &self,
        app_handle: AppHandle,
        generation: u64,
        mut audio_rx: mpsc::UnboundedReceiver<AudioFrame>,
        mut stop_rx: oneshot::Receiver<()>,
        had_speech: Arc<AtomicBool>,
        aborted: Arc<AtomicBool>,
    ) -> SessionOutcome {
        let settings = &self.settings;
        let chunk_size: usize = 16000 * 2 * 200 / 1000; // 6400 bytes = 200ms
        let session_started_at = Instant::now();

        // Session-scoped outcome — populated as the session progresses and
        // returned at every exit path. `finalize_session` (called by the
        // orchestrator in lib.rs) turns this into a `session-ended` event
        // and persists the accumulated text.
        let mut outcome = SessionOutcome::new();

        // ── Phase 1: Wait for speech (shared helper, 30s timeout) ──
        let no_timeout = super::should_skip_timeout(&app_handle);
        let pre_buffer = match wait_for_speech(&app_handle, &mut audio_rx, &mut stop_rx, no_timeout, &had_speech).await {
            WaitForSpeechResult::Speech(buf) => {
                outcome.had_speech = true;
                crate::emit_app_log(
                    &app_handle,
                    "info",
                    &format!("已检测到有效语音，开始云端转写 [豆包，会话 {}]", generation),
                );
                net_log(&app_handle, "info", &format!(
                    "检测到语音 (缓冲 {} bytes, ~{:.1}s)，连接 ASR...",
                    buf.len(), buf.len() as f64 / 32000.0
                ));
                buf
            }
            WaitForSpeechResult::Stopped => {
                net_log(&app_handle, "info", "录音结束 (未检测到语音，未连接 ASR)");
                outcome.had_speech = had_speech.load(Ordering::Acquire);
                outcome.aborted = aborted.load(Ordering::Acquire);
                outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
                return outcome;
            }
            WaitForSpeechResult::ChannelClosed => {
                net_log(&app_handle, "info", "音频通道已关闭 (未连接 ASR)");
                outcome.had_speech = had_speech.load(Ordering::Acquire);
                outcome.aborted = aborted.load(Ordering::Acquire);
                outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
                return outcome;
            }
        };

        // ── Phase 2: Connect and stream ──

        let connect_id = uuid::Uuid::new_v4().to_string();
        let url = match settings.mode {
            DoubaoMode::BiStream => "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async",
            DoubaoMode::NoStream => "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_nostream",
        };

        net_log(
            &app_handle,
            "info",
            &format!("→ 连接 ASR: {} (id={})", url, &connect_id[..8]),
        );

        // Helper closure: record an error into the outcome and return it.
        // Used for all the early-return paths in Phase 2 setup.
        let finalize_with_error = |mut outcome: SessionOutcome, detail: String| -> SessionOutcome {
            let reason = classify_error(&detail);
            net_log(&app_handle, "error", &format!("ASR 错误: {}", detail));
            outcome.error = Some((reason, detail));
            outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
            outcome.aborted = aborted.load(Ordering::Acquire);
            outcome
        };

        let mut request = match url.into_client_request() {
            Ok(r) => r,
            Err(e) => return finalize_with_error(outcome, e.to_string()),
        };
        let headers = request.headers_mut();
        match settings.app_id.parse() {
            Ok(v) => { headers.insert("X-Api-App-Key", v); }
            Err(_) => return finalize_with_error(outcome, "Invalid App ID".to_string()),
        }
        match settings.access_token.parse() {
            Ok(v) => { headers.insert("X-Api-Access-Key", v); }
            Err(_) => return finalize_with_error(outcome, "Invalid Access Token".to_string()),
        }
        match settings.resource_id.parse() {
            Ok(v) => { headers.insert("X-Api-Resource-Id", v); }
            Err(_) => return finalize_with_error(outcome, "Invalid Resource ID".to_string()),
        }
        match connect_id.parse() {
            Ok(v) => { headers.insert("X-Api-Connect-Id", v); }
            Err(_) => return finalize_with_error(outcome, "Invalid Connect ID".to_string()),
        }

        let (ws_stream, response) = match tokio_tungstenite::connect_async_tls_with_config(
            request, None, false, None,
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => {
                return finalize_with_error(outcome, format!("WebSocket 连接失败: {}", e));
            }
        };

        let logid = response
            .headers()
            .get("X-Tt-Logid")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown")
            .to_string();
        net_log(
            &app_handle,
            "info",
            &format!("← 连接成功 (logid={})", logid),
        );

        let _ = app_handle.emit("connection-status", true);
        crate::emit_app_log(&app_handle, "info", "云端转写已连接 [豆包]");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Send full client request — config varies by mode
        let mut request_config = serde_json::json!({
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": false,
            "show_utterances": true,
            "result_type": "full",
        });
        match settings.mode {
            DoubaoMode::BiStream => {
                // Optimized bidirectional: enable two-pass recognition
                request_config["enable_nonstream"] = serde_json::json!(true);
                request_config["end_window_size"] = serde_json::json!(5000);
            }
            DoubaoMode::NoStream => {
                // Streaming input: higher accuracy, supports dialects
            }
        }
        let config = serde_json::json!({
            "user": { "uid": "speakin" },
            "audio": {
                "format": "pcm",
                "rate": 16000,
                "bits": 16,
                "channel": 1
            },
            "request": request_config
        });

        let full_request = match protocol::build_full_client_request(&config.to_string()) {
            Ok(r) => r,
            Err(e) => return finalize_with_error(outcome, e),
        };
        let req_size = full_request.len();
        if let Err(e) = ws_write.send(Message::Binary(full_request.into())).await {
            return finalize_with_error(outcome, format!("发送配置失败: {}", e));
        }

        net_log(
            &app_handle,
            "info",
            &format!("→ 发送 FullClientRequest ({} bytes)", req_size),
        );

        // ── Spawn reader task ──
        //
        // Reader streams results to the main task via an mpsc channel. It
        // does NOT emit `asr-error` or otherwise try to terminate the
        // session on its own — the main task owns that responsibility and
        // will construct the `SessionOutcome`.
        let (reader_tx, mut reader_rx) = mpsc::unbounded_channel::<ReaderMsg>();
        let app_reader = app_handle.clone();
        let reader_tx_clone = reader_tx.clone();
        let reader_task = tauri::async_runtime::spawn(async move {
            let mut msg_count: u32 = 0;
            let mut committed_count: usize = 0;

            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Binary(data)) => {
                        msg_count += 1;
                        match protocol::parse_server_response(&data) {
                            Ok(ServerMessage::Response {
                                sequence,
                                payload,
                                _is_last,
                            }) => {
                                if let Ok(resp) = serde_json::from_str::<AsrResponse>(&payload) {
                                    if let Some(ref result) = resp.result {
                                        if let Some(ref utts) = result.utterances {
                                            let definite_count =
                                                utts.iter().filter(|u| u.definite).count();

                                            if definite_count > committed_count {
                                                let mut new_final = String::new();
                                                for utt in utts
                                                    .iter()
                                                    .skip(committed_count)
                                                    .take(definite_count - committed_count)
                                                {
                                                    new_final.push_str(&utt.text);
                                                }

                                                if !new_final.is_empty() {
                                                    net_log(
                                                        &app_reader,
                                                        "recv",
                                                        &format!(
                                                            "← seq={} [FINAL +{}] text=\"{}\"",
                                                            sequence,
                                                            definite_count - committed_count,
                                                            truncate_for_log(&new_final, 80),
                                                        ),
                                                    );
                                                    // Emit for live UI rendering.
                                                    let update = TranscriptUpdate {
                                                        text: new_final.clone(),
                                                        is_final: true,
                                                        generation,
                                                    };
                                                    let _ = app_reader
                                                        .emit("transcription-update", &update);
                                                    // And hand off to the main task for accumulation.
                                                    let _ = reader_tx_clone
                                                        .send(ReaderMsg::Final(new_final));
                                                }
                                                committed_count = definite_count;
                                            }

                                            let mut interim = String::new();
                                            for utt in utts.iter().filter(|u| !u.definite) {
                                                interim.push_str(&utt.text);
                                            }

                                            if !interim.is_empty() {
                                                net_log(
                                                    &app_reader,
                                                    "recv",
                                                    &format!(
                                                        "← seq={} text=\"{}\"",
                                                        sequence,
                                                        truncate_for_log(&interim, 80),
                                                    ),
                                                );
                                                let update = TranscriptUpdate {
                                                    text: interim,
                                                    is_final: false,
                                                    generation,
                                                };
                                                let _ = app_reader
                                                    .emit("transcription-update", &update);
                                            }
                                        } else if let Some(ref text) = result.text {
                                            if !text.is_empty() {
                                                net_log(
                                                    &app_reader,
                                                    "recv",
                                                    &format!(
                                                        "← seq={} text=\"{}\"",
                                                        sequence,
                                                        truncate_for_log(text, 80),
                                                    ),
                                                );
                                                let update = TranscriptUpdate {
                                                    text: text.clone(),
                                                    is_final: false,
                                                    generation,
                                                };
                                                let _ = app_reader
                                                    .emit("transcription-update", &update);
                                            }
                                        }
                                    }
                                } else {
                                    net_log(
                                        &app_reader,
                                        "info",
                                        &format!(
                                            "← seq={} 非转写响应: {}",
                                            sequence,
                                            truncate_for_log(&payload, 80),
                                        ),
                                    );
                                }

                                if _is_last {
                                    net_log(
                                        &app_reader,
                                        "info",
                                        &format!(
                                            "← seq={} [LAST] 服务端最终响应 (共 {} 条消息)",
                                            sequence, msg_count
                                        ),
                                    );
                                    let _ = reader_tx_clone.send(ReaderMsg::Done);
                                    // Don't break — we may still get trailing frames,
                                    // but the main task can proceed now.
                                }
                            }
                            Ok(ServerMessage::Error { code, message }) => {
                                net_log(
                                    &app_reader,
                                    "error",
                                    &format!("← ASR 错误 {}: {}", code, message),
                                );
                                let _ = reader_tx_clone.send(ReaderMsg::Error(format!(
                                    "ASR 错误 {}: {}",
                                    code, message
                                )));
                                break;
                            }
                            Err(e) => {
                                net_log(&app_reader, "error", &format!("← 解析响应失败: {}", e));
                            }
                        }
                    }
                    Ok(Message::Close(frame)) => {
                        net_log(
                            &app_reader,
                            "info",
                            &format!("← 连接关闭 {:?} (收到 {} 条消息)", frame, msg_count),
                        );
                        break;
                    }
                    Ok(Message::Text(text)) => {
                        net_log(
                            &app_reader,
                            "warn",
                            &format!("← 收到文本帧: {}", truncate_for_log(&text, 100)),
                        );
                    }
                    Err(e) => {
                        net_log(&app_reader, "error", &format!("← WebSocket 错误: {}", e));
                        let _ = reader_tx_clone
                            .send(ReaderMsg::Error(format!("连接错误: {}", e)));
                        break;
                    }
                    _ => {}
                }
            }
            net_log(
                &app_reader,
                "info",
                &format!("Reader 退出 (共 {} 条消息)", msg_count),
            );
            // Make sure the main task unblocks if it's waiting for Done/Error.
            let _ = reader_tx_clone.send(ReaderMsg::Done);
        });
        // We keep the original `reader_tx` around ONLY so the channel
        // isn't closed prematurely if the reader task panics — the Clone
        // held by the reader itself closes when the task exits, but we
        // want to use `reader_rx.recv()` in the main loop below and let
        // it gracefully observe channel closure as "reader exited".
        drop(reader_tx);

        // ── Send pre-buffered audio (lookback) ──
        let mut total_audio_bytes: usize = 0;
        let mut packet_count: u32 = 0;

        let mut pos = 0;
        while pos + chunk_size <= pre_buffer.len() {
            let chunk = &pre_buffer[pos..pos + chunk_size];
            let packet = match protocol::build_audio_request(chunk, false) {
                Ok(p) => p,
                Err(e) => {
                    reader_task.abort();
                    return finalize_with_error(outcome, e);
                }
            };
            total_audio_bytes += chunk_size;
            packet_count += 1;
            if ws_write.send(Message::Binary(packet.into())).await.is_err() {
                net_log(&app_handle, "error", "→ 发送预缓冲音频失败");
            }
            pos += chunk_size;
        }
        let mut buffer: Vec<u8> = pre_buffer[pos..].to_vec();
        drop(pre_buffer);

        net_log(
            &app_handle,
            "send",
            &format!(
                "→ 发送预缓冲 {} 包 ({} bytes, ~{:.1}s)",
                packet_count,
                total_audio_bytes,
                total_audio_bytes as f64 / 32000.0
            ),
        );

        // ── Audio sending loop ──
        //
        // `done_received` flips true when the reader reports the server's
        // `_is_last` frame OR when the reader channel closes (reader
        // exited). `error_opt` is populated when the reader reports an
        // error — in that case we break out of the loop immediately
        // without bothering to send more audio or a final packet.
        let mut done_received = false;
        let mut error_opt: Option<String> = None;
        loop {
            tokio::select! {
                biased;

                _ = &mut stop_rx => {
                    net_log(&app_handle, "info", &format!(
                        "停止录音 (共发送 {} 包, {} bytes, ~{:.1}s)",
                        packet_count, total_audio_bytes, total_audio_bytes as f64 / 32000.0
                    ));
                    break;
                }
                msg = reader_rx.recv() => {
                    match msg {
                        Some(ReaderMsg::Final(text)) => {
                            outcome.finals.push(text);
                        }
                        Some(ReaderMsg::Error(detail)) => {
                            error_opt = Some(detail);
                            break;
                        }
                        Some(ReaderMsg::Done) => {
                            done_received = true;
                            // Keep sending audio until stop — only bi-stream
                            // mode sends Done mid-session for two-pass
                            // confirmation. In nostream mode, Done means
                            // the server has given its final answer.
                            if matches!(settings.mode, DoubaoMode::NoStream) {
                                break;
                            }
                        }
                        None => {
                            // Reader channel closed — reader exited (either
                            // normally after Done or via WebSocket drop).
                            break;
                        }
                    }
                }
                frame = audio_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            let _ = app_handle.emit("audio-level", frame.level);

                            buffer.extend_from_slice(&frame.pcm);
                            while buffer.len() >= chunk_size {
                                let packet = match protocol::build_audio_request(&buffer[..chunk_size], false) {
                                    Ok(p) => p,
                                    Err(e) => {
                                        error_opt = Some(format!("音频打包失败: {}", e));
                                        buffer.clear();
                                        break;
                                    }
                                };
                                total_audio_bytes += chunk_size;
                                packet_count += 1;
                                buffer.drain(..chunk_size);

                                if packet_count % 25 == 1 {
                                    net_log(&app_handle, "send", &format!(
                                        "→ 音频包 #{} ({} bytes 累计, ~{}ms)",
                                        packet_count, total_audio_bytes, total_audio_bytes / 32
                                    ));
                                }

                                if ws_write.send(Message::Binary(packet.into())).await.is_err() {
                                    net_log(&app_handle, "error", "→ 发送音频包失败");
                                    break;
                                }
                            }
                            if error_opt.is_some() {
                                break;
                            }
                        }
                        None => {
                            net_log(&app_handle, "info", "音频通道已关闭");
                            break;
                        }
                    }
                }
            }
        }

        // If the reader reported an error, don't waste time sending the
        // final packet — just wrap up. Any finals we already pushed into
        // `outcome.finals` will be persisted by finalize_session as a
        // "partial" record.
        if error_opt.is_none() && !done_received {
            // Send remaining buffer + final packet so the server can emit
            // its FINAL before we exit.
            if !buffer.is_empty() {
                if let Ok(packet) = protocol::build_audio_request(&buffer, false) {
                    let _ = ws_write.send(Message::Binary(packet.into())).await;
                    net_log(
                        &app_handle,
                        "info",
                        &format!("→ 发送剩余缓冲 ({} bytes)", buffer.len()),
                    );
                }
            }
            if let Ok(final_packet) = protocol::build_audio_request(&[], true) {
                let _ = ws_write.send(Message::Binary(final_packet.into())).await;
                net_log(&app_handle, "info", "→ 发送结束包 (final)");
            }

            // Wait for the reader to receive the server's last frame or
            // drop the connection. Capped at 5 seconds — we don't want the
            // user to perceive any stall longer than that, especially if
            // the network just quietly died.
            net_log(&app_handle, "info", "等待最终响应...");
            let wait_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async {
                    while let Some(msg) = reader_rx.recv().await {
                        match msg {
                            ReaderMsg::Final(text) => outcome.finals.push(text),
                            ReaderMsg::Error(detail) => return Some(detail),
                            ReaderMsg::Done => return None,
                        }
                    }
                    None
                },
            )
            .await;
            match wait_result {
                Ok(Some(detail)) => error_opt = Some(detail),
                Ok(None) => {}
                Err(_) => {
                    net_log(&app_handle, "warn", "等待最终响应超时 (5s)，使用已有累积结果");
                }
            }
        }

        // Give the reader task a brief chance to exit cleanly.
        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), reader_task).await;

        crate::emit_app_log(&app_handle, "info", "云端转写已结束 [豆包]");
        net_log(&app_handle, "info", "ASR 会话结束");

        outcome.had_speech = had_speech.load(Ordering::Acquire);
        outcome.aborted = aborted.load(Ordering::Acquire);
        outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
        if let Some(detail) = error_opt {
            outcome.error = Some((classify_error(&detail), detail));
        }
        outcome
    }
}
