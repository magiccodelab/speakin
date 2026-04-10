import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen, emit } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AnimatePresence, motion } from "motion/react";
import { VoicePanel } from "./components/VoicePanel";
import { Settings } from "./components/Settings";
import { NetworkLog } from "./components/NetworkLog";
import { TitleBar } from "./components/TitleBar";
import { CloseDialog } from "./components/CloseDialog";
import { AboutDialog } from "./components/AboutDialog";
import { OnboardingDialog } from "./components/OnboardingDialog";
import { ThemeToggle } from "./components/ThemeToggle";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Settings as SettingsIcon, Copy, Trash2, Wifi, WifiOff } from "lucide-react";
import { cn } from "./lib/utils";
import { Tooltip } from "./components/ui/Tooltip";
import { playStartSound, playStopSound, playErrorSound } from "./lib/sounds";
import { buildHotkeyString, keyToHotkeyName } from "./lib/hotkey";
import { showOverlay, hideOverlay } from "./lib/overlay";
import { applyThemeColor } from "./lib/theme-colors";
import { RecentTranscripts } from "./components/RecentTranscripts";

export interface DoubaoProviderSettings {
  app_id: string;
  access_token: string;
  resource_id: string;
  asr_mode: string;
}

export interface DashScopeProviderSettings {
  api_key: string;
  model: string;
  region: string;
}

export interface QwenProviderSettings {
  api_key: string;
  model: string;
  region: string;
  language: string;
}

export interface AiOptimizeSettings {
  enabled: boolean;
  active_provider_id: string;
  active_prompt_id: string;
  connect_timeout_secs: number;
  max_request_secs: number;
}

export interface AppSettings {
  provider: string;
  doubao: DoubaoProviderSettings;
  dashscope: DashScopeProviderSettings;
  qwen: QwenProviderSettings;
  ai_optimize: AiOptimizeSettings;
  hotkey: string;
  input_mode: "toggle" | "hold";
  device_name: string;
  audio_source: "microphone" | "system";
  output_mode: "none" | "paste" | "type";
  mic_always_on: boolean;
  debug_mode: boolean;
  filler_enabled: boolean;
  replacement_enabled: boolean;
  replacement_ignore_case: boolean;
  theme_color: string;
  recording_follows_theme: boolean;
  show_overlay: boolean;
  show_overlay_subtitle: boolean;
  close_behavior: "ask" | "minimize" | "quit";
  onboarding_completed: boolean;
  copy_to_clipboard: boolean;
  paste_restore_clipboard: boolean;
  system_no_auto_stop: boolean;
  esc_abort_enabled: boolean;
  silence_auto_stop_secs: number;
  vad_sensitivity: number;
}

interface TranscriptPayload {
  text: string;
  is_final: boolean;
  generation: number;
}

interface RecordingStatusPayload {
  recording: boolean;
  generation: number;
  /**
   * For `recording: false` emits, true iff VAD observed any speech in
   * this session. Kept for legacy compat — the new session-ended event
   * is authoritative for end-of-session logic.
   */
  had_speech?: boolean;
}

/**
 * Single source of truth for "session is done" (2026-04 refactor).
 * Replaces the old asr-error + recording-status(false) + mark_session_idle
 * triangle. The backend owns session lifecycle now: it accumulates finals,
 * persists them on any exit path, and emits this payload exactly once.
 * Frontend just reacts — no gate to release, no rescue logic.
 */
interface SessionEndedPayload {
  generation: number;
  final_text: string;
  status: "ok" | "no_speech" | "error" | "aborted";
  error_reason?: string | null;
  error_detail?: string | null;
  duration_ms: number;
  record_id?: string | null;
}

export interface LogEntry {
  ts: string;
  level: string;
  msg: string;
}

interface BannerState {
  kind: "error" | "warning";
  text: string;
  link?: { text: string; url: string };
}

const DEFAULT_SETTINGS: AppSettings = {
  provider: "doubao",
  doubao: {
    app_id: "",
    access_token: "",
    resource_id: "volc.seedasr.sauc.duration",
    asr_mode: "bistream",
  },
  dashscope: {
    api_key: "",
    model: "paraformer-realtime-v2",
    region: "beijing",
  },
  qwen: {
    api_key: "",
    model: "qwen3-asr-flash-realtime",
    region: "beijing",
    language: "zh",
  },
  ai_optimize: {
    enabled: false,
    active_provider_id: "",
    active_prompt_id: "",
    connect_timeout_secs: 5,
    max_request_secs: 60,
  },
  hotkey: "Ctrl+Shift+V",
  input_mode: "toggle",
  device_name: "",
  audio_source: "microphone",
  output_mode: "type",
  mic_always_on: false,
  debug_mode: false,
  filler_enabled: true,
  replacement_enabled: false,
  replacement_ignore_case: false,
  theme_color: "blue",
  recording_follows_theme: true,
  show_overlay: true,
  show_overlay_subtitle: true,
  close_behavior: "ask",
  onboarding_completed: false,
  copy_to_clipboard: false,
  paste_restore_clipboard: true,
  system_no_auto_stop: false,
  esc_abort_enabled: true,
  silence_auto_stop_secs: 6,
  vad_sensitivity: 7,
};

/** Show "正在努力识别中" after the post-recording wait exceeds this. UX only. */
const SLOW_HINT_MS = 3000;
/** How long the red error waveform lingers before returning to idle. */
const ERROR_FLASH_MS = 2200;
const MAX_LOG_ENTRIES = 200;
const APP_LOG_PREFIX = "[APP]";
const AI_LOG_PREFIX = "[AI]";

