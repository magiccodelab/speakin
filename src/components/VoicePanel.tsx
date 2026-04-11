import { useEffect, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import { cn } from "../lib/utils";
import { Mic, MicOff, Loader2, Sparkles } from "lucide-react";
import type { AppSettings } from "../App";
import { OceanWave } from "./OceanWave";

// Height of the ocean-wave visualizer below the mic button. Tall enough
// for the ribbon to look prominent, compact enough to keep the panel
// visually centered.
const VISUALIZER_HEIGHT = 44;

interface VoicePanelProps {
  isRecording: boolean;
  isProcessing: boolean;
  /** True after 3s of waiting for ASR FINAL — switches label to "正在努力识别中" */
  isProcessingSlow?: boolean;
  isOptimizing: boolean;
  hasError: boolean;
  /**
   * Short contextual label shown when `hasError` is true. Defaults to
   * "识别失败" when null so the status row never goes blank on errors.
   */
  errorLabel?: string | null;
  onToggle: () => void;
  settings: AppSettings;
}

export function VoicePanel({ isRecording, isProcessing, isProcessingSlow, isOptimizing, hasError, errorLabel, onToggle, settings }: VoicePanelProps) {
  const levelRef = useRef(0);

  // Listen to real-time audio level (written to the ref, not state — the
  // OceanWave component's RAF loop reads from this ref directly, so we
  // avoid a React re-render on every audio-level event).
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<number>("audio-level", (e) => {
      levelRef.current = e.payload;
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  // Zero the level ref whenever we leave the recording state so the ocean
  // wave's internal smoothing decays back to baseline during processing /
  // optimizing / error. Baseline amp is still non-zero, so the visualizer
  // stays subtly animated even at level=0 — signalling "still doing work".
  useEffect(() => {
    if (!isRecording) {
      levelRef.current = 0;
    }
  }, [isRecording]);

  const showVisualizer = isRecording || isProcessing || isOptimizing || hasError;

  // One color source for both the button accents and the ocean wave ribbon.
  const waveColor = hasError
    ? "hsl(var(--danger))"
    : isRecording
      ? "hsl(var(--recording))"
      : "hsl(var(--primary))";

  return (
    <div className="flex flex-col items-center gap-4 shrink-0 w-full">
      {/* Mic Button */}
      <div className="relative">
        {/* Pulse rings — always rendered, opacity-controlled */}
        <div
          className={cn(
            "absolute inset-0 rounded-full transition-opacity duration-[var(--t-base)]",
            isRecording ? "opacity-20" : "opacity-0"
          )}
          style={{
            animation: isRecording ? "pulse-ring 1.5s cubic-bezier(0, 0, 0.2, 1) infinite" : "none",
            backgroundColor: "hsl(var(--recording))",
          }}
        />
        <div
          className={cn(
            "absolute inset-0 rounded-full transition-opacity duration-[var(--t-base)]",
            isRecording ? "opacity-15" : "opacity-0"
          )}
          style={{
            animation: isRecording ? "pulse-ring 1.5s cubic-bezier(0, 0, 0.2, 1) infinite 0.4s" : "none",
            backgroundColor: "hsl(var(--recording))",
          }}
        />

        <button
          onClick={onToggle}
          disabled={isProcessing || isOptimizing}
          className={cn(
            "relative z-10 w-20 h-20 rounded-full flex items-center justify-center",
            "transition-all duration-[var(--t-slow)]",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface",
            (isProcessing || isOptimizing)
              ? "bg-surface-2 text-fg-3 cursor-not-allowed"
              : isRecording
                ? "bg-recording text-white shadow-[0_0_32px_-4px_hsl(var(--recording)/0.5)] active:scale-95"
                : "bg-primary text-primary-fg shadow-lg hover:shadow-[0_0_24px_-4px_hsl(var(--primary)/0.4)] hover:-translate-y-0.5 active:scale-95 active:shadow-md"
          )}
          style={
            isRecording && !(isProcessing || isOptimizing)
              ? { backgroundColor: "hsl(var(--recording))" }
              : undefined
          }
        >
          {isRecording ? (
            <MicOff size={28} className="animate-mic-breathe" />
          ) : (
            <Mic size={28} />
          )}
        </button>
      </div>

      {/* Ocean wave visualizer — the sole audio indicator on the main page.
          When idle, the container's height collapses to 0 so the status
          text below moves up smoothly into the freed space (flex reflow
          interpolates the layout for us during the height transition).
          This is how the "按热键或点击麦克风" hint visibly rises closer to
          the mic button instead of sitting low with a dead gap above it. */}
      <div
        className={cn(
          "relative w-full overflow-hidden transition-all duration-[var(--t-base)]",
          showVisualizer
            ? "opacity-100 scale-100"
            : "opacity-0 scale-95 pointer-events-none"
        )}
        style={{ height: showVisualizer ? VISUALIZER_HEIGHT : 0 }}
      >
        <OceanWave
          levelRef={levelRef}
          active={showVisualizer}
          color={waveColor}
          className="absolute inset-0 w-full h-full"
          viewHeight={VISUALIZER_HEIGHT}
        />
      </div>

      {/* Status Text */}
      <p className={cn(
        "text-sm transition-colors duration-[var(--t-base)]",
        hasError ? "text-danger font-medium" : (isRecording || isProcessing || isOptimizing) ? "text-fg font-medium" : "text-fg-3"
      )}>
        {hasError ? (
          <span className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-danger" />
            {errorLabel ?? "识别失败"}
          </span>
        ) : isOptimizing ? (
          <span className="flex items-center gap-2">
            <Sparkles size={14} className="text-primary animate-pulse" />
            AI 优化中
          </span>
        ) : isProcessing ? (
          <span className="flex items-center gap-2">
            <Loader2 size={14} className="text-primary animate-spin" />
            {isProcessingSlow ? "正在努力识别中…" : "识别中"}
          </span>
        ) : isRecording ? (
          <span className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full animate-pulse" style={{ backgroundColor: "hsl(var(--recording))" }} />
            正在录音
          </span>
        ) : (
          <>
            按 <kbd className="px-1.5 py-0.5 text-xs font-mono bg-surface-subtle rounded border border-edge">{settings.hotkey}</kbd> 或点击麦克风
          </>
        )}
      </p>
    </div>
  );
}
