//! 百炼语音识别 (DashScope ASR) provider.
//!
//! Supports Fun-ASR and Paraformer models via the DashScope duplex streaming protocol.
//! Protocol: JSON control messages + binary PCM audio frames.
//! Endpoint: wss://dashscope.aliyuncs.com/api-ws/v1/inference

use super::{net_log, truncate_for_log, wait_for_speech, AsrProvider, TranscriptUpdate, WaitForSpeechResult};
use crate::audio::AudioFrame;
use futures_util::{SinkExt, StreamExt};
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, oneshot};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

/// DashScope-specific settings.
pub struct DashScopeSettings {
    pub api_key: String,
    pub model: String,
    pub region: String,
}

pub struct DashScopeProvider {
    pub settings: DashScopeSettings,
}

impl AsrProvider for DashScopeProvider {
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
        let url = match settings.region.as_str() {
            "singapore" => "wss://dashscope-intl.aliyuncs.com/api-ws/v1/inference",
            _ => "wss://dashscope.aliyuncs.com/api-ws/v1/inference",
        };

        let mut request = url.into_client_request().map_err(|e| format!("请求构建失败: {}", e))?;
        request.headers_mut().insert(
            "Authorization",
            format!("Bearer {}", settings.api_key)
                .parse()
                .map_err(|e| format!("Header 格式错误: {}", e))?,
        );

        net_log(&app_handle, "info", &format!("→ 连接 DashScope ASR ({})", url));
        let (ws_stream, _response) =
            tokio_tungstenite::connect_async_tls_with_config(request, None, false, None)
                .await
                .map_err(|e| format!("WebSocket 连接失败: {}", e))?;

        let _ = app_handle.emit("connection-status", true);
        net_log(&app_handle, "info", "← WebSocket 已连接");

        let (mut ws_write, mut ws_read) = ws_stream.split();

        // Phase 3: Send run-task
        let task_id = uuid::Uuid::new_v4().to_string();
        let run_task = serde_json::json!({
            "header": {
                "action": "run-task",
                "task_id": &task_id,
                "streaming": "duplex"
            },
            "payload": {
                "task_group": "audio",
                "task": "asr",
                "function": "recognition",
                "model": &settings.model,
                "parameters": {
                    "format": "pcm",
                    "sample_rate": 16000
                },
                "input": {}
            }
        });

        let run_task_str = run_task.to_string();
        net_log(
            &app_handle,
            "info",
            &format!("→ run-task (model={})", settings.model),
        );
        ws_write
            .send(Message::text(run_task_str))
            .await
            .map_err(|e| format!("发送 run-task 失败: {}", e))?;

        // Phase 4: Start reader task
        let (task_started_tx, task_started_rx) = oneshot::channel::<()>();
        let (last_response_tx, last_response_rx) = oneshot::channel::<()>();
        let app_reader = app_handle.clone();

        let reader_task = tauri::async_runtime::spawn(async move {
            let mut task_started_tx = Some(task_started_tx);
            let mut last_response_tx = Some(last_response_tx);

            while let Some(msg) = ws_read.next().await {
                match msg {
                    Ok(Message::Text(text)) => {
                        let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else {
                            continue;
                        };

                        let event = json["header"]["event"]
                            .as_str()
                            .unwrap_or("");

                        match event {
                            "task-started" => {
                                net_log(&app_reader, "info", "← task-started");
                                if let Some(tx) = task_started_tx.take() {
                                    let _ = tx.send(());
                                }
                            }
                            "result-generated" => {
                                // Skip heartbeat messages
                                if json["payload"]["output"]["sentence"]["heartbeat"]
                                    .as_bool()
                                    .unwrap_or(false)
                                {
                                    continue;
                                }

                                let sentence = &json["payload"]["output"]["sentence"];
                                let text = sentence["text"].as_str().unwrap_or("");
                                let sentence_end = sentence["sentence_end"]
                                    .as_bool()
                                    .unwrap_or(false);

                                if !text.is_empty() {
                                    let update = TranscriptUpdate {
                                        text: text.to_string(),
                                        is_final: sentence_end,
                                        generation,
                                    };
                                    let _ = app_reader.emit("transcription-update", &update);
                                    net_log(
                                        &app_reader,
                                        "info",
                                        &format!(
                                            "← {} \"{}\"",
                                            if sentence_end { "[FINAL]" } else { "[interim]" },
                                            truncate_for_log(text, 60)
                                        ),
                                    );
                                }
                            }
                            "task-finished" => {
                                net_log(&app_reader, "info", "← task-finished");
                                if let Some(tx) = last_response_tx.take() {
                                    let _ = tx.send(());
                                }
                                break;
                            }
                            "task-failed" => {
                                let code = json["header"]["status_code"]
                                    .as_i64()
                                    .unwrap_or(0);
                                let message = json["header"]["status_message"]
                                    .as_str()
                                    .unwrap_or("未知错误");
                                let err_msg =
                                    format!("DashScope ASR 错误 {}: {}", code, message);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = app_reader.emit("asr-error", &err_msg);
                                break;
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        let err = format!("DashScope 连接错误: {}", e);
                        net_log(&app_reader, "error", &err);
                        let _ = app_reader.emit("asr-error", &err);
                        break;
                    }
                    _ => {}
                }
            }

            // Signal last_response waiter on exit (prevents 10s hang).
            // NOTE: Do NOT signal task_started_tx here — if it wasn't sent during
            // normal operation, the 5s timeout will produce a clear error message.
            if let Some(tx) = last_response_tx.take() {
                let _ = tx.send(());
            }
        });

        // Wait for task-started before sending audio (5s timeout)
        if tokio::time::timeout(std::time::Duration::from_secs(5), task_started_rx)
            .await
            .is_err()
        {
            let _ = app_handle.emit("connection-status", false);
            return Err("等待 task-started 超时".to_string());
        }

        // Phase 5: Send pre-buffered audio + streaming audio
        let chunk_size: usize = 6400; // 200ms @ 16kHz 16-bit mono

        // Send pre-buffer in chunks
        let mut pos = 0;
        let mut packet_count: u32 = 0;
        let mut total_audio_bytes: usize = 0;

        while pos + chunk_size <= pre_buffer.len() {
            let chunk = &pre_buffer[pos..pos + chunk_size];
            ws_write
                .send(Message::Binary(chunk.to_vec().into()))
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
                                if ws_write
                                    .send(Message::Binary(buffer[..chunk_size].to_vec().into()))
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

                                if packet_count % 25 == 1 {
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
            let _ = ws_write
                .send(Message::Binary(buffer.clone().into()))
                .await;
            net_log(
                &app_handle,
                "info",
                &format!("→ 发送剩余缓冲 ({} bytes)", buffer.len()),
            );
        }

        // Phase 6: Send finish-task
        let finish_task = serde_json::json!({
            "header": {
                "action": "finish-task",
                "task_id": &task_id,
                "streaming": "duplex"
            },
            "payload": {
                "input": {}
            }
        });
        let _ = ws_write
            .send(Message::text(finish_task.to_string()))
            .await;
        net_log(&app_handle, "info", "→ finish-task");

        // Wait for task-finished (10s timeout)
        let _ =
            tokio::time::timeout(std::time::Duration::from_secs(10), last_response_rx).await;

        // Wait for reader task (2s timeout)
        let _ = tokio::time::timeout(std::time::Duration::from_secs(2), reader_task).await;

        let _ = app_handle.emit("connection-status", false);
        net_log(&app_handle, "info", "DashScope ASR 会话结束");

        Ok(())
    }
}
