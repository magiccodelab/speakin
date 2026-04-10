use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rubato::{FftFixedIn, Resampler as _};
use std::borrow::Cow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;

// ── Resampler (rubato-backed) ──

/// Wraps rubato's FFT-based resampler for real-time 16-bit mono audio.
/// Maintains a leftover buffer to handle cpal callbacks that deliver fewer
/// frames than the resampler's required chunk size.
pub(crate) struct AudioResampler {
    resampler: FftFixedIn<f32>,
    needs_resample: bool,
    leftover: Vec<f32>,
    chunk_buf: Vec<f32>,
    output_buf: Vec<i16>,
}

impl AudioResampler {
    pub(crate) fn new(from_rate: u32, to_rate: u32) -> Result<Self, String> {
        let needs_resample = from_rate != to_rate;
        // chunk_size: number of input frames per process call.
        // 480 frames ≈ 10ms at 48kHz, matches typical WASAPI buffer sizes.
        let chunk_size = 480;
        let resampler = FftFixedIn::<f32>::new(
            from_rate as usize,
            to_rate as usize,
            chunk_size,
            1, // sub_chunks (1 = no sub-chunking)
            1, // mono channel
        )
        .map_err(|e| format!("创建重采样器失败 ({}Hz → {}Hz): {}", from_rate, to_rate, e))?;

        Ok(Self {
            resampler,
            needs_resample,
            leftover: Vec::new(),
            chunk_buf: Vec::with_capacity(chunk_size),
            output_buf: Vec::with_capacity(chunk_size),
        })
    }

    pub(crate) fn process<'a>(&mut self, input: &'a [i16]) -> Cow<'a, [i16]> {
        if !self.needs_resample {
            return Cow::Borrowed(input);
        }

        // Convert i16 → f32 normalized [-1.0, 1.0] directly into leftover
        self.leftover
            .extend(input.iter().map(|&s| s as f32 / 32768.0));

        let chunk_size = self.resampler.input_frames_next();
        self.output_buf.clear();

        // Process complete chunks from the accumulated buffer
        while self.leftover.len() >= chunk_size {
            // Copy chunk into reusable buffer, then drain leftover
            self.chunk_buf.clear();
            self.chunk_buf
                .extend_from_slice(&self.leftover[..chunk_size]);
            self.leftover.drain(..chunk_size);

            if let Ok(result) = self.resampler.process(&[&self.chunk_buf], None) {
                for &sample in &result[0] {
                    self.output_buf
                        .push((sample * 32767.0).round().clamp(-32768.0, 32767.0) as i16);
                }
            }
        }
        // Remaining samples stay in self.leftover for the next call

        // Clone data for caller, then clear (preserves capacity for next call).
        // One allocation per callback (~640 bytes), but output_buf retains its heap
        // buffer so push() in the next call never re-allocates.
        let result = self.output_buf.clone();
        self.output_buf.clear();
        Cow::Owned(result)
    }
}

pub(crate) fn to_mono(data: &[i16], channels: u16) -> Cow<'_, [i16]> {
    if channels <= 1 {
        return Cow::Borrowed(data);
    }
    let ch = channels as usize;
    Cow::Owned(
        data.chunks(ch)
            .map(|frame| {
                let sum: i32 = frame.iter().map(|&s| s as i32).sum();
                (sum / ch as i32) as i16
            })
            .collect(),
    )
}

// ── Device Utilities ──

/// List all available audio input devices.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let mut devices = Vec::new();

    if let Some(device) = host.default_input_device() {
        if let Ok(name) = device.name() {
            devices.push(format!("{} (默认)", name));
        }
    }

    if let Ok(input_devices) = host.input_devices() {
        for device in input_devices {
            if let Ok(name) = device.name() {
                if !devices.iter().any(|d: &String| d.starts_with(&name)) {
                    devices.push(name);
                }
            }
        }
    }

    devices
}

fn find_device(device_name: Option<&str>) -> Result<cpal::Device, String> {
    let host = cpal::default_host();

    if let Some(name) = device_name {
        if !name.is_empty() {
            let clean_name = name.replace(" (默认)", "");
            if let Ok(devices) = host.input_devices() {
                for device in devices {
                    if let Ok(dev_name) = device.name() {
                        if dev_name == clean_name {
                            return Ok(device);
                        }
                    }
                }
            }
            log::warn!("Device '{}' not found, using default", name);
        }
    }

    host.default_input_device().ok_or_else(|| {
        "未找到麦克风设备。请检查 Windows 设置 > 隐私和安全 > 麦克风 是否已启用。".to_string()
    })
}

