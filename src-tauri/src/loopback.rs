//! WASAPI Loopback capture — records audio from system output devices.
//!
//! Uses Windows Audio Session API (WASAPI) in loopback mode to capture
//! audio playing through an output device (speakers/headphones). The
//! captured audio is converted to 16kHz mono PCM and sent as `AudioFrame`
//! through the same channel used by the microphone path.

use crate::audio::{rms_to_level, AudioFrame, AudioResampler, Vad};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use tokio::sync::mpsc;
use windows::core::GUID;
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;

/// Manages a WASAPI loopback capture session on a dedicated thread.
pub struct LoopbackCapture {
    stop_flag: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl LoopbackCapture {
    /// Start capturing audio from the default render (output) device.
    /// Audio is resampled to 16kHz mono and sent as `AudioFrame`.
    /// `vad_sensitivity` controls the VAD threshold (1-10, default 7).
    pub fn start(audio_tx: mpsc::UnboundedSender<AudioFrame>, vad_sensitivity: u8) -> Result<Self, String> {
        let stop_flag = Arc::new(AtomicBool::new(false));
        let flag = stop_flag.clone();
        let threshold = crate::audio::sensitivity_to_rms(vad_sensitivity);

        let thread = std::thread::Builder::new()
            .name("loopback-capture".into())
            .spawn(move || {
                if let Err(e) = capture_loop(flag, audio_tx, threshold) {
                    log::error!("Loopback capture error: {}", e);
                }
            })
            .map_err(|e| format!("Failed to spawn loopback thread: {}", e))?;

        Ok(Self {
            stop_flag,
            thread: Some(thread),
        })
    }

    /// Signal the capture thread to stop and wait for it to finish.
    pub fn stop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for LoopbackCapture {
    fn drop(&mut self) {
        self.stop();
    }
}


/// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID = GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

const WAVE_FORMAT_EXTENSIBLE_TAG: u16 = 0xFFFE;
const WAVE_FORMAT_IEEE_FLOAT_TAG: u16 = 0x0003;

/// Main capture loop — runs on the loopback thread.
fn capture_loop(
    stop_flag: Arc<AtomicBool>,
    audio_tx: mpsc::UnboundedSender<AudioFrame>,
    vad_threshold: f64,
) -> Result<(), String> {
    unsafe {
        // CoInitializeEx: Ok(()) = success, Err with S_FALSE = already initialized (fine)
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // Get default render device
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("Failed to create device enumerator: {}", e))?;

        let device: IMMDevice = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("No default audio output device: {}", e))?;

        // Activate IAudioClient
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("Failed to activate audio client: {}", e))?;

        // Get mix format
        let mix_format_ptr = client
            .GetMixFormat()
            .map_err(|e| format!("GetMixFormat failed: {}", e))?;

        let sample_rate = (*mix_format_ptr).nSamplesPerSec;
        let channels = (*mix_format_ptr).nChannels;
        let bits_per_sample = (*mix_format_ptr).wBitsPerSample;

        // Detect if format is float
        let is_float = if (*mix_format_ptr).wFormatTag == WAVE_FORMAT_EXTENSIBLE_TAG {
            let ext = mix_format_ptr as *const WAVEFORMATEXTENSIBLE;
            let sub = std::ptr::addr_of!((*ext).SubFormat).read_unaligned();
            sub == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
        } else {
            (*mix_format_ptr).wFormatTag == WAVE_FORMAT_IEEE_FLOAT_TAG
        };

        log::info!(
            "[loopback] mix format: {}Hz {}ch {}bit float={}",
            sample_rate,
            channels,
            bits_per_sample,
            is_float
        );

        // Initialize in loopback mode
        let buffer_duration = 200_000i64; // 20ms in 100ns units
        let init_result = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            buffer_duration,
            0,
            mix_format_ptr,
            None,
        );

        // Free mix format BEFORE checking result (avoid leak on error)
        CoTaskMemFree(Some(mix_format_ptr as *const _ as *const std::ffi::c_void));
        init_result.map_err(|e| format!("IAudioClient::Initialize failed: {}", e))?;

        // Get capture client
        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| format!("GetService(IAudioCaptureClient) failed: {}", e))?;

        // Start the stream
        client
            .Start()
            .map_err(|e| format!("IAudioClient::Start failed: {}", e))?;

        // Processing state
        let mut resampler = AudioResampler::new(sample_rate, 16000)?;
        let mut vad = Vad::with_threshold(vad_threshold);

        log::info!("[loopback] capture started");

        // Capture loop
        loop {
            if stop_flag.load(Ordering::Relaxed) {
                break;
            }

            let packet_size = match capture.GetNextPacketSize() {
                Ok(s) => s,
                Err(_) => break,
            };

            if packet_size == 0 {
                std::thread::sleep(std::time::Duration::from_millis(5));
                continue;
            }

            let mut buffer: *mut u8 = std::ptr::null_mut();
            let mut num_frames: u32 = 0;
            let mut flags: u32 = 0;
            let hr = capture.GetBuffer(
                &mut buffer,
                &mut num_frames,
                &mut flags,
                None,
                None,
            );
            if hr.is_err() {
                break;
            }

            if num_frames > 0 {
                let silent = flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0;

                // Convert buffer to i16 mono samples
                let mono_samples: Vec<i16> = if silent {
                    vec![0i16; num_frames as usize]
                } else if is_float && bits_per_sample == 32 {
                    // Float32 → i16
                    let float_count = num_frames as usize * channels as usize;
                    let floats = std::slice::from_raw_parts(buffer as *const f32, float_count);
                    let i16_samples: Vec<i16> = floats
                        .iter()
                        .map(|&f| (f * 32767.0).round().clamp(-32768.0, 32767.0) as i16)
                        .collect();
                    crate::audio::to_mono(&i16_samples, channels).into_owned()
                } else if bits_per_sample == 16 {
                    // i16 directly
                    let sample_count = num_frames as usize * channels as usize;
                    let samples = std::slice::from_raw_parts(buffer as *const i16, sample_count);
                    crate::audio::to_mono(samples, channels).into_owned()
                } else {
                    // Unsupported format — skip
                    let _ = capture.ReleaseBuffer(num_frames);
                    continue;
                };

                let _ = capture.ReleaseBuffer(num_frames);

                // VAD filters silence to reduce unnecessary data sent to ASR.
                // No auto-stop for loopback — user controls stop manually.
                let (should_send, rms) = vad.process(&mono_samples);

                if should_send {
                    let resampled = resampler.process(&mono_samples);
                    let pcm: Vec<u8> = resampled
                        .iter()
                        .flat_map(|&s| s.to_le_bytes())
                        .collect();

                    let level = rms_to_level(rms);
                    let frame = AudioFrame {
                        pcm,
                        level,
                        rms: rms as f32,
                        has_speech: rms >= vad_threshold,
                    };

                    if audio_tx.send(frame).is_err() {
                        break;
                    }
                }
            } else {
                let _ = capture.ReleaseBuffer(num_frames);
            }
        }

        // Stop the stream
        let _ = client.Stop();
        log::info!("[loopback] capture stopped");

        Ok(())
    }
}
