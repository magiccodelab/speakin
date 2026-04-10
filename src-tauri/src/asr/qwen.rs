//! 千问语音识别 (Qwen ASR Realtime) provider.
//!
//! Uses the Alibaba Cloud Realtime API (OpenAI Realtime API compatible).
//! Protocol: All JSON messages with base64-encoded PCM audio.
//! Endpoint: wss://dashscope.aliyuncs.com/api-ws/v1/realtime

use super::{
    classify_error, net_log, truncate_for_log, wait_for_speech, AsrProvider, SessionOutcome,
    TranscriptUpdate, WaitForSpeechResult,
};
use crate::audio::AudioFrame;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// Qwen ASR-specific settings.
pub struct QwenSettings {
    pub api_key: String,
    pub model: String,
    pub region: String,
    pub language: String,
}

pub struct QwenProvider {
    pub settings: QwenSettings,
}

/// Generate a unique event ID.
fn evt_id() -> String {
    format!("evt_{}", uuid::Uuid::new_v4().simple())
}

/// Messages the reader task sends back to the main loop.
enum ReaderMsg {
    SessionReady,
    Final(String),
    Error(String),
    Done,
}

impl AsrProvider for QwenProvider {
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
        let session_started_at = Instant::now();
        let mut outcome = SessionOutcome::new();

        // Phase 1: Wait for speech (no server cost)
        net_log(&app_handle, "info", "等待语音检测...");
        let no_timeout = super::should_skip_timeout(&app_handle);
        let pre_buffer = match wait_for_speech(&app_handle, &mut audio_rx, &mut stop_rx, no_timeout, &had_speech).await {
            WaitForSpeechResult::Speech(buf) => {
                outcome.had_speech = true;
                crate::emit_app_log(
                    &app_handle,
                    "info",
                    &format!("已检测到有效语音，开始云端转写 [千问，会话 {}]", generation),
                );
                buf
            }
            WaitForSpeechResult::Stopped | WaitForSpeechResult::ChannelClosed => {
                outcome.had_speech = had_speech.load(Ordering::Acquire);
                outcome.aborted = aborted.load(Ordering::Acquire);
                outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
                return outcome;
            }
        };
        net_log(
            &app_handle,
            "info",
            &format!("检测到语音，预缓冲 {} bytes", pre_buffer.len()),
        );

        let finalize_with_error = |mut outcome: SessionOutcome, detail: String| -> SessionOutcome {
            net_log(&app_handle, "error", &format!("Qwen 错误: {}", detail));
            outcome.error = Some((classify_error(&detail), detail));
            outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
            outcome.aborted = aborted.load(Ordering::Acquire);
            outcome.had_speech = had_speech.load(Ordering::Acquire);
            outcome
        };

