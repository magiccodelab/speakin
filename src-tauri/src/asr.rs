use crate::audio::AudioFrame;
use crate::protocol::{self, ServerMessage};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::borrow::Cow;
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct AsrSettings {
    pub app_id: String,
    pub access_token: String,
    pub resource_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TranscriptUpdate {
    pub text: String,
    pub is_final: bool,
}

// 4A: Typed deserialization structs — avoids dynamic serde_json::Value tree
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

fn net_log(app: &AppHandle, level: &str, msg: &str) {
    crate::emit_log(app, level, msg);
    match level {
        "error" => log::error!("{}", msg),
        "warn" => log::warn!("{}", msg),
        _ => log::info!("{}", msg),
    }
}

/// Maximum pre-speech audio to buffer (in bytes).
/// 1 second at 16kHz 16-bit mono = 32000 bytes.
/// This "lookback" buffer is sent to ASR on connect so the beginning of
/// speech is not clipped.
const PRE_SPEECH_BUFFER_MAX: usize = 32000;

pub async fn run_session(
    app_handle: AppHandle,
    settings: AsrSettings,
    mut audio_rx: mpsc::UnboundedReceiver<AudioFrame>,
    stop_rx: oneshot::Receiver<()>,
) -> Result<(), String> {
    let chunk_size: usize = 16000 * 2 * 200 / 1000; // 6400 bytes = 200ms

    // ── Phase 1: Wait for speech before connecting ──
    // Buffer audio locally. Only establish the WebSocket connection when
    // we detect actual speech. This avoids API charges for idle time.

    net_log(&app_handle, "info", "等待语音... (未连接 ASR，不计费)");

    let mut pre_buffer: Vec<u8> = Vec::with_capacity(PRE_SPEECH_BUFFER_MAX * 2);
    let mut stop_rx = stop_rx;

    loop {
        tokio::select! {
            biased;
            _ = &mut stop_rx => {
                net_log(&app_handle, "info", "录音结束 (未检测到语音，未连接 ASR)");
                return Ok(());
            }
            frame = audio_rx.recv() => {
                match frame {
                    Some(frame) => {
                        // Use pre-computed level from AudioFrame (no redundant RMS)
                        let _ = app_handle.emit("audio-level", frame.level);

                        pre_buffer.extend_from_slice(&frame.pcm);

                        // Use pre-computed speech flag (no redundant RMS)
                        if frame.has_speech {
                            net_log(&app_handle, "info", &format!(
                                "检测到语音 (缓冲 {} bytes, ~{:.1}s)，连接 ASR...",
                                pre_buffer.len(), pre_buffer.len() as f64 / 32000.0
                            ));
                            break; // → Phase 2: connect
                        }

                        // Keep pre-buffer bounded: retain the last N bytes as lookback
                        if pre_buffer.len() > PRE_SPEECH_BUFFER_MAX * 2 {
                            let drain_to = pre_buffer.len() - PRE_SPEECH_BUFFER_MAX;
                            pre_buffer.drain(..drain_to);
                        }
                    }
                    None => {
                        net_log(&app_handle, "info", "音频通道已关闭 (未连接 ASR)");
                        return Ok(());
                    }
                }
            }
        }
    }

    // ── Phase 2: Connect and stream ──

    let connect_id = uuid::Uuid::new_v4().to_string();
    let url = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_async";

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

    // Send full client request
    let config = serde_json::json!({
        "user": { "uid": "voice-input-app" },
        "audio": {
            "format": "pcm",
            "rate": 16000,
            "bits": 16,
            "channel": 1
        },
        "request": {
            "model_name": "bigmodel",
            "enable_itn": true,
            "enable_punc": true,
            "enable_ddc": false,
            "show_utterances": true,
            "result_type": "full",
            "end_window_size": 5000
        }
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
                            // 4A: Typed deserialization instead of dynamic Value
                            if let Ok(resp) = serde_json::from_str::<AsrResponse>(&payload) {
                                if let Some(ref result) = resp.result {
                                    if let Some(ref utts) = result.utterances {
                                        let definite_count =
                                            utts.iter().filter(|u| u.definite).count();

                                        if definite_count > committed_count {
                                            // 4B: push_str loop instead of collect+join
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
                                                };
                                                // 4E: Direct typed emit, no double serialization
                                                let _ = app_reader
                                                    .emit("transcription-update", &update);
                                            }
                                            committed_count = definite_count;
                                        }

                                        // 4B: push_str loop for interim
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
                                            };
                                            let _ =
                                                app_reader.emit("transcription-update", &update);
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
                                            };
                                            let _ =
                                                app_reader.emit("transcription-update", &update);
                                        }
                                    }
                                }
                            } else {
                                net_log(
                                    &app_reader,
                                    "warn",
                                    &format!(
                                        "← seq={} 无法解析 JSON: {}",
                                        sequence,
                                        &payload[..payload.len().min(100)]
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
    // This includes ~1s of audio before speech was detected, ensuring the
    // beginning of the utterance isn't clipped.
    let mut total_audio_bytes: usize = 0;
    let mut packet_count: u32 = 0;

    // Flush pre-buffer in chunks
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
    // Keep remainder in working buffer
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
                        // Use pre-computed level from AudioFrame (no redundant RMS)
                        let _ = app_handle.emit("audio-level", frame.level);

                        buffer.extend_from_slice(&frame.pcm);
                        while buffer.len() >= chunk_size {
                            // 4D: slice reference + drain, no collect allocation
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

fn truncate_for_log(s: &str, max_len: usize) -> Cow<'_, str> {
    if s.len() > max_len {
        let end = s.floor_char_boundary(max_len.saturating_sub(3));
        Cow::Owned(format!("{}...", &s[..end]))
    } else {
        Cow::Borrowed(s)
    }
}