// ── VAD (Voice Activity Detection) ──

/// Simple energy-based voice activity detector.
/// Suppresses silent audio to avoid ASR server timeouts.
pub(crate) const VAD_RMS_THRESHOLD: f64 = 150.0;
/// Require several consecutive speech-like chunks before promoting the session
/// into the ASR path. This filters hotkey clicks and other single-frame spikes.
pub(crate) const SPEECH_START_MIN_HITS: u32 = 3;
/// Ignore speech-start promotion during the first ~80ms after recording begins.
/// We still pre-buffer audio during this window; we only suppress the
/// session-level "speech started" transition.
pub(crate) const HOTKEY_NOISE_GUARD_MS: u64 = 80;

/// Map a user-facing sensitivity level (1–10) to an RMS threshold (0–32767 scale).
/// Higher sensitivity = lower threshold = more audio passes through.
pub(crate) fn sensitivity_to_rms(sensitivity: u8) -> f64 {
    match sensitivity.clamp(1, 10) {
        10 => 50.0,   // ~-56 dBFS — extremely sensitive
        9  => 80.0,   // ~-52 dBFS
        8  => 110.0,  // ~-49 dBFS
        7  => 150.0,  // ~-47 dBFS — default
        6  => 220.0,  // ~-43 dBFS
        5  => 400.0,  // ~-38 dBFS — light noise (fan, AC)
        4  => 650.0,  // ~-34 dBFS
        3  => 1000.0, // ~-30 dBFS — moderate noise (café)
        2  => 1600.0, // ~-26 dBFS
        _  => 2500.0, // ~-22 dBFS — very noisy, only loud speech
    }
}

pub(crate) struct Vad {
    /// RMS threshold below which audio is considered silence (0–32767 scale).
    threshold: f64,
    /// Number of consecutive silent chunks observed.
    silent_chunks: u32,
    /// Whether we've detected any speech at all in this session.
    pub(crate) speech_detected: bool,
    /// Number of trailing chunks to keep sending after speech stops (avoids clipping).
    trailing_allowance: u32,
    trailing_remaining: u32,
}

impl Vad {
    pub(crate) fn with_threshold(threshold: f64) -> Self {
        Self {
            threshold,
            silent_chunks: 0,
            speech_detected: false,
            trailing_allowance: 10, // ~10 chunks × ~20ms = ~200ms trailing audio
            trailing_remaining: 0,
        }
    }

    /// Returns (should_send, rms) — RMS is returned to avoid redundant computation downstream.
    pub(crate) fn process(&mut self, samples: &[i16]) -> (bool, f64) {
        let rms = if samples.is_empty() {
            0.0
        } else {
            let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (sum_sq / samples.len() as f64).sqrt()
        };

        let is_speech = rms >= self.threshold;

        if is_speech {
            self.speech_detected = true;
            self.silent_chunks = 0;
            self.trailing_remaining = self.trailing_allowance;
            (true, rms)
        } else {
            self.silent_chunks += 1;

            // Send trailing audio after speech ends to avoid clipping
            if self.trailing_remaining > 0 {
                self.trailing_remaining -= 1;
                return (true, rms);
            }

            // Before any speech is detected, let some audio through
            // so ASR can start processing (first 1 second)
            if !self.speech_detected && self.silent_chunks < 50 {
                return (true, rms);
            }

            (false, rms)
        }
    }

    /// Seconds of continuous silence.
    fn silence_duration_secs(&self) -> f32 {
        // Each chunk is roughly 20ms from cpal callback at typical buffer sizes
        self.silent_chunks as f32 * 0.02
    }
}

// ── AudioFrame: pre-computed audio data ──

/// A processed audio frame with pre-computed metadata.
/// RMS/level/speech are computed once in the capture stage,
/// avoiding redundant computation downstream in ASR.
#[derive(Debug, Clone)]
pub struct AudioFrame {
    /// 16kHz mono PCM bytes (little-endian i16)
    pub pcm: Vec<u8>,
    /// Normalized RMS level (0.0..1.0), for UI visualization
    pub level: f32,
    /// Raw RMS value on the 16-bit PCM scale, for speech-start decisions/logging.
    pub rms: f32,
    /// Whether this frame contains speech (RMS >= threshold)
    pub has_speech: bool,
}

