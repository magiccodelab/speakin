import { cn } from "../lib/utils";
import { Mic, MicOff } from "lucide-react";
import type { AppSettings } from "../App";

interface VoicePanelProps {
  isRecording: boolean;
  onToggle: () => void;
  settings: AppSettings;
}

export function VoicePanel({ isRecording, onToggle, settings }: VoicePanelProps) {
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
          className={cn(
            "relative z-10 w-20 h-20 rounded-full flex items-center justify-center",
            "transition-all duration-[var(--t-slow)]",
            "focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface",
            isRecording
              ? "bg-recording text-white shadow-[0_0_32px_-4px_hsl(var(--recording)/0.5)] active:scale-95"
              : "bg-primary text-primary-fg shadow-lg hover:shadow-[0_0_24px_-4px_hsl(var(--primary)/0.4)] hover:-translate-y-0.5 active:scale-95 active:shadow-md"
          )}
          style={
            isRecording
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

      {/* Audio Visualizer — always rendered, opacity-controlled to prevent layout jump */}
      <div className={cn(
        "flex items-center gap-1 h-8 transition-all duration-[var(--t-base)]",
        isRecording ? "opacity-100 scale-100" : "opacity-0 scale-95 pointer-events-none"
      )}>
        {Array.from({ length: 5 }).map((_, i) => (
          <div
            key={i}
            className="w-1 rounded-full"
            style={{
              backgroundColor: "hsl(var(--recording))",
              animation: isRecording ? `wave 1.2s ease-in-out ${i * 0.15}s infinite` : "none",
              height: "8px",
            }}
          />
        ))}
      </div>

      {/* Status Text */}
      <p className={cn(
        "text-sm transition-colors duration-[var(--t-base)]",
        isRecording ? "text-fg font-medium" : "text-fg-3"
      )}>
        {isRecording ? (
          <span className="flex items-center gap-2">
            <span className="w-2 h-2 rounded-full animate-pulse" style={{ backgroundColor: "hsl(var(--recording))" }} />
            正在录音...
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
