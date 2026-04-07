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
  close_behavior: "ask" | "minimize" | "quit";
  onboarding_completed: boolean;
  copy_to_clipboard: boolean;
  paste_restore_clipboard: boolean;
  system_no_auto_stop: boolean;
  esc_abort_enabled: boolean;
}

interface TranscriptPayload {
  text: string;
  is_final: boolean;
  generation: number;
}

interface RecordingStatusPayload {
  recording: boolean;
  generation: number;
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
  close_behavior: "ask",
  onboarding_completed: false,
  copy_to_clipboard: false,
  paste_restore_clipboard: true,
  system_no_auto_stop: false,
  esc_abort_enabled: true,
};

/** Simplify raw backend error strings for non-debug display. */
function simplifyError(raw: string): string {
  if (raw.includes("连接失败")) return "语音服务连接失败，检查网络后重试";
  if (raw.includes("超时")) return "语音服务响应超时";
  if (raw.includes("401") || raw.includes("403")) return "语音服务鉴权失败，检查凭据后重试";
  return "语音识别出错，稍后重试";
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
  const autoInputTimerRef = useRef<number | null>(null);
  // [Batch 2] 3s soft-hint timer — when post-recording waits > 3s for ASR
  // FINAL, flip `isProcessingSlow` so the UI shows "正在努力识别中" instead of
  // "识别中". Cleared on FINAL arrival, new session, forced abort, or error.
  const slowHintTimerRef = useRef<number | null>(null);
  const [isProcessingSlow, setIsProcessingSlow] = useState(false);
  const textSentRef = useRef(false);
  // Two distinct session counters, by design:
  //   sessionIdRef       — frontend-owned, increments on recording-status(true).
  //                        Gates AI optimize callbacks and overlay-phase events
  //                        (both are frontend-initiated flows).
  //   backendGenerationRef — shadow of backend's recording_generation.
  //                        Updated from recording-status AND self-heals when
  //                        transcription-update arrives with a newer generation
  //                        (Tauri cross-thread emit is not strictly ordered).
  //                        Used to filter ASR transcription events from stale sessions.
  // The two don't have to agree on value, just each monotonically increase.
  const sessionIdRef = useRef(0);
  const backendGenerationRef = useRef(0);
  // [修订 R2] Tracks whether the current session's text has been persisted to
  // history. Independent from textSentRef (which only means "doAutoInput has
  // been entered"). Used by the new-session rescue block to decide whether
  // to flush pending text from a session whose doAutoInput never reached
  // save_transcript_record (e.g., AI optimize hung on the network).
  const persistedRef = useRef(false);
  // [Codex Check 7a fix] Stores the record id returned by doAutoInput's
  // Step 1 save_transcript_record. Used by handleForcedSessionAbort to
  // promote the saved record's status from "partial" → "aborted" when
  // the user presses ESC during the AI optimize phase. Cleared on every
  // new session start.
  const currentRecordIdRef = useRef<string | null>(null);
  // Ref mirror of `interimText` state — event listeners can't read current
  // state values from closures. Must be kept in sync via `setInterimTextBoth`.
  const interimTextRef = useRef("");
  const [isPostRecording, setIsPostRecording] = useState(false); // ASR processing after recording stops
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
  const settingsRef = useRef(settings);
  settingsRef.current = settings;
  const recordingStartTimeRef = useRef(0);
  const errorHandledRef = useRef(false); // Prevents asr-error + recording-status(false) race
  const cancelledRef = useRef(false); // Mistouch cancel — skip post-recording flow
  const forcedAbortRef = useRef(false); // Manual abort — ignore late ASR/AI callbacks
  const aiLogs = logs.filter((log) => log.msg.includes("[AI]"));
  const latestAiLogText = aiLogs.map((log) => `[${log.ts}] ${log.level.toUpperCase()} ${log.msg}`).join("\n\n");

  // Helper: update both the React state AND the ref mirror in lockstep.
  // ALL call sites that clear or update `interimText` must use this helper —
  // otherwise the ref goes stale and rescue/listener logic reads wrong values.
  const setInterimTextBoth = useCallback((value: string) => {
    interimTextRef.current = value;
    setInterimText(value);
  }, []);

  type OverlayPhase = "recording" | "processing" | "optimizing" | "idle";
  const emitOverlayPhase = useCallback((phase: OverlayPhase) => {
    emit("overlay-phase", { phase, sessionId: sessionIdRef.current });
  }, []);

  // [Batch 2] Clear both post-recording timers (force-advance + soft-hint)
  // and reset the slow-hint flag. Must be called on every path that exits
  // the post-recording phase, otherwise the 6s force-advance may fire
  // against a new session or the hint state leaks across sessions.
  // [Codex Check 5 fix] All overlay-slow emits carry the current
  // sessionId. The overlay window filters out events from stale sessions
  // so a late-arriving `slow=true` from a previous session can never
  // override a freshly reset state.
  const emitOverlaySlow = useCallback((slow: boolean) => {
    emit("overlay-slow", { slow, sessionId: sessionIdRef.current }).catch(() => {});
  }, []);

  const clearPostRecordingTimers = useCallback(() => {
    if (autoInputTimerRef.current !== null) {
      window.clearTimeout(autoInputTimerRef.current);
      autoInputTimerRef.current = null;
    }
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

  const handleForcedSessionAbort = useCallback(() => {
    forcedAbortRef.current = true;
    cancelledRef.current = isRecordingRef.current;
    textSentRef.current = true;

    // ── ESC abort rescue ──────────────────────────────────────────────
    // Save whatever first-pass text the user already saw on screen
    // (transcript + interim) as an "aborted" history record before
    // wiping state. The user's intent in pressing ESC is "stop this
    // session", not "throw away what you already heard" — Doubao's
    // first-pass interim is still valuable.
    //
    // Two cases:
    //   1. persistedRef === true: doAutoInput Step 1 already saved the
    //      raw text as a "partial" record (ESC during AI optimize). Use
    //      currentRecordIdRef to promote partial → aborted via the new
    //      update_transcript_status command. [Codex Check 7a fix]
    //   2. persistedRef === false: nothing saved yet. Save now as
    //      "aborted" directly. [Codex Check 7b: track save success
    //      precisely so a failure shows a banner instead of silently
    //      dropping content.]
    let attemptedSave = false;
    if (persistedRef.current && currentRecordIdRef.current) {
      // Case 1: promote existing partial record to aborted.
      const idToUpdate = currentRecordIdRef.current;
      invoke("update_transcript_status", {
        id: idToUpdate,
        status: "aborted",
      })
        .then(() => setTranscriptRefreshKey((k) => k + 1))
        .catch((err) => {
          // Record may have been evicted; not user-visible enough to
          // alert, just log.
          console.warn("ESC promote partial→aborted failed:", err);
        });
    } else if (!persistedRef.current) {
      const parts = [transcriptRef.current, interimTextRef.current]
        .filter((s): s is string => typeof s === "string" && s.trim().length > 0);
      if (parts.length > 0) {
        attemptedSave = true;
        const pending = parts.join("\n");
        const durationMs = recordingStartTimeRef.current > 0
          ? Date.now() - recordingStartTimeRef.current
          : 0;
        invoke<string>("save_transcript_record", {
          original: pending,
          optimized: null,
          durationMs,
          status: "aborted",
        })
          .then(() => {
            persistedRef.current = true;
            setTranscriptRefreshKey((k) => k + 1);
          })
          .catch((err) => {
            if (err === "empty_text") {
              // Race: text was effectively empty after backend processing
              // (filler/replacements stripped it). Treat as persisted to
              // avoid stale rescue attempts.
              persistedRef.current = true;
            } else {
              console.error("ESC abort save failed:", err);
              setBanner({
                kind: "error",
                text: settingsRef.current.debug_mode
                  ? `中止保存失败: ${err}`
                  : "中止时保存历史记录失败",
              });
              // Leave persistedRef as-is — there's nothing the rescue can
              // do (state already cleared), but at least the user was
              // notified.
            }
          });
      }
    }

    // If we didn't attempt a save (no pending text, OR doAutoInput already
    // persisted as "partial"), mark as persisted so the new-session rescue
    // skips it. The async save's .then() handles the success path above.
    if (!attemptedSave) {
      persistedRef.current = true;
    }
    clearPostRecordingTimers();
    setTranscript("");
    setInterimTextBoth("");
    setOptimizedText("");
    transcriptRef.current = "";
    setIsPostRecording(false);
    setIsOptimizing(false);
    emitOverlayPhase("idle");
    closeOverlay();
    // Release the backend is_processing gate — the forced-abort flow never
    // reaches doAutoInput's .finally block (forcedAbortRef short-circuits it),
    // so the backend would otherwise wait for the 65s safety timeout.
    invoke("mark_session_idle", {
      generation: backendGenerationRef.current,
    }).catch(() => {});
  }, [closeOverlay, emitOverlayPhase, setInterimTextBoth, clearPostRecordingTimers]);

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

  useEffect(() => {
    const active = settings.esc_abort_enabled && (isRecording || isPostRecording || isOptimizing);
    invoke("set_escape_abort_active", { active }).catch(() => {});
    return () => {
      invoke("set_escape_abort_active", { active: false }).catch(() => {});
    };
  }, [settings.esc_abort_enabled, isRecording, isPostRecording, isOptimizing]);

  // Auto-input pipeline: persist to history FIRST (so content is safe even
  // if anything below fails), then AI optimize (if enabled), then paste.
  //
  // Why "persist first": the old pipeline bolted `send_text_and_record` to
  // the tail end of AI optimize, so a hung AI request meant history was
  // never written. Now the raw text is written to history the moment
  // doAutoInput starts; AI result (if any) is then appended via
  // update_transcript_optimized. See Batch 1 plan Fix B.
  const doAutoInput = useCallback(async (text: string, generation: number) => {
    if (textSentRef.current || forcedAbortRef.current) return;
    textSentRef.current = true;
    // [Batch 2] Slow hint only applies while waiting for FINAL — once
    // doAutoInput runs we're past that phase, hide the hint. Also
    // ensures the overlay is in sync if doAutoInput was reached via
    // the 6s force-advance path.
    setIsProcessingSlow(false);
    emitOverlaySlow(false);

    // Update usage statistics
    const durationMs = recordingStartTimeRef.current > 0
      ? Date.now() - recordingStartTimeRef.current
      : 0;
    if (text && durationMs > 0) {
      invoke("update_usage_stats", {
        sessionDurationMs: durationMs,
        text,
      }).catch(() => {});
    }

    const currentSession = sessionIdRef.current;
    const currentSettings = settingsRef.current;

    // ── Step 1: persist to history (no output, persist-only) ─────────
    // If AI is enabled, save as "partial" — will be promoted to "done"
    // by update_transcript_optimized once AI succeeds. If AI is disabled,
    // this is already the final state so save as "done".
    let recordId: string | null = null;
    try {
      recordId = await invoke<string>("save_transcript_record", {
        original: text,
        optimized: null,
        durationMs,
        status: currentSettings.ai_optimize.enabled ? "partial" : "done",
      });
      persistedRef.current = true;
      // [Codex Check 7a fix] Expose recordId so ESC abort can promote
      // partial → aborted if the user cancels during AI optimize.
      currentRecordIdRef.current = recordId;
      setTranscriptRefreshKey((k) => k + 1);
    } catch (e) {
      // [修订 R6] `empty_text` is expected (empty content shouldn't be saved),
      // swallow silently. Other errors are real failures worth logging.
      if (e !== "empty_text") {
        console.error("Failed to persist transcript:", e);
      }
      // recordId stays null — AI success path has a fallback retry (修订 R2)
    }

    // ── Step 2: AI optimize (if enabled) or direct paste ─────────────
    if (currentSettings.ai_optimize.enabled) {
      setIsOptimizing(true);
      setOptimizedText("");
      emitOverlayPhase("optimizing");
      setLogs((prev) => prev.filter((log) => !log.msg.includes("[AI]")));

      try {
        const optimized = await invoke<string>("ai_optimize_text", {
          text,
          sessionId: currentSession,
        });

        if (sessionIdRef.current !== currentSession || forcedAbortRef.current) {
          return; // stale session, drop
        }

        setOptimizedText(optimized);

        if (recordId) {
          // Normal path: update the record we saved in step 1.
          invoke("update_transcript_optimized", {
            id: recordId,
            optimized,
          })
            .then(() => setTranscriptRefreshKey((k) => k + 1))
            .catch(() => {});
        } else {
          // [修订 R2] Initial save failed but AI succeeded. Fallback retry
          // with full data (original + optimized) as one complete record.
          invoke("save_transcript_record", {
            original: text,
            optimized,
            durationMs,
            status: "done",
          })
            .then(() => setTranscriptRefreshKey((k) => k + 1))
            .catch(() => {
              console.error("Both initial and fallback saves failed");
            });
        }

        // Paste the optimized version
        invoke("send_text_input", { text: optimized }).catch(() => {});
      } catch (e) {
        if (sessionIdRef.current !== currentSession || forcedAbortRef.current) return;

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

        // AI failed — original text is already in history (status=partial).
        // Just fall back to pasting the raw text.
        invoke("send_text_input", { text }).catch(() => {});
      } finally {
        if (sessionIdRef.current === currentSession && !forcedAbortRef.current) {
          setIsOptimizing(false);
          emitOverlayPhase("idle");
          closeOverlay();
          invoke("mark_session_idle", { generation }).catch(() => {});
        }
      }
    } else {
      // Non-AI path: persisted in step 1 with status="done", just paste.
      invoke("send_text_input", { text }).catch(() => {});
      emitOverlayPhase("idle");
      closeOverlay();
      invoke("mark_session_idle", { generation }).catch(() => {});
    }
  }, [closeOverlay, emitOverlayPhase, emitOverlaySlow]);

  useEffect(() => {
    const unlisteners: (() => void)[] = [];
    const listenerPromises = [
      listen<string>("recording-cancelled", () => {
        // Mistouch: mark so recording-status(false) skips post-recording flow
        cancelledRef.current = true;
      }),
      listen<{ generation: number; reason: string }>("session-force-abort", (e) => {
        // [Codex Check 6 fix] Filter stale aborts. abort releases the
        // backend gate immediately, so a new session may already be
        // running by the time this event arrives. Only act on aborts
        // tagged with the current backend generation.
        if (e.payload.generation !== backendGenerationRef.current) {
          return;
        }
        handleForcedSessionAbort();
      }),
      listen<RecordingStatusPayload>("recording-status", (e) => {
      const { recording, generation } = e.payload;
      // Sync backend generation ref — ASR transcription-update events are
      // filtered against this to discard stale-session packets.
      backendGenerationRef.current = generation;
      const wasRecording = isRecordingRef.current;
      isRecordingRef.current = recording;
      setIsRecording(recording);
      if (recording) {
        // ── [Fix B Rescue] Before wiping state: if the previous session
        // ── ended but its text was never persisted (fallback timer cleared
        // ── by this very handler OR AI optimize hung on the network),
        // ── flush pending text to history as "partial". Persist-only,
        // ── NO send_text_input — avoids ghost-typing old text into the
        // ── user's current focused window (Codex caught this).
        //
        // CRITICAL: capture duration BEFORE resetting recordingStartTimeRef
        // a few lines down.
        const priorDurationMs = recordingStartTimeRef.current > 0
          ? Date.now() - recordingStartTimeRef.current
          : 0;
        if (!persistedRef.current) {
          const parts = [transcriptRef.current, interimTextRef.current]
            .filter((s): s is string => typeof s === "string" && s.trim().length > 0);
          if (parts.length > 0) {
            const pending = parts.join("\n");
            invoke<string>("save_transcript_record", {
              original: pending,
              optimized: null,
              durationMs: priorDurationMs,
              status: "partial",
            })
              .then(() => setTranscriptRefreshKey((k) => k + 1))
              .catch((err) => {
                if (err !== "empty_text") {
                  console.error("Rescue save failed:", err);
                }
              });
          }
        }

        // ── Now safe to reset all state ──────────────────────────────
        setBanner(null);
        setHasError(false);
        errorHandledRef.current = false;
        if (errorTimerRef.current !== null) {
          window.clearTimeout(errorTimerRef.current);
          errorTimerRef.current = null;
        }
        setTranscript("");
        setInterimTextBoth("");
        transcriptRef.current = "";
        clearPostRecordingTimers();
        textSentRef.current = false;
        persistedRef.current = false; // new session starts unpersisted
        currentRecordIdRef.current = null; // clear stale record id
        cancelledRef.current = false;
        forcedAbortRef.current = false;
        sessionIdRef.current += 1;
        setIsPostRecording(false);
        setIsOptimizing(false);
        setOptimizedText("");
        recordingStartTimeRef.current = Date.now();
        playStartSound();
        if (settingsRef.current.show_overlay) showOverlay();
        emitOverlayPhase("recording");
      } else if (wasRecording) {
        // If asr-error already handled cleanup, skip duplicate post-recording flow
        if (errorHandledRef.current) {
          errorHandledRef.current = false;
          return;
        }
        // [修订 R3] Mistouch cancel — user explicitly discarded this session.
        // Must clear all text state AND mark persisted=true to prevent the
        // next new-session rescue block from saving the discarded text.
        if (cancelledRef.current) {
          cancelledRef.current = false;
          setTranscript("");
          setInterimTextBoth("");
          transcriptRef.current = "";
          setOptimizedText("");
          persistedRef.current = true; // "handled" — rescue will skip
          emitOverlayPhase("idle");
          closeOverlay();
          // Release the backend is_processing gate — the mistouch flow
          // doesn't reach doAutoInput, so without this the backend would
          // wait 65 seconds for the safety timeout.
          invoke("mark_session_idle", { generation }).catch(() => {});
          return;
        }
        playStopSound();
        setIsPostRecording(true);
        emitOverlayPhase("processing");
        // Capture the generation of the session that just stopped — thread
        // it through every deferred callback below so any late call to
        // `mark_session_idle` is tagged with THIS session's gen, not
        // whatever the ref happens to hold when the callback fires.
        // Without this, a late call could wrongly clear the gate of a
        // later session that happens to be in its own processing window.
        const sessionGeneration = generation;

        // ── [Batch 2] Phased post-recording timers ────────────────────
        // Two timers replace the old single 2s fallback:
        //
        //   3s  → soft hint: UI switches "识别中" → "正在努力识别中"
        //         so the user sees the app is still working on a slow
        //         network, instead of wondering if it died.
        //   6s  → force advance: promote interim → transcript and run
        //         doAutoInput, same fallback logic as the old 2s timer.
        //         By this point whatever Doubao gave us in first-pass
        //         is already persisted by doAutoInput Step 1, so even
        //         if FINAL never arrives the content is safe.
        //
        // The 6s must be LESS than the ASR provider's own internal
        // timeout (doubao/dashscope/qwen all ~10s) so the frontend
        // force-advances before the backend gives up — avoids double
        // error paths racing.
        const SLOW_HINT_MS = 3000;
        const FORCE_ADVANCE_MS = 6000;

        slowHintTimerRef.current = window.setTimeout(() => {
          slowHintTimerRef.current = null;
          if (textSentRef.current) return; // FINAL already handled it
          setIsProcessingSlow(true);
          emitOverlaySlow(true);
        }, SLOW_HINT_MS);

        // Don't hide overlay yet — keep showing during ASR processing / AI optimization
        // Recording stopped — wait for FINAL from ASR before auto-inputting.
        // The ASR sends a FINAL event after the end packet (typically 0.5-1.5s).
        // If FINAL arrives, the transcription-update handler will trigger auto-input.
        // Fallback: if FINAL doesn't arrive within 6s, promote interim and send.
        autoInputTimerRef.current = window.setTimeout(() => {
          autoInputTimerRef.current = null;
          if (textSentRef.current) return; // FINAL already handled it
          // Promote interim → transcript (both state and ref stay in sync)
          const pending = interimTextRef.current;
          if (pending) {
            setTranscript((t) => {
              const next = (t ? t + "\n" : "") + pending;
              transcriptRef.current = next;
              return next;
            });
            setInterimTextBoth("");
          }
          setTimeout(() => {
            if (textSentRef.current) return;
            const text = transcriptRef.current;
            if (text) {
              doAutoInput(text, sessionGeneration);
            } else {
              // No text at all (e.g. stop pressed before any speech detected).
              // Must release the backend is_processing gate here — doAutoInput
              // is the only other place that calls mark_session_idle, and
              // we're skipping it. Without this, is_processing stays true
              // until the 65s safety timeout, causing "session busy" rejects
              // on every new hotkey press during that window.
              setIsProcessingSlow(false);
              emitOverlaySlow(false);
              emitOverlayPhase("idle");
              closeOverlay();
              invoke("mark_session_idle", {
                generation: sessionGeneration,
              }).catch(() => {});
            }
          }, 100);
        }, FORCE_ADVANCE_MS);
      }
      }),

      listen<TranscriptPayload>("transcription-update", (e) => {
      // [修订 R4] Self-healing generation filter.
      // Tauri cross-thread emit doesn't guarantee strict ordering —
      // transcription-update may arrive before the recording-status event
      // that would have updated backendGenerationRef. So:
      //   - gen < current : stale-session late arrival → drop
      //   - gen > current : recording-status hasn't arrived yet → self-heal
      //   - gen == current : normal, accept
      const gen = e.payload.generation;
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

        // If recording already stopped, cancel fallback timer and send text now
        if (!isRecordingRef.current && !textSentRef.current) {
          // FINAL arrived — cancel BOTH the soft-hint timer and the
          // force-advance timer, and reset the slow-hint state if it
          // had already fired before FINAL came in.
          clearPostRecordingTimers();
          // Use the generation from THIS event payload — that's the
          // session that produced this FINAL, regardless of what
          // backendGenerationRef has drifted to since.
          const finalGen = gen;
          setTimeout(() => {
            const text = transcriptRef.current;
            if (text) {
              doAutoInput(text, finalGen);
            } else {
              // FINAL arrived but with empty text — same release-the-gate
              // requirement as the timer fallback below: doAutoInput is
              // skipped, so we must call mark_session_idle ourselves or
              // is_processing stays true until the 65s safety timeout.
              emitOverlayPhase("idle");
              closeOverlay();
              invoke("mark_session_idle", { generation: finalGen }).catch(() => {});
            }
          }, 100);
        }
      } else {
        setInterimTextBoth(data.text);
      }
      }),

      listen<string>("asr-error", (e) => {
        if (forcedAbortRef.current) return;
        errorHandledRef.current = true; // Prevent duplicate post-recording from recording-status(false)
        playErrorSound();
        if (errorTimerRef.current !== null) window.clearTimeout(errorTimerRef.current);
        setHasError(true);
        errorTimerRef.current = window.setTimeout(() => {
          setHasError(false);
          errorTimerRef.current = null;
        }, 3000);
        const errorText = settingsRef.current.debug_mode
          ? e.payload
          : simplifyError(e.payload);
        setBanner({ kind: "error", text: errorText });
        // Mark persisted=true so any pending text from this failed session
        // isn't rescue-saved on the next recording attempt.
        persistedRef.current = true;
        // Clear 3s/6s post-recording timers in case error fired after stop.
        clearPostRecordingTimers();
        emitOverlayPhase("idle");
        closeOverlay();
        // Release the backend is_processing gate in case stop was called
        // before the error fired (do_stop_recording_impl would have set it).
        invoke("mark_session_idle", {
          generation: backendGenerationRef.current,
        }).catch(() => {});
      }),

      // Backend rejected a start-recording attempt because a previous
      // session is still wrapping up (is_processing=true). Show a brief toast.
      // showToast already replaces any currently-visible toast, so rapid
      // repeated rejections just refresh the same single toast — no stacking.
      listen<string>("session-busy", (e) => {
        showToast(e.payload);
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
          const entry: LogEntry = JSON.parse(e.payload);
          setLogs((prev) => [...prev.slice(-200), entry]);
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
    ];

    Promise.all(listenerPromises).then((fns) => {
      unlisteners.push(...fns);
      invoke("emit_pending_settings_warning").catch(() => {});
    });

    return () => { unlisteners.forEach((fn) => fn()); };
  }, [closeOverlay, doAutoInput, emitOverlayPhase, handleForcedSessionAbort, setInterimTextBoth, showToast]);

  const handleToggleRecording = useCallback(async () => {
    // Ignore if ASR/AI processing is in progress (prevent rapid re-triggering)
    if (!isRecording && (isPostRecording || isOptimizing)) return;
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
  }, [isRecording, isPostRecording, isOptimizing, settings]);

  // Pending settings: saved to backend immediately, but App state deferred
  // until panel closes to avoid re-render flash behind backdrop-blur.
  const pendingSettingsRef = useRef<AppSettings | null>(null);

  const handleSaveSettings = useCallback(async (newSettings: AppSettings) => {
    try {
      setBanner(null);
      const savedSettings = await invoke<AppSettings>("save_settings", { settings: newSettings });
      pendingSettingsRef.current = savedSettings;
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
          handleToggleRecording();
        }
      } else {
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
      if (Date.now() - jsHoldStartTime < MIN_HOLD_MS) {
        cancelledRef.current = true;
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
          cancelledRef.current = true;
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
  }, [handleToggleRecording]);

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
                <Settings settings={settings} onSave={handleSaveSettings} onClose={handleCloseSettings} initialTab={settingsInitialTab} />
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