/// Compute normalized RMS level (0.0..1.0) from raw RMS value.
/// Uses aggressive amplification (16x) + sqrt compression so normal
/// speech at typical mic distances produces visible waveform activity.
pub(crate) fn rms_to_level(rms: f64) -> f32 {
    let linear = (rms / 32767.0) as f32;
    (linear * 16.0).min(1.0).sqrt()
}

/// Convert raw RMS / peak amplitude (16-bit scale) to dBFS.
/// Returns -100.0 dBFS as a floor for near-silent input.
pub(crate) fn amp_to_dbfs(amp: f64) -> f32 {
    if amp < 1.0 {
        return -100.0;
    }
    (20.0 * (amp / 32768.0).log10()) as f32
}

/// Result of a one-shot microphone level measurement.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LevelStats {
    /// Average RMS on the 16-bit PCM scale (0..32767).
    pub avg_rms: f32,
    /// Peak absolute sample on the 16-bit PCM scale (0..32767).
    pub peak_amp: f32,
    /// Average RMS expressed in dBFS (negative; 0 dBFS = full scale).
    pub avg_dbfs: f32,
    /// Peak amplitude expressed in dBFS (negative; 0 dBFS = full scale).
    pub peak_dbfs: f32,
    /// Sample rate of the captured device (Hz).
    pub sample_rate: u32,
    /// Number of samples accumulated during the measurement window.
    pub sample_count: u64,
}

/// Open a temporary cpal input stream on the given device, capture audio for
/// `duration_ms`, then return aggregated level statistics.
///
/// This is a blocking call (uses thread::sleep) and must be invoked from a
/// blocking context (e.g. tokio::task::spawn_blocking). It does NOT interfere
/// with `MicrophoneManager` — on Windows WASAPI shared mode multiple capture
/// clients can coexist on the same device.
pub fn measure_input_level(
    device_name: Option<&str>,
    duration_ms: u64,
) -> Result<LevelStats, String> {
    let device = find_device(device_name)?;
    let default_config = device
        .default_input_config()
        .map_err(|e| format!("无法获取麦克风配置: {}. 请检查麦克风权限。", e))?;

    let sample_rate = default_config.sample_rate().0;
    let channels = default_config.channels();
    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    // (sum_sq, count, peak_abs) — locked from cpal callback thread.
    let stats = Arc::new(parking_lot::Mutex::new((0.0_f64, 0u64, 0i32)));

    fn accumulate(stats: &parking_lot::Mutex<(f64, u64, i32)>, mono: &[i16]) {
        let mut s = stats.lock();
        for &sample in mono {
            let v = sample as f64;
            s.0 += v * v;
            s.1 += 1;
            let abs = (sample as i32).abs();
            if abs > s.2 {
                s.2 = abs;
            }
        }
    }

    let ch_count = channels;
    let stream = match default_config.sample_format() {
        cpal::SampleFormat::I16 => {
            let stats = stats.clone();
            device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let mono = to_mono(data, ch_count);
                        accumulate(&stats, &mono);
                    },
                    |err| log::error!("Audio stream error (level test): {}", err),
                    None,
                )
                .map_err(|e| format!("无法启动音频流: {}", e))?
        }
        cpal::SampleFormat::F32 => {
            let stats = stats.clone();
            let mut buf: Vec<i16> = Vec::new();
            device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        buf.clear();
                        buf.extend(
                            data.iter()
                                .map(|&s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16),
                        );
                        let mono = to_mono(&buf, ch_count);
                        accumulate(&stats, &mono);
                    },
                    |err| log::error!("Audio stream error (level test): {}", err),
                    None,
                )
                .map_err(|e| format!("无法启动音频流: {}", e))?
        }
        format => return Err(format!("不支持的音频格式: {:?}", format)),
    };

    stream
        .play()
        .map_err(|e| format!("无法开始录音: {}", e))?;
    std::thread::sleep(std::time::Duration::from_millis(duration_ms));
    drop(stream);

    let s = stats.lock();
    if s.1 == 0 {
        return Err("未采集到任何音频样本，请检查麦克风是否工作正常".to_string());
    }
    let avg_rms = (s.0 / s.1 as f64).sqrt();
    let peak = s.2 as f64;

    Ok(LevelStats {
        avg_rms: avg_rms as f32,
        peak_amp: peak as f32,
        avg_dbfs: amp_to_dbfs(avg_rms),
        peak_dbfs: amp_to_dbfs(peak),
        sample_rate,
        sample_count: s.1,
    })
}

