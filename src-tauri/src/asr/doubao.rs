//! Doubao (豆包/火山引擎) ASR provider implementation.
//!
//! Uses the bigmodel_async endpoint (optimized bidirectional streaming)
//! with the Doubao binary WebSocket protocol.

use super::protocol::{self, ServerMessage};
use super::{net_log, truncate_for_log, wait_for_speech, AsrProvider, TranscriptUpdate, WaitForSpeechResult};
use crate::audio::AudioFrame;
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

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
    ) -> Result<(), String> {
        let settings = &self.settings;
        let chunk_size: usize = 16000 * 2 * 200 / 1000; // 6400 bytes = 200ms

        // ── Phase 1: Wait for speech (shared helper, 30s timeout) ──
        let no_timeout = super::should_skip_timeout(&app_handle);
        let pre_buffer = match wait_for_speech(&app_handle, &mut audio_rx, &mut stop_rx, no_timeout).await {
            WaitForSpeechResult::Speech(buf) => {
                net_log(&app_handle, "info", &format!(
                    "检测到语音 (缓冲 {} bytes, ~{:.1}s)，连接 ASR...",
                    buf.len(), buf.len() as f64 / 32000.0
                ));
                buf
            }
            WaitForSpeechResult::Stopped => {
                net_log(&app_handle, "info", "录音结束 (未检测到语音，未连接 ASR)");
                return Ok(());
            }
            WaitForSpeechResult::ChannelClosed => {
                net_log(&app_handle, "info", "音频通道已关闭 (未连接 ASR)");
                return Ok(());
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

        let mut request = url.into_client_request().map_err(|e| e.to_string())?;
        let headers = request.headers_mut();
        headers.insert(
            "X-Api-App-Key",
            settings.app_id.parse().map_err(|_| "Invalid App ID")?,
        );
        headers.insert(
            "X-Api-Access-Key",
            settings
                .access_token
                .parse()
                .map_err(|_| "Invalid Access Token")?,
        );
        headers.insert(
            "X-Api-Resource-Id",
            settings
                .resource_id
                .parse()
                .map_err(|_| "Invalid Resource ID")?,
        );
        headers.insert(
            "X-Api-Connect-Id",
            connect_id.parse().map_err(|_| "Invalid Connect ID")?,
        );

        let (ws_stream, response) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, None)
                .await
                .map_err(|e| {
                    let msg = format!("WebSocket 连接失败: {}", e);
                    net_log(&app_handle, "error", &msg);
                    msg
                })?;

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

        let full_request = protocol::build_full_client_request(&config.to_string())?;
        let req_size = full_request.len();
        ws_write
            .send(Message::Binary(full_request.into()))
            .await
            .map_err(|e| format!("发送配置失败: {}", e))?;

        net_log(
            &app_handle,
            "info",
            &format!("→ 发送 FullClientRequest ({} bytes)", req_size),
        );

        // Spawn reader task
        let (last_response_tx, last_response_rx) = oneshot::channel::<()>();
        let mut last_response_tx = Some(last_response_tx);

        let app_reader = app_handle.clone();
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
                                                    let update = TranscriptUpdate {
                                                        text: new_final,
                                                        is_final: true,
                                                        generation,
                                                    };
                                                    let _ = app_reader
                                                        .emit("transcription-update", &update);
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
                                    if let Some(tx) = last_response_tx.take() {
                                        let _ = tx.send(());
                                    }
                                }
                            }
                            Ok(ServerMessage::Error { code, message }) => {
                                net_log(
                                    &app_reader,
                                    "error",
                                    &format!("← ASR 错误 {}: {}", code, message),
                                );
                                let _ = app_reader
                                    .emit("asr-error", format!("ASR 错误 {}: {}", code, message));
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
                        let _ = app_reader.emit("asr-error", format!("连接错误: {}", e));
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
            if let Some(tx) = last_response_tx.take() {
                let _ = tx.send(());
            }
        });

        // ── Send pre-buffered audio (lookback) ──
        let mut total_audio_bytes: usize = 0;
        let mut packet_count: u32 = 0;

        let mut pos = 0;
        while pos + chunk_size <= pre_buffer.len() {
            let chunk = &pre_buffer[pos..pos + chunk_size];
            let packet = protocol::build_audio_request(chunk, false)?;
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
                frame = audio_rx.recv() => {
                    match frame {
                        Some(frame) => {
                            let _ = app_handle.emit("audio-level", frame.level);

                            buffer.extend_from_slice(&frame.pcm);
                            while buffer.len() >= chunk_size {
                                let packet = protocol::build_audio_request(&buffer[..chunk_size], false)?;
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
                        }
                        None => {
                            net_log(&app_handle, "info", "音频通道已关闭");
                            break;
                        }
                    }
                }
            }
        }

        // Send remaining buffer + final packet
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

        // Wait for final response
        net_log(&app_handle, "info", "等待最终响应...");
        let _ = tokio::time::timeout(std::time::Duration::from_secs(10), last_response_rx).await;

        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reader_task).await;

        let _ = app_handle.emit("connection-status", false);
        net_log(&app_handle, "info", "ASR 会话结束");

        Ok(())
    }
}
