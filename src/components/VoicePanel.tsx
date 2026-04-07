import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { cn } from "../lib/utils";
import { Mic, MicOff, Loader2, Sparkles } from "lucide-react";
import type { AppSettings } from "../App";

const BAR_COUNT = 7;
const MIN_H = 4;
const MAX_H = 28;
const BAR_WEIGHTS = [0.45, 0.65, 0.85, 1.0, 0.85, 0.65, 0.45];

interface VoicePanelProps {
  isRecording: boolean;
  isProcessing: boolean;
  /** True after 3s of waiting for ASR FINAL — switches label to "正在努力识别中" */
  isProcessingSlow?: boolean;
  isOptimizing: boolean;
  hasError: boolean;
  onToggle: () => void;
  settings: AppSettings;
}

export function VoicePanel({ isRecording, isProcessing, isProcessingSlow, isOptimizing, hasError, onToggle, settings }: VoicePanelProps) {
  const [heights, setHeights] = useState<number[]>(() => Array(BAR_COUNT).fill(MIN_H));
  const levelRef = useRef(0);
  const smoothRef = useRef(0);
  const rafRef = useRef(0);

  // Listen to real-time audio level
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<number>("audio-level", (e) => {
      levelRef.current = e.payload;
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  // Animate bars based on audio level (~30fps)
  useEffect(() => {
    if (!isRecording) {
      smoothRef.current = 0;
      levelRef.current = 0;
      setHeights(Array(BAR_COUNT).fill(MIN_H));
      return;
    }
    let running = true;
    const animate = () => {
      if (!running) return;
      const target = levelRef.current;
      const alpha = target > smoothRef.current ? 0.35 : 0.1;
      smoothRef.current += (target - smoothRef.current) * alpha;
      const level = smoothRef.current;
      setHeights(BAR_WEIGHTS.map(
        (w) => MIN_H + (MAX_H - MIN_H) * Math.min(level * w, 1)
      ));
      rafRef.current = requestAnimationFrame(animate);
    };
    rafRef.current = requestAnimationFrame(animate);
    return () => { running = false; cancelAnimationFrame(rafRef.current); };
  }, [isRecording]);

  const showVisualizer = isRecording || isProcessing || isOptimizing || hasError;

  return (
    <div className="flex flex-col items-center gap-4 shrink-0">
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

      {/* Audio Visualizer — real-time when recording, gentle CSS animation for other states */}
      <div className={cn(
        "flex items-center gap-[3px] transition-all duration-[var(--t-fast)]",
        showVisualizer ? "opacity-100 scale-100" : "opacity-0 scale-95 pointer-events-none"
      )} style={{ height: MAX_H }}>
        {Array.from({ length: BAR_COUNT }).map((_, i) => (
          <div
            key={i}
            className="w-1 rounded-full"
            style={{
              backgroundColor: hasError
                ? "hsl(var(--danger))"
                : isRecording
                  ? "hsl(var(--recording))"
                  : "hsl(var(--primary))",
              height: isRecording
                ? heights[i]
                : MIN_H,
              transition: isRecording
                ? "height 60ms ease-out, background-color 300ms"
                : "height 300ms ease-in-out, background-color 300ms",
              animation: hasError
                ? `wave-gentle 2s ease-in-out ${i * 0.15}s infinite`
                : !isRecording && (isProcessing || isOptimizing)
                  ? isOptimizing
                    ? `wave-gentle 2.5s ease-in-out ${i * 0.18}s infinite`
                    : `wave-medium 1.8s ease-in-out ${i * 0.15}s infinite`
                  : "none",
            }}
          />
        ))}
      </div>

      {/* Status Text */}
      <p className={cn(
        "text-sm transition-colors duration-[var(--t-base)]",
        hasError ? "text-danger font-medium" : (isRecording || isProcessing || isOptimizing) ? "text-fg font-medium" : "text-fg-3"
      )}>
        {hasError ? (
          <span className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full bg-danger" />
            识别失败
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
