import { useState, useEffect, useCallback, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { AnimatePresence, motion } from "motion/react";
import { VoicePanel } from "./components/VoicePanel";
import { Settings } from "./components/Settings";
import { NetworkLog } from "./components/NetworkLog";
import { TitleBar } from "./components/TitleBar";
import { ThemeToggle } from "./components/ThemeToggle";
import { Settings as SettingsIcon } from "lucide-react";
import { cn } from "./lib/utils";
import { Tooltip } from "./components/ui/Tooltip";
import { playStartSound, playStopSound } from "./lib/sounds";
import { showOverlay, hideOverlay } from "./lib/overlay";

export interface AppSettings {
  app_id: string;
  access_token: string;
  resource_id: string;
  hotkey: string;
  input_mode: "toggle" | "hold";
  device_name: string;
  output_mode: "none" | "paste" | "type";
  mic_always_on: boolean;
}

interface TranscriptPayload {
  text: string;
  is_final: boolean;
}

export interface LogEntry {
  ts: string;
  level: string;
  msg: string;
}

const DEFAULT_SETTINGS: AppSettings = {
  app_id: "",
  access_token: "",
  resource_id: "volc.bigasr.sauc.duration",
  hotkey: "Ctrl+Shift+V",
  input_mode: "toggle",
  device_name: "",
  output_mode: "none",
  mic_always_on: true,
};

export default function App() {
  const [isRecording, setIsRecording] = useState(false);
  const [transcript, setTranscript] = useState("");
  const [interimText, setInterimText] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState<AppSettings>(DEFAULT_SETTINGS);
  const [error, setError] = useState<string | null>(null);
  const [isConnected, setIsConnected] = useState(false);
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const isRecordingRef = useRef(false);
  const transcriptRef = useRef("");

  useEffect(() => {
    invoke<AppSettings>("get_settings")
      .then((s) => { if (s) setSettings(s); })
      .catch(() => {});
  }, []);

  useEffect(() => {
    const unlisteners: (() => void)[] = [];

    listen<boolean>("recording-status", (e) => {
      const wasRecording = isRecordingRef.current;
      isRecordingRef.current = e.payload;
      setIsRecording(e.payload);
      if (e.payload) {
        // New recording session
        setError(null);
        setTranscript("");
        setInterimText("");
        transcriptRef.current = "";
        playStartSound();
        showOverlay();
      } else if (wasRecording) {
        playStopSound();
        hideOverlay();
        // Recording just stopped — promote any remaining interim text to final,
        // then auto-input. This ensures nothing stays gray after stop.
        setTimeout(() => {
          setInterimText((prev) => {
            if (prev) {
              setTranscript((t) => {
                const next = (t ? t + "\n" : "") + prev;
                transcriptRef.current = next;
                return next;
              });
            }
            return "";
          });
          // Small extra delay after promotion for state to settle
          setTimeout(() => {
            const text = transcriptRef.current;
            if (text) {
              invoke("send_text_input", { text }).catch(() => {});
            }
          }, 100);
        }, 500);
      }
    }).then((fn) => unlisteners.push(fn));

    listen<TranscriptPayload>("transcription-update", (e) => {
      const data = e.payload;
      if (data.is_final) {
        setTranscript((prev) => {
          const next = (prev ? prev + "\n" : "") + data.text;
          transcriptRef.current = next;
          return next;
        });
        setInterimText("");
      } else {
        setInterimText(data.text);
      }
    }).then((fn) => unlisteners.push(fn));

    listen<string>("asr-error", (e) => {
      setError(e.payload);
    }).then((fn) => unlisteners.push(fn));

    listen<boolean>("connection-status", (e) => {
      setIsConnected(e.payload);
    }).then((fn) => unlisteners.push(fn));

    listen<string>("network-log", (e) => {
      try {
        const entry: LogEntry = JSON.parse(e.payload);
        setLogs((prev) => [...prev.slice(-200), entry]);
      } catch {}
    }).then((fn) => unlisteners.push(fn));

    return () => { unlisteners.forEach((fn) => fn()); };
  }, []);

  const handleToggleRecording = useCallback(async () => {
    try {
      setError(null);
      if (isRecording) {
        await invoke("stop_recording");
      } else {
        if (!settings.app_id || !settings.access_token) {
          setError("请先在设置中配置 App ID 和 Access Token");
          setSettingsOpen(true);
          return;
        }
        await invoke("start_recording");
      }
    } catch (e) {
      setError(String(e));
    }
  }, [isRecording, settings]);

  // Pending settings: saved to backend immediately, but App state deferred
  // until panel closes to avoid re-render flash behind backdrop-blur.
  const pendingSettingsRef = useRef<AppSettings | null>(null);

  const handleSaveSettings = useCallback(async (newSettings: AppSettings) => {
    try {
      await invoke("save_settings", { settings: newSettings });
      pendingSettingsRef.current = newSettings;
    } catch (e) {
      setError(String(e));
    }
  }, []);

  const handleCopy = useCallback(() => {
    const fullText = transcript + (interimText ? "\n" + interimText : "");
    if (fullText) navigator.clipboard.writeText(fullText);
  }, [transcript, interimText]);

  const handleClear = useCallback(() => {
    setTranscript("");
    setInterimText("");
    transcriptRef.current = "";
  }, []);

  const handleCloseSettings = useCallback(() => {
    setSettingsOpen(false);
    // Apply pending settings after panel close animation completes
    if (pendingSettingsRef.current) {
      setSettings(pendingSettingsRef.current);
      pendingSettingsRef.current = null;
    }
  }, []);

  return (
    <div className="flex flex-col h-screen bg-surface overflow-hidden">
      {/* Title Bar */}
      <TitleBar>
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

      {/* Main Content — top-aligned fixed layout, no centering to prevent jumps */}
      <main className="flex-1 flex flex-col px-5 py-5 gap-4 overflow-hidden min-h-0">
        <div className="shrink-0 flex justify-center">
          <VoicePanel isRecording={isRecording} onToggle={handleToggleRecording} settings={settings} />
        </div>

        {/* Error — smooth expand/collapse */}
        <div className={cn(
          "w-full grid shrink-0 transition-all duration-[var(--t-base)]",
          error ? "grid-rows-[1fr] opacity-100" : "grid-rows-[0fr] opacity-0"
        )}>
          <div className="overflow-hidden">
            <div className="px-4 py-3 rounded-xl bg-danger-muted text-danger-muted-fg text-sm select-text">
              {error}
            </div>
          </div>
        </div>

        {/* Transcript — fills remaining space */}
        <div className="w-full flex-1 min-h-0 flex flex-col">
          <div className="flex items-center justify-between mb-2 shrink-0">
            <span className="text-xs font-medium text-fg-3 uppercase tracking-widest">转写结果</span>
            <div className="flex gap-1">
              <button onClick={handleCopy} disabled={!transcript && !interimText}
                className={cn("px-3 py-1 text-xs font-medium rounded-md transition-all duration-[var(--t-fast)]",
                  "text-fg-2 hover:text-fg bg-surface-subtle hover:bg-surface-inset active:scale-95",
                  "disabled:opacity-40 disabled:cursor-not-allowed")}>
                复制
              </button>
              <button onClick={handleClear} disabled={!transcript && !interimText}
                className={cn("px-3 py-1 text-xs font-medium rounded-md transition-all duration-[var(--t-fast)]",
                  "text-fg-2 hover:text-fg bg-surface-subtle hover:bg-surface-inset active:scale-95",
                  "disabled:opacity-40 disabled:cursor-not-allowed")}>
                清除
              </button>
            </div>
          </div>
          <div className={cn(
            "flex-1 min-h-0 rounded-xl p-4 overflow-y-auto",
            "bg-surface-card border border-edge select-text",
            "transition-colors duration-[var(--t-base)]"
          )}>
            {transcript || interimText ? (
              <div className="text-sm leading-relaxed whitespace-pre-wrap">
                {transcript && <span className="text-fg">{transcript}</span>}
                {interimText && (
                  <span className="text-fg-3 italic">
                    {transcript ? "\n" : ""}{interimText}
                  </span>
                )}
              </div>
            ) : (
              <div className="flex items-center justify-center h-full">
                <p className="text-fg-3 text-sm">
                  {isRecording ? "正在聆听..." : "按下热键或点击麦克风开始录音"}
                </p>
              </div>
            )}
          </div>
        </div>

        {/* Status Bar */}
        <div className="flex items-center justify-center gap-3 text-xs text-fg-3 shrink-0 select-text">
          <span className="flex items-center gap-1.5">
            <span className={cn("w-1.5 h-1.5 rounded-full transition-colors", isConnected ? "bg-ok" : "bg-fg-3")} />
            {isConnected ? "已连接" : "未连接"}
          </span>
          <span className="text-edge-strong">|</span>
          <span>热键: {settings.hotkey}</span>
          <span className="text-edge-strong">|</span>
          <span>{settings.input_mode === "toggle" ? "切换模式" : "按住模式"}</span>
        </div>
      </main>

      {/* Network Log Panel */}
      <NetworkLog logs={logs} onClear={() => setLogs([])} />

      {/* Settings Panel Overlay — AnimatePresence for flicker-free transitions */}
      <AnimatePresence>
        {settingsOpen && (
          <>
            {/* Backdrop: absolute overlay with blur */}
            <motion.div
              key="settings-backdrop"
              className="absolute inset-0 z-50 bg-black/20 backdrop-blur-sm"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={{ duration: 0.2 }}
              onClick={handleCloseSettings}
            />
            {/* Panel: slides from right, positioned absolutely to avoid affecting layout */}
            <motion.div
              key="settings-panel"
              className="absolute top-0 right-0 bottom-0 z-50 w-[320px] bg-surface-card border-l border-edge"
              initial={{ x: "100%" }}
              animate={{ x: 0 }}
              exit={{ x: "100%" }}
              transition={{ type: "spring", damping: 30, stiffness: 350 }}
            >
              <Settings settings={settings} onSave={handleSaveSettings} onClose={handleCloseSettings} />
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}
