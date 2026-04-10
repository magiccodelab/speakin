//! 百炼语音识别 (DashScope ASR) provider.
//!
//! Supports Fun-ASR and Paraformer models via the DashScope duplex streaming protocol.
//! Protocol: JSON control messages + binary PCM audio frames.
//! Endpoint: wss://dashscope.aliyuncs.com/api-ws/v1/inference

use super::{
    classify_error, net_log, truncate_for_log, wait_for_speech, AsrProvider, SessionOutcome,
    TranscriptUpdate, WaitForSpeechResult,
};
use crate::audio::AudioFrame;
use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;
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

/// Messages the reader task sends back to the main loop.
enum ReaderMsg {
    Started,
    Final(String),
    Error(String),
    Done,
}

impl AsrProvider for DashScopeProvider {
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
                    &format!("已检测到有效语音，开始云端转写 [百炼，会话 {}]", generation),
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
            net_log(&app_handle, "error", &format!("DashScope 错误: {}", detail));
            outcome.error = Some((classify_error(&detail), detail));
            outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
            outcome.aborted = aborted.load(Ordering::Acquire);
            outcome.had_speech = had_speech.load(Ordering::Acquire);
            outcome
        };

        // Phase 2: WebSocket connection
        let url = match settings.region.as_str() {
            "singapore" => "wss://dashscope-intl.aliyuncs.com/api-ws/v1/inference",
            _ => "wss://dashscope.aliyuncs.com/api-ws/v1/inference",
        };

        let mut request = match url.into_client_request() {
            Ok(r) => r,
            Err(e) => return finalize_with_error(outcome, format!("请求构建失败: {}", e)),
        };
        match format!("Bearer {}", settings.api_key).parse() {
            Ok(v) => { request.headers_mut().insert("Authorization", v); }
            Err(e) => return finalize_with_error(outcome, format!("Header 格式错误: {}", e)),
        }

        net_log(&app_handle, "info", &format!("→ 连接 DashScope ASR ({})", url));
        let (ws_stream, _response) = match tokio_tungstenite::connect_async_tls_with_config(
            request, None, false, None,
        )
        .await
        {
            Ok(pair) => pair,
            Err(e) => return finalize_with_error(outcome, format!("WebSocket 连接失败: {}", e)),
        };

        let _ = app_handle.emit("connection-status", true);
        crate::emit_app_log(&app_handle, "info", "云端转写已连接 [百炼]");
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
        if let Err(e) = ws_write.send(Message::text(run_task_str)).await {
            return finalize_with_error(outcome, format!("发送 run-task 失败: {}", e));
        }

        // Phase 4: Start reader task — sends ReaderMsg back via channel
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

                        let event = json["header"]["event"].as_str().unwrap_or("");

                        match event {
                            "task-started" => {
                                net_log(&app_reader, "info", "← task-started");
                                let _ = reader_tx_clone.send(ReaderMsg::Started);
                            }
                            "result-generated" => {
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
                                    if sentence_end {
                                        let _ = reader_tx_clone
                                            .send(ReaderMsg::Final(text.to_string()));
                                    }
                                }
                            }
                            "task-finished" => {
                                net_log(&app_reader, "info", "← task-finished");
                                let _ = reader_tx_clone.send(ReaderMsg::Done);
                                break;
                            }
                            "task-failed" => {
                                let code = json["header"]["status_code"].as_i64().unwrap_or(0);
                                let message = json["header"]["status_message"]
                                    .as_str()
                                    .unwrap_or("未知错误");
                                let err_msg =
                                    format!("DashScope ASR 错误 {}: {}", code, message);
                                net_log(&app_reader, "error", &err_msg);
                                let _ = reader_tx_clone.send(ReaderMsg::Error(err_msg));
                                break;
                            }
                            _ => {}
                        }
                    }
                    Ok(Message::Close(_)) => break,
                    Err(e) => {
                        let err = format!("DashScope 连接错误: {}", e);
                        net_log(&app_reader, "error", &err);
                        let _ = reader_tx_clone.send(ReaderMsg::Error(err));
                        break;
                    }
                    _ => {}
                }
            }
            // Unblock main task if it's still waiting.
            let _ = reader_tx_clone.send(ReaderMsg::Done);
        });
        drop(reader_tx);

        // Wait for task-started before sending audio (5s timeout)
        let started = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while let Some(msg) = reader_rx.recv().await {
                match msg {
                    ReaderMsg::Started => return Ok(()),
                    ReaderMsg::Error(detail) => return Err(detail),
                    ReaderMsg::Final(text) => outcome.finals.push(text),
                    ReaderMsg::Done => return Err("reader ended before task-started".to_string()),
                }
            }
            Err("reader channel closed".to_string())
        })
        .await;
        match started {
            Ok(Ok(())) => {}
            Ok(Err(detail)) => {
                reader_task.abort();
                return finalize_with_error(outcome, detail);
            }
            Err(_) => {
                reader_task.abort();
                return finalize_with_error(outcome, "等待 task-started 超时".to_string());
            }
        }

        // Phase 5: Send pre-buffered audio + streaming audio
        let chunk_size: usize = 6400; // 200ms @ 16kHz 16-bit mono
        let mut pos = 0;
        let mut packet_count: u32 = 0;
        let mut total_audio_bytes: usize = 0;

        while pos + chunk_size <= pre_buffer.len() {
            let chunk = &pre_buffer[pos..pos + chunk_size];
            if let Err(e) = ws_write.send(Message::Binary(chunk.to_vec().into())).await {
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

        // Stream live audio. Also drain reader messages to accumulate
        // finals in real-time, and watch for reader errors to bail early.
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
                        Some(ReaderMsg::Started) => {} // shouldn't happen post-start, ignore
                        None => break,
                    }
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
                                    error_opt = Some("音频发送失败".to_string());
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
                let _ = ws_write.send(Message::Binary(buffer.clone().into())).await;
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
                "payload": { "input": {} }
            });
            let _ = ws_write.send(Message::text(finish_task.to_string())).await;
            net_log(&app_handle, "info", "→ finish-task");

            // Wait for task-finished (5s cap)
            let wait_result = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                async {
                    while let Some(msg) = reader_rx.recv().await {
                        match msg {
                            ReaderMsg::Final(text) => outcome.finals.push(text),
                            ReaderMsg::Error(detail) => return Some(detail),
                            ReaderMsg::Done => return None,
                            ReaderMsg::Started => {}
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
                    net_log(&app_handle, "warn", "等待 task-finished 超时 (5s)，使用已有累积结果");
                }
            }
        }

        let _ = tokio::time::timeout(std::time::Duration::from_secs(1), reader_task).await;

        crate::emit_app_log(&app_handle, "info", "云端转写已结束 [百炼]");
        net_log(&app_handle, "info", "DashScope ASR 会话结束");

        outcome.had_speech = had_speech.load(Ordering::Acquire);
        outcome.aborted = aborted.load(Ordering::Acquire);
        outcome.duration_ms = session_started_at.elapsed().as_millis() as u64;
        if let Some(detail) = error_opt {
            outcome.error = Some((classify_error(&detail), detail));
        }
        outcome
    }
}