        // Phase 2: WebSocket connection
        let base_url = match settings.region.as_str() {
            "singapore" => "wss://dashscope-intl.aliyuncs.com/api-ws/v1/realtime",
            _ => "wss://dashscope.aliyuncs.com/api-ws/v1/realtime",
        };
        let encoded_model: String = settings
            .model
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                    (b as char).to_string()
                } else {
                    format!("%{:02X}", b)
                }
            })
            .collect();
        let url = format!("{}?model={}", base_url, encoded_model);

        let mut request = match url.as_str().into_client_request() {
            Ok(r) => r,
            Err(e) => return finalize_with_error(outcome, format!("请求构建失败: {}", e)),
        };
        {
            let headers = request.headers_mut();
            match format!("Bearer {}", settings.api_key).parse() {
                Ok(v) => { headers.insert("Authorization", v); }
                Err(e) => return finalize_with_error(outcome, format!("Header 格式错误: {}", e)),
            }
            match "realtime=v1".parse() {
                Ok(v) => { headers.insert("OpenAI-Beta", v); }
                Err(_) => return finalize_with_error(outcome, "OpenAI-Beta header 错误".to_string()),
            }
        }

        net_log(
            &app_handle,
            "info",
            &format!("→ 连接 Qwen ASR (model={})", settings.model),
        );
        let (ws_stream, _response) = match tokio_tungstenite::connect_async_tls_with_config(
            request, None, false, None,
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => return finalize_with_error(outcome, format!("WebSocket 连接失败: {}", e)),
        };

        let _ = app_handle.emit("connection-status", true);
        crate::emit_app_log(&app_handle, "info", "云端转写已连接 [千问]");
        net_log(&app_handle, "info", "← WebSocket 已连接");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Phase 3: Send session.update
        let session_update = serde_json::json!({
            "event_id": evt_id(),
            "type": "session.update",
            "session": {
                "modalities": ["text"],
                "input_audio_format": "pcm",
                "sample_rate": 16000,
                "input_audio_transcription": {
                    "language": &settings.language
                },
                "turn_detection": {
                    "type": "server_vad",
                    "threshold": 0.0,
                    "silence_duration_ms": 400
                }
            }
        });

        if let Err(e) = ws_write.send(Message::text(session_update.to_string())).await {
            return finalize_with_error(outcome, format!("发送 session.update 失败: {}", e));
        }
        net_log(&app_handle, "info", "→ session.update");

        // Phase 4: Start reader task
        let (reader_tx, mut reader_rx) = mpsc::unbounded_channel::<ReaderMsg>();
        let app_reader = app_handle.clone();
        let reader_tx_clone = reader_tx.clone();
        let reader_task = tauri::async_runtime::spawn(async move {
            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };

                        let event_type = json["type"].as_str().unwrap_or("");

                        match event_type {
                            "session.created" => {
                                let session_id =
                                    json["session"]["id"].as_str().unwrap_or("unknown");
                                net_log(
                                    &app_reader,
                                    "info",
                                    &format!("← session.created (id={})", session_id),
                                );
                            }
                            "session.updated" => {
                                net_log(&app_reader, "info", "← session.updated");
                                let _ = reader_tx_clone.send(ReaderMsg::SessionReady);
                            }
                            "input_audio_buffer.speech_started" => {
                                net_log(&app_reader, "info", "← VAD: speech started");
                            }
                            "input_audio_buffer.speech_stopped" => {
                                net_log(&app_reader, "info", "← VAD: speech stopped");
                            }
                            "conversation.item.input_audio_transcription.text" => {
                                let confirmed = json["text"].as_str().unwrap_or("");
                                let stash = json["stash"].as_str().unwrap_or("");
                                let combined = format!("{}{}", confirmed, stash);

                                if !combined.is_empty() {
                                    let update = TranscriptUpdate {
                                        text: combined.clone(),
                                        is_final: false,
                                        generation,
                                    };
                                    let _ = app_reader.emit("transcription-update", &update);
                                    net_log(
                                        &app_reader,
                                        "info",
                                        &format!(
                                            "← [interim] \"{}\"",
                                            truncate_for_log(&combined, 60)
                                        ),
                                    );
                                }
                            }
                            "conversation.item.input_audio_transcription.completed" => {
                                let transcript =
                                    json["transcript"].as_str().unwrap_or("");
                                if !transcript.is_empty() {
                                    let update = TranscriptUpdate {
                                        text: transcript.to_string(),
                                        is_final: true,
                                        generation,
                                    };
                                    let _ = app_reader.emit("transcription-update", &update);
                                    net_log(
                                        &app_reader,
                                        "info",
                                        &format!(
                                            "← [FINAL] \"{}\"",
                                            truncate_for_log(transcript, 60)
                                        ),
                                    );
                                    let _ = reader_tx_clone
                                        .send(ReaderMsg::Final(transcript.to_string()));
                                }
                            }
                            "conversation.item.input_audio_transcription.failed" => {
                                let error = json["error"]["message"]
                                    .as_str()
                                    .unwrap_or("转写失败");
                                let err_msg = format!("Qwen ASR 转写失败: {}", error);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = reader_tx_clone.send(ReaderMsg::Error(err_msg));
                            }
                            "error" => {
                                let error = json["error"]["message"]
                                    .as_str()
                                    .unwrap_or("未知错误");
                                let err_msg = format!("Qwen ASR 错误: {}", error);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = reader_tx_clone.send(ReaderMsg::Error(err_msg));
                                break;
                            }
                            "session.finished" => {
                                net_log(&app_reader, "info", "← session.finished");
                                let _ = reader_tx_clone.send(ReaderMsg::Done);
                                break;
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        let err = format!("Qwen 连接错误: {}", e);
                        net_log(&app_reader, "error", &err);
                        let _ = reader_tx_clone.send(ReaderMsg::Error(err));
                        break;
                    }
                    _ => {}
                }
            }
            // Unblock main task if still waiting.
            let _ = reader_tx_clone.send(ReaderMsg::Done);
        });
        drop(reader_tx);

        // Wait for session.updated before sending audio (5s timeout)
        let ready = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(msg) = reader_rx.recv().await {
                match msg {
                    ReaderMsg::SessionReady => return Ok(()),
                    ReaderMsg::Error(detail) => return Err(detail),
                    ReaderMsg::Final(text) => outcome.finals.push(text),
                    ReaderMsg::Done => return Err("reader ended before session.updated".to_string()),
                }
            }
            Err("reader channel closed".to_string())
        })
        .await;
        match ready {
            Ok(Ok(())) => {}
            Ok(Err(detail)) => {
                reader_task.abort();
                return finalize_with_error(outcome, detail);
            }
            Err(_) => {
                reader_task.abort();
                return finalize_with_error(outcome, "等待 session.updated 超时".to_string());
            }
        }

        // Phase 5: Send pre-buffered audio + streaming audio
        let chunk_size: usize = 3200; // ~100ms @ 16kHz 16-bit mono
        let mut event_counter: u64 = 0;
        let mut pos = 0;
        let mut packet_count: u32 = 0;
        let mut total_audio_bytes: usize = 0;

        while pos + chunk_size <= pre_buffer.len() {
            let chunk = &pre_buffer[pos..pos + chunk_size];
            let b64 = BASE64.encode(chunk);
            let msg = serde_json::json!({
                "event_id": format!("evt_audio_{}", event_counter),
                "type": "input_audio_buffer.append",
                "audio": b64
            });
            event_counter += 1;
            if let Err(e) = ws_write.send(Message::text(msg.to_string())).await {
                reader_task.abort();
                return finalize_with_error(outcome, format!("发送预缓冲音频失败: {}", e));
            }
            pos += chunk_size;
            packet_count += 1;
            total_audio_bytes += chunk_size;
        }

        let mut buffer: Vec<u8> = pre_buffer[pos..].to_vec();
        drop(pre_buffer);

        net_log(
            &app_handle,
            "info",
            &format!("→ 预缓冲发送 {} 包 ({} bytes)", packet_count, total_audio_bytes),
        );

        // Stream live audio
        let mut error_opt: Option<String> = None;
        let mut done_received = false;
        loop {
            tokio::select! {
                biased;
                _ = &mut stop_rx => {
                    net_log(&app_handle, "info", &format!(
                        "停止录音 (共 {} 包, {} bytes, ~{:.1}s)",
                        packet_count, total_audio_bytes, total_audio_bytes as f64 / 32000.0
                    ));
                    break;
                }
                msg = reader_rx.recv() => {
                    match msg {
                        Some(ReaderMsg::Final(text)) => outcome.finals.push(text),
                        Some(ReaderMsg::Error(detail)) => {
                            error_opt = Some(detail);
                            break;
                        }
                        Some(ReaderMsg::Done) => {
                            done_received = true;
                            break;
                        }
                        Some(ReaderMsg::SessionReady) => {} // ignore, already handled
                        None => break,
                    }
                }
                frame = audio_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            let _ = app_handle.emit("audio-level", frame.level);
                            buffer.extend_from_slice(&frame.pcm);

                            while buffer.len() >= chunk_size {
                                let b64 = BASE64.encode(&buffer[..chunk_size]);
                                let msg = serde_json::json!({
                                    "event_id": format!("evt_audio_{}", event_counter),
                                    "type": "input_audio_buffer.append",
                                    "audio": b64
                                });
                                event_counter += 1;

                                if ws_write
                                    .send(Message::text(msg.to_string()))
                                    .await
                                    .is_err()
                                {
                                    net_log(&app_handle, "error", "→ 发送音频包失败，中止发送");
                                    error_opt = Some("音频发送失败".to_string());
                                    break;
                                }
                                buffer.drain(..chunk_size);
                                packet_count += 1;
                                total_audio_bytes += chunk_size;

                                if packet_count % 50 == 1 {
                                    net_log(&app_handle, "send", &format!(
                                        "→ 音频包 #{} ({} bytes 累计)",
                                        packet_count, total_audio_bytes
                                    ));
                                }
                            }
                            if error_opt.is_some() { break; }
                        }
                        None => {
                            net_log(&app_handle, "info", "音频通道已关闭");
                            break;
                        }
                    }
                }
            }
        }

        if error_opt.is_none() && !done_received {
            // Send remaining buffer
            if !buffer.is_empty() {
                let b64 = BASE64.encode(&buffer);
                let msg = serde_json::json!({
                    "event_id": format!("evt_audio_{}", event_counter),
                    "type": "input_audio_buffer.append",
                    "audio": b64
                });
                let _ = ws_write.send(Message::text(msg.to_string())).await;
                net_log(
                    &app_handle,
                    "info",
                    &format!("→ 发送剩余缓冲 ({} bytes)", buffer.len()),
                );
            }

            // Phase 6: Send session.finish
            let finish_msg = serde_json::json!({
                "event_id": evt_id(),
                "type": "session.finish"
            });
            let _ = ws_write.send(Message::text(finish_msg.to_string())).await;
            net_log(&app_handle, "info", "→ session.finish");

            // Wait for session.finished (5s cap)
            let wait_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async {
                    while let Some(msg) = reader_rx.recv().await {
                        match msg {
                            ReaderMsg::Final(text) => outcome.finals.push(text),
                            ReaderMsg::Error(detail) => return Some(detail),
                            ReaderMsg::Done => return None,
                            ReaderMsg::SessionReady => {}
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
                    net_log(&app_handle, "warn", "等待 session.finished 超时 (5s)，使用已有累积结果");
                }
            }
        }

        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), reader_task).await;

        crate::emit_app_log(&app_handle, "info", "云端转写已结束 [千问]");
        net_log(&app_handle, "info", "Qwen ASR 会话结束");

        outcome.had_speech = had_speech.load(Ordering::Acquire);
        outcome.aborted = aborted.load(Ordering::Acquire);
        outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
        if let Some(detail) = error_opt {
            outcome.error = Some((classify_error(&detail), detail));
        }
        outcome
    }
}