// ── MicrophoneManager: always-on audio stream ──

enum MicCommand {
    SwitchDevice(Option<String>),
    Shutdown,
}

/// Callback for silence auto-stop notification.
type SilenceCallback = Arc<dyn Fn() + Send + Sync>;

/// Shared VAD configuration that can be updated between sessions.
#[derive(Clone, Copy)]
struct VadConfig {
    threshold: f64,
    silence_auto_stop_secs: f32,
}

impl Default for VadConfig {
    fn default() -> Self {
        Self {
            threshold: VAD_RMS_THRESHOLD,
            silence_auto_stop_secs: DEFAULT_SILENCE_AUTO_STOP_SECS,
        }
    }
}

/// Keeps the cpal audio stream always active.
/// When recording, audio is forwarded to the ASR channel.
/// When idle, audio is silently discarded (zero latency on start).
pub struct MicrophoneManager {
    is_forwarding: Arc<AtomicBool>,
    vad_reset: Arc<AtomicBool>,
    audio_tx: Arc<parking_lot::Mutex<Option<mpsc::UnboundedSender<AudioFrame>>>>,
    silence_cb: Arc<parking_lot::Mutex<Option<SilenceCallback>>>,
    vad_config: Arc<parking_lot::Mutex<VadConfig>>,
    cmd_tx: std::sync::mpsc::Sender<MicCommand>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl MicrophoneManager {
    /// Open the microphone stream. Audio is captured but discarded until
    /// `start_forwarding()` is called.
    pub fn new(device_name: Option<&str>) -> Result<Self, String> {
        let is_forwarding = Arc::new(AtomicBool::new(false));
        let vad_reset = Arc::new(AtomicBool::new(false));
        let audio_tx: Arc<parking_lot::Mutex<Option<mpsc::UnboundedSender<AudioFrame>>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let silence_cb: Arc<parking_lot::Mutex<Option<SilenceCallback>>> =
            Arc::new(parking_lot::Mutex::new(None));
        let vad_config: Arc<parking_lot::Mutex<VadConfig>> =
            Arc::new(parking_lot::Mutex::new(VadConfig::default()));
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<MicCommand>();
        let (startup_tx, startup_rx) = std::sync::mpsc::channel::<Result<(), String>>();

        let fwd = is_forwarding.clone();
        let vr = vad_reset.clone();
        let tx = audio_tx.clone();
        let scb = silence_cb.clone();
        let vc = vad_config.clone();
        let initial_device = device_name.map(|s| s.to_string());

        let thread = std::thread::spawn(move || {
            mic_thread(fwd, vr, tx, scb, vc, cmd_rx, startup_tx, initial_device);
        });

        // Wait for initial stream to open
        match startup_rx.recv_timeout(std::time::Duration::from_secs(3)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return Err(e),
            Err(_) => return Err("麦克风初始化超时".to_string()),
        }

        Ok(Self {
            is_forwarding,
            vad_reset,
            audio_tx,
            silence_cb,
            vad_config,
            cmd_tx,
            thread: Some(thread),
        })
    }

    /// Update VAD configuration (threshold and auto-stop duration).
    /// Takes effect on the next `start_forwarding` call (VAD reset).
    pub fn set_vad_config(&self, sensitivity: u8, silence_auto_stop_secs: u8) {
        let sensitivity = sensitivity.clamp(1, 10);
        let silence_auto_stop_secs = silence_auto_stop_secs.clamp(3, 60);
        *self.vad_config.lock() = VadConfig {
            threshold: sensitivity_to_rms(sensitivity),
            silence_auto_stop_secs: silence_auto_stop_secs as f32,
        };
    }

    /// Begin forwarding audio to the given channel (zero latency).
    /// `on_silence` is called once when silence exceeds the auto-stop threshold.
    pub fn start_forwarding(
        &self,
        tx: mpsc::UnboundedSender<AudioFrame>,
        on_silence: Option<impl Fn() + Send + Sync + 'static>,
    ) {
        // Signal the callback to reset VAD on next invocation
        self.vad_reset.store(true, Ordering::Release);
        *self.audio_tx.lock() = Some(tx);
        *self.silence_cb.lock() = on_silence.map(|f| Arc::new(f) as SilenceCallback);
        self.is_forwarding.store(true, Ordering::Release);
    }

    /// Stop forwarding audio. The stream stays open.
    pub fn stop_forwarding(&self) {
        self.is_forwarding.store(false, Ordering::Release);
        *self.audio_tx.lock() = None;
        *self.silence_cb.lock() = None;
    }

    /// Switch to a different audio device (re-opens the stream).
    pub fn switch_device(&self, device_name: Option<&str>) {
        let _ = self
            .cmd_tx
            .send(MicCommand::SwitchDevice(device_name.map(|s| s.to_string())));
    }
}

impl Drop for MicrophoneManager {
    fn drop(&mut self) {
        let _ = self.cmd_tx.send(MicCommand::Shutdown);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Long-running thread that owns the cpal stream and handles device switching.
fn mic_thread(
    is_forwarding: Arc<AtomicBool>,
    vad_reset: Arc<AtomicBool>,
    audio_tx: Arc<parking_lot::Mutex<Option<mpsc::UnboundedSender<AudioFrame>>>>,
    silence_cb: Arc<parking_lot::Mutex<Option<SilenceCallback>>>,
    vad_config: Arc<parking_lot::Mutex<VadConfig>>,
    cmd_rx: std::sync::mpsc::Receiver<MicCommand>,
    startup_tx: std::sync::mpsc::Sender<Result<(), String>>,
    initial_device: Option<String>,
) {
    let mut stream = match open_stream(
        initial_device.as_deref(),
        &is_forwarding,
        &vad_reset,
        &audio_tx,
        &silence_cb,
        &vad_config,
    ) {
        Ok(s) => {
            let _ = startup_tx.send(Ok(()));
            s
        }
        Err(e) => {
            let _ = startup_tx.send(Err(e));
            return;
        }
    };

    loop {
        match cmd_rx.recv() {
            Ok(MicCommand::SwitchDevice(name)) => {
                log::info!("Switching mic device to: {:?}", name);
                is_forwarding.store(false, Ordering::Release);
                drop(stream);
                match open_stream(
                    name.as_deref(),
                    &is_forwarding,
                    &vad_reset,
                    &audio_tx,
                    &silence_cb,
                    &vad_config,
                ) {
                    Ok(s) => {
                        stream = s;
                        log::info!("Device switched successfully");
                    }
                    Err(e) => {
                        log::error!("Failed to switch device: {}", e);
                        match open_stream(None, &is_forwarding, &vad_reset, &audio_tx, &silence_cb, &vad_config)
                        {
                            Ok(s) => {
                                stream = s;
                                log::warn!("Fell back to default device");
                            }
                            Err(e2) => {
                                log::error!("Cannot open any audio device: {}", e2);
                                return;
                            }
                        }
                    }
                }
            }
            Ok(MicCommand::Shutdown) | Err(_) => {
                log::info!("Mic thread shutting down");
                drop(stream);
                return;
            }
        }
    }
}

/// Default max continuous silence (seconds) before triggering auto-stop.
const DEFAULT_SILENCE_AUTO_STOP_SECS: f32 = 6.0;

/// Open a cpal input stream. VAD and Resampler are created here and moved
/// into the callback closure — no Mutex needed since cpal guarantees
/// single-threaded callback invocation per stream.
fn open_stream(
    device_name: Option<&str>,
    is_forwarding: &Arc<AtomicBool>,
    vad_reset: &Arc<AtomicBool>,
    audio_tx: &Arc<parking_lot::Mutex<Option<mpsc::UnboundedSender<AudioFrame>>>>,
    silence_cb: &Arc<parking_lot::Mutex<Option<SilenceCallback>>>,
    vad_config: &Arc<parking_lot::Mutex<VadConfig>>,
) -> Result<cpal::Stream, String> {
    let device = find_device(device_name)?;
    let name = device.name().unwrap_or_else(|_| "unknown".to_string());
    log::info!("Opening mic stream: {}", name);

    let default_config = device
        .default_input_config()
        .map_err(|e| format!("无法获取麦克风配置: {}. 请检查麦克风权限。", e))?;

    let sample_rate = default_config.sample_rate().0;
    let channels = default_config.channels();
    log::info!(
        "Device config: {}Hz, {}ch, {:?}",
        sample_rate,
        channels,
        default_config.sample_format()
    );

    let stream_config = cpal::StreamConfig {
        channels,
        sample_rate: cpal::SampleRate(sample_rate),
        buffer_size: cpal::BufferSize::Default,
    };

    let ch_count = channels;

    // Build the common callback that owns VAD and Resampler (no Mutex needed).
    // This is a macro-like closure builder to avoid duplicating for I16/F32.
    let build_callback = |is_f32: bool| -> Result<_, String> {
        let fwd = is_forwarding.clone();
        let vr = vad_reset.clone();
        let tx = audio_tx.clone();
        let scb = silence_cb.clone();
        let vc = vad_config.clone();
        // VAD and Resampler owned by closure — lock-free in callback
        let initial_cfg = *vad_config.lock();
        let mut vad = Vad::with_threshold(initial_cfg.threshold);
        let mut resampler = AudioResampler::new(sample_rate, 16000)?;
        let mut f32_conv_buf: Vec<i16> = Vec::new();
        let mut auto_stop_fired = false;
        let mut auto_stop_limit = initial_cfg.silence_auto_stop_secs;
        let mut current_threshold = initial_cfg.threshold;

        Ok(move |data_i16: Option<&[i16]>, data_f32: Option<&[f32]>| {
            if !fwd.load(Ordering::Acquire) {
                return;
            }

            // Check VAD reset signal (set by start_forwarding).
            // Re-read VadConfig so that sensitivity/timeout changes take effect
            // even when mic_always_on keeps the stream open across sessions.
            if vr.swap(false, Ordering::AcqRel) {
                let cfg = *vc.lock();
                vad = Vad::with_threshold(cfg.threshold);
                auto_stop_limit = cfg.silence_auto_stop_secs;
                current_threshold = cfg.threshold;
                auto_stop_fired = false;
            }

            // Clone sender and release lock immediately — rest of callback is lock-free
            let sender = {
                let guard = tx.lock();
                match guard.as_ref() {
                    Some(s) => s.clone(),
                    None => return,
                }
            };

            // Get i16 data (convert from f32 if needed)
            let i16_data: &[i16] = if is_f32 {
                let f32_data = data_f32.unwrap();
                f32_conv_buf.clear();
                f32_conv_buf.extend(
                    f32_data
                        .iter()
                        .map(|&s| (s * 32767.0).round().clamp(-32768.0, 32767.0) as i16),
                );
                &f32_conv_buf
            } else {
                data_i16.unwrap()
            };

            let mono = to_mono(i16_data, ch_count);
            let (should_send, rms) = vad.process(&mono);
            let silence_secs = vad.silence_duration_secs();
            let speech_detected = vad.speech_detected;

            // Auto-stop after prolonged silence (fire only once per session)
            if speech_detected && silence_secs >= auto_stop_limit && !auto_stop_fired {
                auto_stop_fired = true;
                let cb_clone = scb.lock().as_ref().map(Arc::clone);
                if let Some(cb) = cb_clone {
                    log::info!(
                        "VAD: {:.1}s silence after speech, triggering auto-stop",
                        silence_secs
                    );
                    cb();
                }
                return;
            }

            if should_send {
                let resampled = resampler.process(&mono);
                let pcm_slice = resampled.as_ref();
                let mut bytes = Vec::with_capacity(pcm_slice.len() * 2);
                for &s in pcm_slice {
                    bytes.extend_from_slice(&s.to_le_bytes());
                }
                let has_speech = rms >= current_threshold;
                let level = rms_to_level(rms);
                let _ = sender.send(AudioFrame {
                    pcm: bytes,
                    level,
                    rms: rms as f32,
                    has_speech,
                });
            }
        })
    };

    let stream = match default_config.sample_format() {
        cpal::SampleFormat::I16 => {
            let mut callback = build_callback(false)?;
            device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| callback(Some(data), None),
                    |err| log::error!("Audio stream error: {}", err),
                    None,
                )
                .map_err(|e| format!("无法启动音频流: {}", e))?
        }
        cpal::SampleFormat::F32 => {
            let mut callback = build_callback(true)?;
            device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| callback(None, Some(data)),
                    |err| log::error!("Audio stream error: {}", err),
                    None,
                )
                .map_err(|e| format!("无法启动音频流: {}", e))?
        }
        format => return Err(format!("不支持的音频格式: {:?}", format)),
    };

    stream.play().map_err(|e| format!("无法开始录音: {}", e))?;
    log::info!(
        "Mic stream active: {}Hz {}ch → 16kHz mono",
        sample_rate,
        channels
    );
    Ok(stream)
}
