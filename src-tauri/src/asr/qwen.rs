//! 千问语音识别 (Qwen ASR Realtime) provider.
//!
//! Uses the Alibaba Cloud Realtime API (OpenAI Realtime API compatible).
//! Protocol: All JSON messages with base64-encoded PCM audio.
//! Endpoint: wss://dashscope.aliyuncs.com/api-ws/v1/realtime

use super::{net_log, truncate_for_log, wait_for_speech, AsrProvider, TranscriptUpdate, WaitForSpeechResult};
use crate::audio::AudioFrame;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use futures_util::{SinkExt, StreamExt};
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

impl AsrProvider for QwenProvider {
    async fn run_session(
        &self,
        app_handle: AppHandle,
        generation: u64,
        mut audio_rx: mpsc::UnboundedReceiver<AudioFrame>,
        mut stop_rx: oneshot::Receiver<()>,
    ) -> Result<(), String> {
        let settings = &self.settings;

        // Phase 1: Wait for speech (no server cost)
        net_log(&app_handle, "info", "等待语音检测...");
        let no_timeout = super::should_skip_timeout(&app_handle);
        let pre_buffer = match wait_for_speech(&app_handle, &mut audio_rx, &mut stop_rx, no_timeout).await {
            WaitForSpeechResult::Speech(buf) => buf,
            WaitForSpeechResult::Stopped => {
                net_log(&app_handle, "info", "语音检测前已停止");
                return Ok(());
            }
            WaitForSpeechResult::ChannelClosed => {
                net_log(&app_handle, "info", "音频通道已关闭");
                return Ok(());
            }
        };
        net_log(
            &app_handle,
            "info",
            &format!("检测到语音，预缓冲 {} bytes", pre_buffer.len()),
        );

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

        let mut request = url.as_str().into_client_request().map_err(|e| format!("请求构建失败: {}", e))?;

        {
            let headers = request.headers_mut();
            headers.insert(
                "Authorization",
                format!("Bearer {}", settings.api_key)
                    .parse()
                    .map_err(|e| format!("Header 格式错误: {}", e))?,
            );
            headers.insert(
                "OpenAI-Beta",
                "realtime=v1"
                    .parse()
                    .map_err(|_| "OpenAI-Beta header 错误")?,
            );
        }

        net_log(
            &app_handle,
            "info",
            &format!("→ 连接 Qwen ASR (model={})", settings.model),
        );
        let (ws_stream, _response) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, None)
                .await
                .map_err(|e| format!("WebSocket 连接失败: {}", e))?;

        let _ = app_handle.emit("connection-status", true);
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

        ws_write
            .send(Message::text(session_update.to_string()))
            .await
            .map_err(|e| format!("发送 session.update 失败: {}", e))?;
        net_log(&app_handle, "info", "→ session.update");

        // Phase 4: Start reader task
        let (session_ready_tx, session_ready_rx) = oneshot::channel::<()>();
        let (last_response_tx, last_response_rx) = oneshot::channel::<()>();
        let app_reader = app_handle.clone();

        let reader_task = tauri::async_runtime::spawn(async move {
            let mut session_ready_tx = Some(session_ready_tx);
            let mut last_response_tx = Some(last_response_tx);

            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };

                        let event_type = json["type"].as_str().unwrap_or("");

                        match event_type {
                            "session.created" => {
                                let session_id = json["session"]["id"]
                                    .as_str()
                                    .unwrap_or("unknown");
                                net_log(
                                    &app_reader,
                                    "info",
                                    &format!("← session.created (id={})", session_id),
                                );
                            }
                            "session.updated" => {
                                net_log(&app_reader, "info", "← session.updated");
                                if let Some(tx) = session_ready_tx.take() {
                                    let _ = tx.send(());
                                }
                            }
                            "input_audio_buffer.speech_started" => {
                                net_log(&app_reader, "info", "← VAD: speech started");
                            }
                            "input_audio_buffer.speech_stopped" => {
                                net_log(&app_reader, "info", "← VAD: speech stopped");
                            }
                            "conversation.item.input_audio_transcription.text" => {
                                // Interim result: text (confirmed prefix) + stash (draft suffix)
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
                                }
                            }
                            "conversation.item.input_audio_transcription.failed" => {
                                let error = json["error"]["message"]
                                    .as_str()
                                    .unwrap_or("转写失败");
                                let err_msg = format!("Qwen ASR 转写失败: {}", error);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = app_reader.emit("asr-error", &err_msg);
                            }
                            "error" => {
                                let error = json["error"]["message"]
                                    .as_str()
                                    .unwrap_or("未知错误");
                                let err_msg = format!("Qwen ASR 错误: {}", error);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = app_reader.emit("asr-error", &err_msg);
                                break;
                            }
                            "session.finished" => {
                                net_log(&app_reader, "info", "← session.finished");
                                if let Some(tx) = last_response_tx.take() {
                                    let _ = tx.send(());
                                }
                                break;
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        let err = format!("Qwen 连接错误: {}", e);
                        net_log(&app_reader, "error", &err);
                        let _ = app_reader.emit("asr-error", &err);
                        break;
                    }
                    _ => {}
                }
            }

            // Signal last_response waiter on exit (prevents 10s hang).
            // NOTE: Do NOT signal session_ready_tx here — if it wasn't sent during
            // normal operation, the 5s timeout will produce a clear error message.
            if let Some(tx) = last_response_tx.take() {
                let _ = tx.send(());
            }
        });

        // Wait for session.updated before sending audio (5s timeout)
        if tokio::time::timeout(std::time::Duration::from_secs(5), session_ready_rx)
            .await
            .is_err()
        {
            let _ = app_handle.emit("connection-status", false);
            return Err("等待 session.updated 超时".to_string());
        }

        // Phase 5: Send pre-buffered audio + streaming audio
        let chunk_size: usize = 3200; // ~100ms @ 16kHz 16-bit mono
        let mut event_counter: u64 = 0;

        // Send pre-buffer in chunks
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
            ws_write
                .send(Message::text(msg.to_string()))
                .await
                .map_err(|e| format!("发送预缓冲音频失败: {}", e))?;
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
        let mut send_error = false;
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
                                    send_error = true;
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
                            if send_error { break; }
                        }
                        None => {
                            net_log(&app_handle, "info", "音频通道已关闭");
                            break;
                        }
                    }
                }
            }
            if send_error { break; }
        }

        // Send remaining buffer
        if !buffer.is_empty() {
            let b64 = BASE64.encode(&buffer);
            let msg = serde_json::json!({
                "event_id": format!("evt_audio_{}", event_counter),
                "type": "input_audio_buffer.append",
                "audio": b64
            });
            let _ = ws_write
                .send(Message::text(msg.to_string()))
                .await;
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
        let _ = ws_write
            .send(Message::text(finish_msg.to_string()))
            .await;
        net_log(&app_handle, "info", "→ session.finish");

        // Wait for session.finished (10s timeout)
        // Note: final transcription events may arrive between session.finish and session.finished
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(10), last_response_rx).await;

        // Wait for reader task (2s timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reader_task).await;

        let _ = app_handle.emit("connection-status", false);
        net_log(&app_handle, "info", "Qwen ASR 会话结束");

        Ok(())
    }
}