function formatLogTimestamp(date = new Date()): string {
  const pad = (value: number, width = 2) => String(value).padStart(width, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())} ` +
    `${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}.${pad(date.getMilliseconds(), 3)}`;
}

function normalizeLogEntry(entry: LogEntry): LogEntry {
  if (/^\d{4}-\d{2}-\d{2} /.test(entry.ts)) {
    return entry;
  }
  return { ...entry, ts: `${formatLogTimestamp().slice(0, 10)} ${entry.ts}` };
}

function shouldKeepLogEntry(entry: LogEntry): boolean {
  const msg = entry.msg ?? "";
  return msg.startsWith(APP_LOG_PREFIX)
    || msg.startsWith(AI_LOG_PREFIX)
    || entry.level === "warn"
    || entry.level === "error";
}

export default function App() {
  const [isRecording, setIsRecording] = useState(false);
  const [transcript, setTranscript] = useState("");
  const [interimText, setInterimText] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [banner, setBanner] = useState<BannerState | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const isRecordingRef = useRef(false);
  const transcriptRef = useRef("");
  const transcriptScrollRef = useRef<HTMLDivElement>(null);
  // UX-only: when post-recording wait exceeds SLOW_HINT_MS, flip to
  // "正在努力识别中". Cleared when session-ended arrives.
  const slowHintTimerRef = useRef<number | null>(null);
  const [isProcessingSlow, setIsProcessingSlow] = useState(false);
  // Two distinct session counters:
  //   sessionIdRef       — frontend-owned, increments on each new recording.
  //                        Scopes AI optimize callbacks to the session that
  //                        started them.
  //   backendGenerationRef — shadow of backend's recording_generation,
  //                        authoritative for "which session is current".
  //                        Used to filter stale transcription-update events
  //                        and to guard late AI-optimize auto-paste from
  //                        typing into a newer session's focus target.
  const sessionIdRef = useRef(0);
  const backendGenerationRef = useRef(0);
  // Ref mirror of `interimText` state — event listeners read this from
  // closures. Must be kept in sync via `setInterimTextBoth`.
  const interimTextRef = useRef("");
  const [isPostRecording, setIsPostRecording] = useState(false);
  const [isOptimizing, setIsOptimizing] = useState(false);
  const [optimizedText, setOptimizedText] = useState("");
  const [closeDialogOpen, setCloseDialogOpen] = useState(false);
  const [aboutOpen, setAboutOpen] = useState(false);
  const [onboardingOpen, setOnboardingOpen] = useState(false);
  const [settingsInitialTab, setSettingsInitialTab] = useState<string | undefined>();
  const closingRef = useRef(false);
  const [hasError, setHasError] = useState(false);
  const errorTimerRef = useRef<number | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [transcriptRefreshKey, setTranscriptRefreshKey] = useState(0);
  const toastTimerRef = useRef<number | null>(null);
  // Pending settings: saved to backend immediately, but App state deferred
  // until panel closes to avoid re-render flash behind backdrop-blur.
  const pendingSettingsRef = useRef<AppSettings | null>(null);
  const settingsRef = useRef(settings);
  // Skip sync when a pending save hasn't propagated to React state yet,
  // otherwise a re-render between save and panel-close would clobber the
  // eagerly-written ref with stale React state.
  if (!pendingSettingsRef.current) {
    settingsRef.current = settings;
  }
  const appendLogEntry = useCallback((entry: LogEntry) => {
    if (!shouldKeepLogEntry(entry)) return;
    const normalized = normalizeLogEntry(entry);
    setLogs((prev) => [...prev.slice(-(MAX_LOG_ENTRIES - 1)), normalized]);
  }, []);
  const appendLocalLog = useCallback((level: LogEntry["level"], msg: string) => {
    appendLogEntry({
      ts: formatLogTimestamp(),
      level,
      msg: `${APP_LOG_PREFIX} ${msg}`,
    });
  }, [appendLogEntry]);
  const recordingStartTimeRef = useRef(0);
  // Set on ESC abort — guards AI optimize callbacks so we don't output
  // AI-optimized text from a session the user just cancelled. Reset on
  // every new session start.
  const forcedAbortRef = useRef(false);
  const aiLogs = logs.filter((log) => log.msg.startsWith(AI_LOG_PREFIX));
  const latestAiLogText = aiLogs.map((log) => `[${log.ts}] ${log.level.toUpperCase()} ${log.msg}`).join("\n\n");

  // Helper: update both the React state AND the ref mirror in lockstep.
  // ALL call sites that clear or update `interimText` must use this helper —
  // otherwise the ref goes stale and rescue/listener logic reads wrong values.
  const setInterimTextBoth = useCallback((value: string) => {
    interimTextRef.current = value;
    setInterimText(value);
  }, []);

  type OverlayPhase = "recording" | "processing" | "optimizing" | "error" | "idle";
  const emitOverlayPhase = useCallback((phase: OverlayPhase) => {
    emit("overlay-phase", { phase, sessionId: sessionIdRef.current });
  }, []);

  // Overlay window filters stale slow-hint emits by sessionId so a
  // late-arriving slow=true from a previous session can never override
  // a freshly reset state.
  const emitOverlaySlow = useCallback((slow: boolean) => {
    emit("overlay-slow", { slow, sessionId: sessionIdRef.current }).catch(() => {});
  }, []);

  const clearSlowHintTimer = useCallback(() => {
    if (slowHintTimerRef.current !== null) {
      window.clearTimeout(slowHintTimerRef.current);
      slowHintTimerRef.current = null;
    }
    setIsProcessingSlow(false);
    emitOverlaySlow(false);
  }, [emitOverlaySlow]);

  const closeOverlay = useCallback(() => {
    setIsPostRecording(false);
    hideOverlay();
  }, []);

  // showToast hoisted here (ahead of the listener useEffect below) so it's
  // in scope when the useEffect deps array evaluates. TS catches the
  // temporal dead zone even though runtime order would have worked.
  const showToast = useCallback((msg: string) => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    setToast(msg);
    toastTimerRef.current = window.setTimeout(() => setToast(null), 1500);
  }, []);

  // Safety timeout: auto-clear stuck processing states after 60s
  useEffect(() => {
    if (!isPostRecording && !isOptimizing) return;
    const timer = window.setTimeout(() => {
      setIsPostRecording(false);
      setIsOptimizing(false);
      emitOverlayPhase("idle");
      hideOverlay();
    }, 60_000);
    return () => clearTimeout(timer);
  }, [isPostRecording, isOptimizing, emitOverlayPhase]);

  const settingsLoadedRef = useRef(false);
  const [appReady, setAppReady] = useState(false);

  useEffect(() => {
    invoke<AppSettings>("get_settings")
      .then((s) => {
        if (s) {
          settingsLoadedRef.current = true;
          setSettings(s);
          if (!s.onboarding_completed) setOnboardingOpen(true);
        }
      })
      .catch(() => {})
      .finally(() => setAppReady(true));
  }, []);

  // Sync theme color when settings change (skip before backend settings are loaded
  // to avoid overwriting the flash-prevention script's applied color with defaults)
  useEffect(() => {
    if (!settingsLoadedRef.current) return;
    const isDark = document.documentElement.classList.contains("dark");
    applyThemeColor(settings.theme_color, isDark, settings.recording_follows_theme);
  }, [settings.theme_color, settings.recording_follows_theme]);

  // Sync overlay subtitle preference to localStorage for the overlay window to read
  useEffect(() => {
    localStorage.setItem("overlay-subtitle", settings.show_overlay_subtitle ? "1" : "0");
  }, [settings.show_overlay_subtitle]);

  useEffect(() => {
    const active = settings.esc_abort_enabled && (isRecording || isPostRecording || isOptimizing);
    invoke("set_escape_abort_active", { active }).catch(() => {});
    return () => {
      invoke("set_escape_abort_active", { active: false }).catch(() => {});
    };
  }, [settings.esc_abort_enabled, isRecording, isPostRecording, isOptimizing]);

  /**
   * AI optimize post-processing. Runs AFTER session-ended has delivered
   * the authoritative raw text + record_id, so the history record
   * already exists — our job is just:
   *   1. stream the optimized text into the UI
   *   2. update the record via update_transcript_optimized
   *   3. send the optimized text to the focused window, BUT only if
   *      this session is still the "current" one on the backend. If
   *      the user has already started a new session, skip the
   *      send_text_input to avoid typing old text into the new
   *      focus target (the record_update still happens — user can
   *      copy from history).
   */
  const runAiOptimize = useCallback(
    async (rawText: string, recordId: string | null, generation: number) => {
      const mySession = sessionIdRef.current;
      setIsOptimizing(true);
      setOptimizedText("");
      emitOverlayPhase("optimizing");
      setLogs((prev) => prev.filter((log) => !log.msg.startsWith(AI_LOG_PREFIX)));

      try {
        const optimized = await invoke<string>("ai_optimize_text", {
          text: rawText,
          sessionId: mySession,
        });
        if (forcedAbortRef.current) return;
        setOptimizedText(optimized);

        if (recordId) {
          invoke("update_transcript_optimized", { id: recordId, optimized })
            .then(() => setTranscriptRefreshKey((k) => k + 1))
            .catch(() => {});
        }

        // Only auto-paste if this is still the current backend session.
        // If the user already started a new session, silently drop the
        // auto-paste — the optimized text is still in the history record,
        // and typing it now would land in the wrong window.
        if (backendGenerationRef.current === generation && !forcedAbortRef.current) {
          invoke("send_text_input", { text: optimized }).catch(() => {});
        } else {
          appendLocalLog("info", "AI 优化完成但已有新会话开始，只更新历史记录");
        }
      } catch (e) {
        if (forcedAbortRef.current) return;
        playErrorSound();
        if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
        setHasError(true);
        errorTimerRef.current = window.setTimeout(() => {
          setHasError(false);
          errorTimerRef.current = null;
        }, 3000);
        setBanner({
          kind: "warning",
          text: settingsRef.current.debug_mode
            ? `AI 优化失败: ${e}，使用原始转写`
            : "AI 优化失败，使用原始转写",
        });
        appendLocalLog("warn", "AI 优化失败，已回退到原始转写");
        // Fall back to pasting raw text (same guard: only if still current session)
        if (backendGenerationRef.current === generation && !forcedAbortRef.current) {
          invoke("send_text_input", { text: rawText }).catch(() => {});
        }
      } finally {
        if (sessionIdRef.current === mySession) {
          setIsOptimizing(false);
          emitOverlayPhase("idle");
          closeOverlay();
        }
      }
    },
    [appendLocalLog, closeOverlay, emitOverlayPhase],
  );

  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    const listenerPromises = [
      // ── recording-cancelled ───────────────────────────────────────
      // Fired when the user mistouches a hold-mode hotkey (<300ms) or
      // presses ESC while recording. Just a UI hint — the authoritative
      // end-of-session signal is still `session-ended`. We use this to
      // hide the overlay a bit sooner on mistouch without waiting for
      // the backend's wrap-up.
      listen<string>("recording-cancelled", (e) => {
        if (e.payload === "mistouch") {
          // Mistouch: no speech expected, close the overlay fast.
          // session-ended will still arrive (likely with status=no_speech)
          // and will be a no-op when it does.
          setTranscript("");
          setInterimTextBoth("");
          transcriptRef.current = "";
          emitOverlayPhase("idle");
          closeOverlay();
        }
      }),

      // ── session-force-abort ───────────────────────────────────────
      // Fired when the user presses ESC. Backend has already set the
      // aborted flag, so the ASR task will produce a
      // session-ended { status: "aborted" } shortly. We set the
      // forcedAbortRef to suppress any in-flight AI optimize output.
      listen<{ generation: number; reason: string }>("session-force-abort", (e) => {
        if (e.payload.generation !== backendGenerationRef.current) return;
        forcedAbortRef.current = true;
        appendLocalLog("info", "已中止当前会话，保留已有转写");
      }),

      // ── recording-status ──────────────────────────────────────────
      // Start/stop markers for the waveform UI. Source of truth for
      // "am I recording right now" in the UI, but NOT for "is the
      // session done" (that's session-ended).
      listen<RecordingStatusPayload>("recording-status", (e) => {
        const { recording, generation } = e.payload;
        // Stale session-stop: when the user rapidly restarts, the old
        // ASR task's wrap-up may emit recording-status(false, N) after
        // a new session N+1 is already active. Drop it so we don't
        // flip the UI back to "not recording" during the new session.
        if (generation < backendGenerationRef.current) return;
        if (generation > backendGenerationRef.current) {
          backendGenerationRef.current = generation;
        }
        const wasRecording = isRecordingRef.current;
        isRecordingRef.current = recording;
        setIsRecording(recording);

        if (recording) {
          // ── New session ─────────────────────────────────────────
          setBanner(null);
          setHasError(false);
          if (errorTimerRef.current !== null) {
            window.clearTimeout(errorTimerRef.current);
            errorTimerRef.current = null;
          }
          setTranscript("");
          setInterimTextBoth("");
          transcriptRef.current = "";
          clearSlowHintTimer();
          forcedAbortRef.current = false;
          sessionIdRef.current += 1;
          setIsPostRecording(false);
          setIsOptimizing(false);
          setOptimizedText("");
          setIsConnected(false);
          recordingStartTimeRef.current = Date.now();
          playStartSound();
          if (settingsRef.current.show_overlay) showOverlay();
          emitOverlayPhase("recording");
        } else if (wasRecording) {
          // ── Backend stopped recording; now waiting for session-ended ──
          //
          // Skip the "processing" UI flash if this session was aborted —
          // session-ended will arrive shortly with status="aborted" and
          // close the overlay, no point showing a processing indicator
          // for a cancelled session.
          if (forcedAbortRef.current) {
            setIsPostRecording(false);
            return;
          }
          // Fast-path: if VAD never observed speech (mistouch hold, silence
          // timeout, no-speech session), there is no ASR result coming
          // worth waiting for. Skip the processing UI and let session-ended
          // (status=no_speech) silently close a moment later.
          if (e.payload.had_speech === false) {
            setIsPostRecording(false);
            return;
          }
          playStopSound();
          setIsPostRecording(true);
          emitOverlayPhase("processing");
          // Start the slow-hint timer (UX: reassure the user that
          // something is still happening if session-ended takes > 3s).
          if (slowHintTimerRef.current !== null) {
            window.clearTimeout(slowHintTimerRef.current);
          }
          slowHintTimerRef.current = window.setTimeout(() => {
            slowHintTimerRef.current = null;
            setIsProcessingSlow(true);
            emitOverlaySlow(true);
            appendLocalLog("info", "识别等待时间较长，继续处理中");
          }, SLOW_HINT_MS);
        }
      }),

      // ── transcription-update (live interim rendering only) ────────
      // Used purely for UI feedback during recording and the brief
      // post-recording window. The authoritative text comes from
      // session-ended — this event is just a faster visual preview.
      listen<TranscriptPayload>("transcription-update", (e) => {
        const gen = e.payload.generation;
        // Self-heal: transcription-update may arrive before recording-status
        if (gen < backendGenerationRef.current) return;
        if (gen > backendGenerationRef.current) {
          backendGenerationRef.current = gen;
        }
        if (forcedAbortRef.current) return;
        const data = e.payload;
        if (data.is_final) {
          setTranscript((prev) => {
            const next = (prev ? prev + "\n" : "") + data.text;
            transcriptRef.current = next;
            return next;
          });
          setInterimTextBoth("");
        } else {
          setInterimTextBoth(data.text);
        }
      }),

      // ── session-ended: the single end-of-session truth ────────────
      listen<SessionEndedPayload>("session-ended", (e) => {
        const {
          generation,
          final_text,
          status,
          error_reason,
          error_detail,
          duration_ms,
          record_id,
        } = e.payload;

        // Stale session-ended (belongs to a session older than the current one):
        // still update history stats if needed, but don't touch UI.
        if (generation < backendGenerationRef.current) {
          if (record_id) setTranscriptRefreshKey((k) => k + 1);
          return;
        }
        if (generation > backendGenerationRef.current) {
          backendGenerationRef.current = generation;
        }

        // Clear the post-recording wait state
        clearSlowHintTimer();
        setIsPostRecording(false);
        setIsConnected(false);
        if (record_id) setTranscriptRefreshKey((k) => k + 1);

        // Update usage stats on successful sessions with text
        if (status === "ok" && final_text && duration_ms > 0) {
          invoke("update_usage_stats", {
            sessionDurationMs: duration_ms,
            text: final_text,
          }).catch(() => {});
        }

        switch (status) {
          case "ok": {
            // Replace any interim text with the authoritative final
            setTranscript(final_text);
            transcriptRef.current = final_text;
            setInterimTextBoth("");

            if (forcedAbortRef.current) {
              emitOverlayPhase("idle");
              closeOverlay();
              return;
            }

            if (settingsRef.current.ai_optimize.enabled) {
              runAiOptimize(final_text, record_id ?? null, generation);
            } else {
              invoke("send_text_input", { text: final_text }).catch(() => {});
              emitOverlayPhase("idle");
              closeOverlay();
            }
            break;
          }

          case "no_speech": {
            // Silent close — no banner, no sound.
            setTranscript("");
            setInterimTextBoth("");
            transcriptRef.current = "";
            emitOverlayPhase("idle");
            closeOverlay();
            break;
          }

          case "aborted": {
            // User-initiated abort. The backend already persisted any
            // accumulated finals as an aborted record. Show the text on
            // screen briefly so the user knows what was kept.
            setTranscript(final_text);
            transcriptRef.current = final_text;
            setInterimTextBoth("");
            appendLocalLog("info", final_text ? "会话已中止，保留已识别文本" : "会话已中止");
            emitOverlayPhase("idle");
            closeOverlay();
            break;
          }

          case "error": {
            // Red waveform + error sound + banner. Show whatever finals
            // the backend accumulated before the error — they're already
            // persisted as a "partial" record.
            const reason = error_reason ?? "语音识别出错";
            const detail = error_detail ?? reason;
            const displayText = settingsRef.current.debug_mode ? detail : reason;
            setBanner({ kind: "error", text: displayText });
            appendLocalLog("error", `语音识别失败：${reason}`);
            playErrorSound();
            if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
            setHasError(true);
            errorTimerRef.current = window.setTimeout(() => {
              setHasError(false);
              errorTimerRef.current = null;
            }, 3000);
            // Show any partial text so the user can see what was preserved
            setTranscript(final_text);
            transcriptRef.current = final_text;
            setInterimTextBoth("");
            // Flash the red error phase on the overlay before closing.
            //
            // Session-guard: capture the current sessionId and only close
            // the overlay if we're still in the same session when the timer
            // fires. Otherwise a new session started within the 2.2s flash
            // window would have its overlay closed by this stale timer.
            emitOverlayPhase("error");
            const errorSessionId = sessionIdRef.current;
            window.setTimeout(() => {
              if (sessionIdRef.current !== errorSessionId) return;
              emitOverlayPhase("idle");
              closeOverlay();
            }, ERROR_FLASH_MS);
            break;
          }
        }
      }),

      listen<string>("settings-warning", (e) => {
        setBanner({ kind: "warning", text: e.payload });
      }),

      listen<boolean>("connection-status", (e) => {
        setIsConnected(e.payload);
      }),

      listen<{ chunk: string; session_id: number }>("ai-optimize-chunk", (e) => {
        if (forcedAbortRef.current) return;
        if (e.payload.session_id === sessionIdRef.current) {
          setOptimizedText((prev) => prev + e.payload.chunk);
        }
      }),

      listen<string>("network-log", (e) => {
        try {
          appendLogEntry(JSON.parse(e.payload) as LogEntry);
        } catch {}
      }),

      // Sync settings when changed from tray menu or other sources
      listen("settings-changed", () => {
        invoke<AppSettings>("get_settings")
          .then((s) => { if (s) setSettings(s); })
          .catch(() => {});
      }),
      listen("show-about", () => {
        setAboutOpen(true);
      }),
      listen("show-stats", () => {
        setSettingsInitialTab("stats");
        setSettingsOpen(true);
      }),
    ];

    Promise.all(listenerPromises).then((fns) => {
      unlisteners.push(...fns);
      invoke("emit_pending_settings_warning").catch(() => {});
    });

    return () => { unlisteners.forEach((fn) => fn()); };
  }, [
    appendLocalLog,
    appendLogEntry,
    clearSlowHintTimer,
    closeOverlay,
    emitOverlayPhase,
    emitOverlaySlow,
    runAiOptimize,
    setInterimTextBoth,
  ]);

  const handleToggleRecording = useCallback(async () => {
    // NOTE: no gate against isPostRecording/isOptimizing — the whole
    // point of the 2026-04 refactor is that any session's post-processing
    // (waiting for session-ended, AI optimize, text output) never blocks
    // a new recording. The user's flow trumps any in-flight work.
    try {
      setBanner(null);
      if (isRecording) {
        await invoke("stop_recording");
      } else {
        const needsConfig = (() => {
          switch (settings.provider) {
            case "doubao": return !settings.doubao.app_id || !settings.doubao.access_token;
            case "dashscope": return !settings.dashscope.api_key;
            case "qwen": return !settings.qwen.api_key;
            default: return true;
          }
        })();
        if (needsConfig) {
          setBanner({
            kind: "error",
            text: "还没有配置语音识别服务，先在设置中填入服务商凭据吧",
            link: {
              text: "或查看教程 ↗",
              url: "https://afengblog.com/blog/speakin-settings-guide.html",
            },
          });
          setSettingsOpen(true);
          return;
        }
        await invoke("start_recording");
      }
    } catch (e) {
      setBanner({ kind: "error", text: String(e) });
    }
  }, [isRecording, settings]);

  const handleSaveSettings = useCallback(async (newSettings: AppSettings) => {
    try {
      setBanner(null);
      const savedSettings = await invoke<AppSettings>("save_settings", { settings: newSettings });
      pendingSettingsRef.current = savedSettings;
      // Sync settingsRef immediately so in-flight recordings see the new value
      // (setSettings is deferred until panel close to avoid re-render flash)
      settingsRef.current = savedSettings;
      invoke("rebuild_tray_menu_cmd").catch(() => {});
      return savedSettings;
    } catch (e) {
      const message = String(e);
      setBanner({ kind: "error", text: message });
      throw new Error(message);
    }
  }, []);

  // Auto-scroll transcript to bottom on new text
  useEffect(() => {
    const el = transcriptScrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [transcript, interimText]);

  useEffect(() => () => {
    if (toastTimerRef.current) clearTimeout(toastTimerRef.current);
    if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
  }, []);

  const copyText = useCallback(async (text: string) => {
    if (!text) return;
    try {
      await navigator.clipboard.writeText(text);
      showToast("已复制");
    } catch {
      showToast("复制失败");
    }
  }, [showToast]);

  const handleClear = useCallback(() => {
    setTranscript("");
    setInterimTextBoth("");
    setOptimizedText("");
    transcriptRef.current = "";
    showToast("已清除");
  }, [showToast, setInterimTextBoth]);

  const handleCopyAiLogs = useCallback(() => {
    if (latestAiLogText) {
      navigator.clipboard.writeText(latestAiLogText);
    }
  }, [latestAiLogText]);

  const handleOutputModeToggle = useCallback(async (mode: "paste" | "type") => {
    if (isRecording || settingsOpen) return;
    const newMode = settings.output_mode === mode ? "none" : mode;
    const updated = { ...settings, output_mode: newMode as AppSettings["output_mode"] };
    try {
      const saved = await invoke<AppSettings>("save_settings", { settings: updated });
      setSettings(saved);
    } catch {}
  }, [settings, isRecording, settingsOpen]);

  const handleCloseSettings = useCallback(() => {
    setSettingsOpen(false);
    // Revert any unsaved theme color preview back to the persisted value
    const isDark = document.documentElement.classList.contains("dark");
    applyThemeColor(settings.theme_color, isDark, settings.recording_follows_theme);
  }, [settings.theme_color, settings.recording_follows_theme]);

  const handleSettingsExitComplete = useCallback(() => {
    if (pendingSettingsRef.current) {
      setSettings(pendingSettingsRef.current);
      pendingSettingsRef.current = null;
    }
    setSettingsInitialTab(undefined); // Reset so next open uses default tab
  }, []);

  // ── Window close behavior ──
  const handleWindowClose = useCallback(async () => {
    if (closingRef.current) return;
    closingRef.current = true;
    try {
      const behavior = settingsRef.current.close_behavior;
      if (behavior === "minimize") {
        await getCurrentWindow().hide();
      } else if (behavior === "quit") {
        await invoke("quit_app");
        return; // app is exiting, don't reset
      } else {
        setCloseDialogOpen(true);
      }
    } catch (e) {
      console.error("close action failed:", e);
    }
    closingRef.current = false;
  }, []);

  const handleCloseChoice = useCallback(async (behavior: "minimize" | "quit", remember: boolean) => {
    setCloseDialogOpen(false);
    if (remember) {
      const updated = { ...settingsRef.current, close_behavior: behavior };
      try {
        const saved = await invoke<AppSettings>("save_settings", { settings: updated });
        setSettings(saved);
      } catch {}
    }
    if (behavior === "minimize") {
      await getCurrentWindow().hide();
    } else {
      await invoke("quit_app");
    }
  }, []);

  const handleCloseCancel = useCallback(() => {
    setCloseDialogOpen(false);
  }, []);

  // Intercept all close requests (Alt+F4, taskbar close, etc.)
  useEffect(() => {
    const unlisten = getCurrentWindow().onCloseRequested(async (e) => {
      e.preventDefault();
      handleWindowClose();
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [handleWindowClose]);

  // ESC key closes settings modal
  useEffect(() => {
    if (!settingsOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") handleCloseSettings();
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [settingsOpen, handleCloseSettings]);

  // JS fallback for when our WebView2 window has focus.
  // WebView2/Chromium uses a different keyboard input pipeline that bypasses
  // the native WH_KEYBOARD_LL hook when the WebView has focus.
  // The native hook handles all other windows; this JS listener covers our own.
  // Double-triggering is safe: start/stop are idempotent (mutex-guarded is_recording check).
  useEffect(() => {
    // Track whether JS initiated a hold-start (for blur safety)
    let jsHoldActive = false;
    let jsHoldStartTime = 0;
    const MIN_HOLD_MS = 300;

    // Extract the trigger key name from the configured hotkey (e.g., "Ctrl+R" → "R")
    const getTriggerKey = () => {
      const parts = settingsRef.current.hotkey.split("+");
      return parts[parts.length - 1];
    };

    const onKeyDown = (e: KeyboardEvent) => {
      if (
        e.key === "Escape"
        && settingsRef.current.esc_abort_enabled
        && (isRecording || isPostRecording || isOptimizing)
      ) {
        e.preventDefault();
        e.stopPropagation();
        if (!e.repeat) {
          appendLocalLog("info", "窗口内 ESC：请求中止当前会话");
          invoke("abort_current_session").catch(() => {});
        }
        return;
      }
      if (e.repeat) return; // Suppress key-repeat (prevents rapid toggle in Toggle mode)
      const combo = buildHotkeyString(e);
      if (!combo || combo !== settingsRef.current.hotkey) return;
      e.preventDefault();
      e.stopPropagation();
      if (settingsRef.current.input_mode === "hold") {
        if (!isRecordingRef.current) {
          jsHoldActive = true;
          jsHoldStartTime = Date.now();
          appendLocalLog("info", "窗口内热键按下：请求开始录音");
          handleToggleRecording();
        }
      } else {
        appendLocalLog("info", isRecordingRef.current ? "窗口内热键：请求结束录音" : "窗口内热键：请求开始录音");
        handleToggleRecording();
      }
    };

    const onKeyUp = (e: KeyboardEvent) => {
      if (settingsRef.current.input_mode !== "hold") return;
      if (!jsHoldActive) return;
      // Match only the trigger key on release, ignoring modifiers.
      // This mirrors the native hook behavior: if user presses Ctrl+R then
      // releases Ctrl first, the R keyup should still stop recording.
      const keyName = keyToHotkeyName(e);
      if (!keyName || keyName !== getTriggerKey()) return;
      e.preventDefault();
      e.stopPropagation();
      jsHoldActive = false;
      // Use jsHoldActive (now false) as the primary guard instead of isRecordingRef,
      // because recording-status(true) may not have arrived yet via async IPC.
      // handleToggleRecording → invoke("stop_recording") is idempotent (is_recording guard).
      //
      // Mistouch handling: a <300ms hold rarely captures any speech (VAD
      // needs multiple consecutive speech frames), so the backend's
      // session-ended will come back with status="no_speech" and the
      // UI closes silently. No special flag needed.
      if (Date.now() - jsHoldStartTime < MIN_HOLD_MS) {
        appendLocalLog("info", "窗口内热键释放：判定为误触，取消本次录音");
      } else {
        appendLocalLog("info", "窗口内热键释放：请求结束录音");
      }
      handleToggleRecording();
    };

    // Hold mode safety: if the user switches focus away while holding the key,
    // we won't receive keyup. Stop recording on blur to prevent it getting stuck.
    // Uses jsHoldActive instead of isRecordingRef to handle the race where
    // blur fires before the recording-status event updates the ref.
    const onBlur = () => {
      if (jsHoldActive) {
        jsHoldActive = false;
        if (Date.now() - jsHoldStartTime < MIN_HOLD_MS) {
          appendLocalLog("info", "窗口失焦：判定为误触，取消本次录音");
        } else {
          appendLocalLog("info", "窗口失焦：请求结束录音");
        }
        handleToggleRecording();
      }
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("blur", onBlur);
    };
  }, [appendLocalLog, handleToggleRecording, isOptimizing, isPostRecording, isRecording]);

  return (
    <div className={cn(
      "flex flex-col h-screen bg-surface overflow-hidden transition-opacity duration-300",
      appReady ? "opacity-100" : "opacity-0"
    )}>
      {/* Title Bar */}
      <TitleBar onClose={handleWindowClose}>
        <ThemeToggle />
        <Tooltip content="设置" side="bottom">
          <button
            onClick={() => setSettingsOpen(true)}
            className={cn("p-1.5 rounded-md transition-all duration-[var(--t-fast)]", "text-fg-2 hover:text-fg hover:bg-surface-subtle active:scale-95")}
          >
            <SettingsIcon size={16} />
          </button>
        </Tooltip>
      </TitleBar>

      {/* Global Toast — top center */}
      <AnimatePresence>
        {toast && (
          <motion.div key="toast"
            className="absolute top-10 left-0 right-0 flex justify-center pointer-events-none z-[50]"
            initial={{ opacity: 0, y: -8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -8 }}
            transition={{ duration: 0.15 }}>
            <div className="px-4 py-1.5 rounded-full bg-surface-card border border-edge shadow-md text-xs font-medium text-fg">
              {toast}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      {/* Main Content — top-aligned fixed layout, no centering to prevent jumps */}
      <main className="flex-1 flex flex-col px-5 py-5 gap-4 overflow-hidden min-h-0">
        <div className="shrink-0 flex justify-center">
          <VoicePanel isRecording={isRecording} isProcessing={isPostRecording && !isOptimizing} isProcessingSlow={isProcessingSlow} isOptimizing={isOptimizing} hasError={hasError} onToggle={handleToggleRecording} settings={settings} />
        </div>

        {/* Error — smooth expand/collapse */}
        <div className={cn(
          "w-full grid shrink-0 transition-all duration-[var(--t-base)]",
          banner ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"
        )}>
          <div className="overflow-hidden">
            <div className={cn(
              "px-4 py-3 rounded-xl text-sm select-text",
              banner?.kind === "warning"
                ? "bg-primary/10 text-primary"
                : "bg-danger-muted text-danger-muted-fg"
            )}>
              {banner?.text}
              {banner?.link && (
                <>
                  {" "}
                  <button
                    onClick={() => openUrl(banner.link!.url).catch(() => {})}
                    className="underline underline-offset-2 hover:opacity-80 transition-opacity font-medium"
                  >
                    {banner.link.text}
                  </button>
                </>
              )}
            </div>
          </div>
        </div>

        {/* Transcript — fills remaining space */}
        <div className="w-full flex-1 min-h-0 flex flex-col">
          <div className="flex items-center justify-between mb-2 shrink-0">
            <span className="text-xs font-medium text-fg-3 uppercase tracking-widest">转写结果</span>
            <div className="flex gap-1">
              <button onClick={handleClear} disabled={(!transcript && !interimText && !optimizedText) || isRecording || isOptimizing}
                className={cn("inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-lg",
                  "transition-all duration-[var(--t-fast)]",
                  "text-fg-2 hover:text-fg bg-surface-subtle hover:bg-surface-inset active:scale-95",
                  "disabled:opacity-40 disabled:cursor-not-allowed")}>
                <Trash2 size={12} />
                清除
              </button>
            </div>
          </div>
          <div className="relative flex-1 min-h-0">
          <div
            ref={transcriptScrollRef}
            className={cn(
            "h-full rounded-xl p-4 overflow-y-auto",
            "bg-surface-card border border-edge select-text",
            "transition-colors duration-[var(--t-base)]"
          )}>
            {transcript || interimText || isOptimizing || optimizedText || (settings.debug_mode && aiLogs.length > 0) ? (
              <div className="text-sm leading-relaxed whitespace-pre-wrap">
                {transcript && (
                  <button
                    onClick={() => copyText(transcript)}
                    title="复制转写原文"
                    className={cn(
                      "float-right ml-2 mb-1 inline-flex items-center justify-center w-6 h-6 rounded-md shrink-0",
                      "text-fg-3 hover:text-fg bg-surface-subtle/60 hover:bg-surface-inset active:scale-95",
                      "transition-all duration-[var(--t-fast)]"
                    )}
                  >
                    <Copy size={12} />
                  </button>
                )}
                {transcript && <span className="text-fg">{transcript}</span>}
                {interimText && (
                  <span className="text-fg-3 italic">
                    {transcript ? "\n" : ""}{interimText}
                  </span>
                )}
                {(isOptimizing || optimizedText) && (
                  <div className="mt-3 pt-3 border-t border-edge clear-both">
                    <div className="flex items-center justify-between mb-1.5">
                      <div className="flex items-center gap-1.5">
                        <span className="text-xs font-medium text-primary uppercase tracking-widest">
                          AI 优化
                        </span>
                        {isOptimizing && (
                          <span className="w-1.5 h-1.5 rounded-full bg-primary animate-pulse" />
                        )}
                      </div>
                      {optimizedText && (
                        <button
                          onClick={() => copyText(optimizedText)}
                          title="复制 AI 优化结果"
                          className={cn(
                            "inline-flex items-center justify-center w-6 h-6 rounded-md shrink-0",
                            "text-fg-3 hover:text-fg bg-surface-subtle/60 hover:bg-surface-inset active:scale-95",
                            "transition-all duration-[var(--t-fast)]"
                          )}
                        >
                          <Copy size={12} />
                        </button>
                      )}
                    </div>
                    {optimizedText && (
                      <span className="text-fg">{optimizedText}</span>
                    )}
                    {isOptimizing && !optimizedText && (
                      <span className="text-fg-3 italic">正在优化</span>
                    )}
                  </div>
                )}
                {settings.debug_mode && aiLogs.length > 0 && (
                  <div className="mt-3 rounded-lg border border-edge bg-surface-subtle/60">
                    <div className="flex items-center justify-between px-3 py-2 border-b border-edge">
                      <span className="text-[11px] font-medium text-fg-3 uppercase tracking-widest">
                        AI 请求日志
                      </span>
                      <button
                        onClick={handleCopyAiLogs}
                        className={cn(
                          "px-2 py-1 text-[11px] font-medium rounded-md transition-all duration-[var(--t-fast)]",
                          "text-fg-2 hover:text-fg bg-surface-card hover:bg-surface-inset active:scale-95"
                        )}
                      >
                        复制日志
                      </button>
                    </div>
                    <div className="max-h-44 overflow-y-auto px-3 py-2 font-mono text-xs leading-5 text-fg-2 whitespace-pre-wrap break-all select-text">
                      {aiLogs.map((log, index) => (
                        <div key={`${log.ts}-${index}`} className="mb-3 last:mb-0">
                          <div className="text-fg-3">
                            [{log.ts}] {log.level.toUpperCase()}
                          </div>
                          <div>{log.msg}</div>
                        </div>
                      ))}
                    </div>
                  </div>
                )}
              </div>
            ) : (
              <div className="flex flex-col h-full">
                <div className="flex-1 min-h-0 overflow-y-auto">
                  <RecentTranscripts refreshKey={transcriptRefreshKey} onToast={showToast} />
                </div>
                <div className="flex items-center justify-center py-4 shrink-0">
                  <p className="text-fg-3 text-sm">
                    {isRecording ? "正在聆听..." : "按下热键或点击麦克风开始录音"}
                  </p>
                </div>
              </div>
            )}
          </div>
          {/* (toast moved to top-level) */}
          </div>
        </div>

        {/* Status Bar */}
        <div className="flex items-center justify-between text-[11px] text-fg-3 shrink-0 px-2.5 py-1 border-t border-edge/40">
          <Tooltip content={isConnected ? "服务已连接" : "服务未连接"} side="top">
            <span className={cn(
              "flex items-center transition-colors",
              isConnected ? "text-ok" : "text-fg-3/40"
            )}>
              {isConnected ? <Wifi size={13} /> : <WifiOff size={13} />}
            </span>
          </Tooltip>
          <span className="flex items-center gap-1.5">
            <span className="font-mono px-1.5 py-0.5 rounded bg-surface-subtle text-fg-3/70">{settings.hotkey}</span>
            <span className="text-fg-3/40">·</span>
            <span className="text-fg-3/50">{settings.input_mode === "toggle" ? "切换" : "按住"}</span>
          </span>
          <div className={cn(
            "flex items-center rounded-md border border-edge overflow-hidden text-[11px]",
            (isRecording || settingsOpen) && "opacity-50 pointer-events-none"
          )}>
            <button onClick={() => handleOutputModeToggle("paste")}
              className={cn("px-2 py-0.5 transition-all duration-[var(--t-fast)]",
                settings.output_mode === "paste"
                  ? "bg-primary/12 text-primary font-medium"
                  : "text-fg-3/60 hover:text-fg-3 hover:bg-surface-subtle"
              )}>粘贴</button>
            <div className="w-px h-3 bg-edge" />
            <button onClick={() => handleOutputModeToggle("type")}
              className={cn("px-2 py-0.5 transition-all duration-[var(--t-fast)]",
                settings.output_mode === "type"
                  ? "bg-primary/12 text-primary font-medium"
                  : "text-fg-3/60 hover:text-fg-3 hover:bg-surface-subtle"
              )}>键入</button>
          </div>
        </div>
      </main>

      {/* Network Log Panel */}
      {settings.debug_mode && (
        <NetworkLog logs={logs} onClear={() => setLogs([])} />
      )}

      {/* Settings Panel Overlay — AnimatePresence for flicker-free transitions */}
      <AnimatePresence onExitComplete={handleSettingsExitComplete}>
        {settingsOpen && (
          <>
            {/* Backdrop */}
            <motion.div
              key="settings-backdrop"
              className="absolute inset-0 z-[110] bg-black/20 backdrop-blur-sm"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2 }}
              onClick={handleCloseSettings}
            />
            {/* Modal: centered dialog, sibling of backdrop for independent exit animation */}
            <motion.div
              key="settings-modal"
              className="absolute inset-0 z-[110] flex items-center justify-center pointer-events-none"
              initial={{ opacity: 0, scale: 0.95 }}
              animate={{ opacity: 1, scale: 1 }}
              exit={{ opacity: 0, scale: 0.95 }}
              transition={{ type: "spring", damping: 30, stiffness: 350 }}
            >
              <div
                className="pointer-events-auto w-full max-w-[calc(100%-1rem)] h-[85vh] bg-surface-card border border-edge rounded-2xl shadow-2xl overflow-hidden flex flex-col"
                onClick={(e) => e.stopPropagation()}
              >
                <Settings settings={settings} onSave={handleSaveSettings} onClose={handleCloseSettings} initialTab={settingsInitialTab} isRecording={isRecording} />
              </div>
            </motion.div>
          </>
        )}
      </AnimatePresence>

      {/* Close behavior dialog */}
      <CloseDialog
        open={closeDialogOpen}
        onChoice={handleCloseChoice}
        onCancel={handleCloseCancel}
      />

      {/* About dialog */}
      <AboutDialog open={aboutOpen} onClose={() => setAboutOpen(false)} />

      {/* First-launch onboarding */}
      <OnboardingDialog
        open={onboardingOpen}
        onClose={() => {
          setOnboardingOpen(false);
          const updated = { ...settings, onboarding_completed: true };
          setSettings(updated);
          invoke("save_settings", { settings: updated }).catch(() => {});
        }}
        onConfigure={(provider) => {
          setOnboardingOpen(false);
          const updated = { ...settings, provider, onboarding_completed: true };
          setSettings(updated);
          invoke("save_settings", { settings: updated }).catch(() => {});
          setSettingsInitialTab("asr");
          setSettingsOpen(true);
        }}
      />
    </div>
  );
}
