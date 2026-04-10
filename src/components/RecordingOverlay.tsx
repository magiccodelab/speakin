/**
 * System-level recording indicator overlay (transparent, always on top).
 *
 * ADAPTIVE LAYOUT:
 *   ┌──────────────────────────────────┐
 *   │ subtitle bubble grows to 3 lines │ ← then scrolls inside the bubble
 *   │ using the space above the pill   │
 *   ├──────────────────────────────────┤
 *   │      ● ▎▌█▌▎                    │ ← draggable pill, hover to move
 *   └──────────────────────────────────┘
 *
 * State is driven entirely by the `overlay-phase` event from App.tsx
 * (single source of truth). The overlay never independently tracks
 * recording/optimizing state — it only responds to phase transitions.
 *
 * Position: draggable via the pill area. Position is persisted in
 * localStorage. Right-click the pill to reset to default bottom-center.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { currentMonitor } from "@tauri-apps/api/window";
import { LogicalPosition } from "@tauri-apps/api/dpi";
import { OVERLAY_WIDTH, OVERLAY_HEIGHT, BOTTOM_MARGIN, OVERLAY_POSITION_KEY } from "../lib/overlay";
import "../index.css";

const BAR_COUNT = 7;
const MIN_H = 3;
const MAX_H = 16;
const BAR_WEIGHTS = [0.45, 0.65, 0.85, 1.0, 0.85, 0.65, 0.45];

const SUBTITLE_LINE_COUNT = 3;
const SUBTITLE_FONT_SIZE = 13;
// Use an integer line-height grid so auto-scroll lands on full-line boundaries.
// A fractional 13px * 1.6 = 20.8px line-height can leave a partial extra line visible.
const SUBTITLE_LINE_HEIGHT_PX = 21;
const SUBTITLE_PADDING_Y = 6;
const SUBTITLE_BORDER_WIDTH = 1;
const SUBTITLE_VISIBLE_TEXT_MAX = SUBTITLE_LINE_COUNT * SUBTITLE_LINE_HEIGHT_PX;
const SUBTITLE_BUBBLE_MAX =
  SUBTITLE_VISIBLE_TEXT_MAX + SUBTITLE_PADDING_Y * 2 + SUBTITLE_BORDER_WIDTH * 2;
interface TranscriptPayload {
  text: string;
  is_final: boolean;
  generation: number;
}

type OverlayPhase = "recording" | "processing" | "optimizing" | "error" | "idle";

// Sync theme & color BEFORE first render
(() => {
  const stored = localStorage.getItem("theme");
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const isDark = stored ? stored === "dark" : prefersDark;
  document.documentElement.classList.toggle("dark", isDark);

  // Theme color (read localStorage cache)
  const id = localStorage.getItem("theme-color-id");
  if (id) {
    const hsl = localStorage.getItem(isDark ? "theme-color-dark" : "theme-color-light");
    if (hsl) {
      if (id !== "blue") {
        document.documentElement.style.setProperty("--primary", hsl);
      }
      if (localStorage.getItem("theme-recording-follows") === "1") {
        document.documentElement.style.setProperty("--recording", hsl);
        document.documentElement.style.setProperty("--recording-pulse", hsl);
      }
    }
  }
})();

function readSubtitlePref(): boolean {
  return localStorage.getItem("overlay-subtitle") !== "0";
}

export function RecordingOverlay() {
  const [heights, setHeights] = useState<number[]>(() => Array(BAR_COUNT).fill(MIN_H));
  const [committed, setCommitted] = useState("");
  const [interim, setInterim] = useState("");
  const [phase, setPhase] = useState<OverlayPhase>("recording");
  const [slow, setSlow] = useState(false);
  const [subtitleEnabled, setSubtitleEnabled] = useState(readSubtitlePref);
  // Latest known session id (from overlay-phase events — the app-window
  // increments its sessionId on each new recording and sends it in
  // overlay-phase + overlay-slow). Used to drop stale events.
  const sessionRef = useRef(0);
  // Latest known backend generation (from transcription-update events).
  // The overlay window runs in its own process and doesn't share state
  // with App.tsx, so it needs an independent self-healing filter here.
  const backendGenRef = useRef(0);
  const scrollRef = useRef<HTMLDivElement>(null);
  const levelRef = useRef(0);
  const rafRef = useRef(0);
  const smoothRef = useRef(0);

  // Derive display booleans from phase
  const isRecording = phase === "recording";
  const isProcessing = phase === "processing" || phase === "optimizing";
  const isOptimizing = phase === "optimizing";
  const isError = phase === "error";

  const displayText = committed + (interim ? (committed ? " " : "") + interim : "");

  // Auto-scroll to bottom when text changes
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [committed, interim]);

  // Audio level
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<number>("audio-level", (e) => {
      levelRef.current = e.payload;
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  // Transcription data + unified overlay state
  useEffect(() => {
    const fns: (() => void)[] = [];

    listen<TranscriptPayload>("transcription-update", (e) => {
      // [Codex Q3a fix] Drop late events from previous sessions — without
      // this, a slow FINAL from session N can append to session N+1's
      // subtitle bubble because the overlay window has its own listener
      // independent of App.tsx's filter.
      const gen = e.payload.generation;
      if (gen < backendGenRef.current) return;
      if (gen > backendGenRef.current) backendGenRef.current = gen;
      const { text, is_final } = e.payload;
      if (is_final) {
        setCommitted((prev) => (prev ? prev + " " : "") + text);
        setInterim("");
      } else {
        setInterim(text);
      }
    }).then((fn) => fns.push(fn));

    // Single unified state listener — replaces recording-status + ai-optimize-started
    listen<{ phase: OverlayPhase; sessionId: number }>("overlay-phase", (e) => {
      const { phase: newPhase, sessionId } = e.payload;
      // Ignore stale events from old sessions
      if (sessionId < sessionRef.current) return;
      sessionRef.current = sessionId;
      setPhase(newPhase);
      if (newPhase === "recording") {
        // New recording started — reset text and slow hint.
        // Note: we do NOT reset backendGenRef here because
        // transcription-update and overlay-phase are independent event
        // streams. The next transcription-update with a larger
        // generation will self-heal the filter.
        setCommitted("");
        setInterim("");
        setSlow(false);
        setSubtitleEnabled(readSubtitlePref());
      } else if (newPhase === "idle") {
        // Defensive: make sure slow doesn't linger into the next phase
        setSlow(false);
      }
    }).then((fn) => fns.push(fn));

    // [Batch 2 + Codex Check 5 fix] Slow-hint signal from App.tsx —
    // true at 3s into post-recording, false on FINAL / force-advance /
    // new session. Filtered by sessionId so a stale `slow=true` from
    // a previous session can never sneak in after a reset.
    listen<{ slow: boolean; sessionId: number }>("overlay-slow", (e) => {
      if (e.payload.sessionId < sessionRef.current) return;
      setSlow(e.payload.slow);
    }).then((fn) => fns.push(fn));

    return () => fns.forEach((fn) => fn());
  }, []);

  // Track window position changes → debounced save to localStorage
  useEffect(() => {
    let timer: number | undefined;
    let seq = 0; // Monotonic sequence to discard stale async saves
    let unlisten: (() => void) | undefined;
    const win = getCurrentWindow();
    win.onMoved(async (e) => {
      if (timer !== undefined) clearTimeout(timer);
      const thisSeq = ++seq;
      timer = window.setTimeout(async () => {
        const scale = await win.scaleFactor();
        if (thisSeq !== seq) return; // Newer move arrived — discard stale save
        const logicalX = Math.round(e.payload.x / scale);
        const logicalY = Math.round(e.payload.y / scale);
        localStorage.setItem(OVERLAY_POSITION_KEY, JSON.stringify({ x: logicalX, y: logicalY }));
      }, 300);
    }).then((fn) => { unlisten = fn; });
    return () => {
      if (timer !== undefined) clearTimeout(timer);
      unlisten?.();
    };
  }, []);

  // Right-click pill → reset to default position
  const handleContextMenu = async (e: React.MouseEvent) => {
    e.preventDefault();
    localStorage.removeItem(OVERLAY_POSITION_KEY);
    const monitor = await currentMonitor();
    const screenWidth = monitor?.size.width ?? 1920;
    const screenHeight = monitor?.size.height ?? 1080;
    const scaleFactor = monitor?.scaleFactor ?? 1;
    const logicalWidth = screenWidth / scaleFactor;
    const logicalHeight = screenHeight / scaleFactor;
    const x = Math.round((logicalWidth - OVERLAY_WIDTH) / 2);
    const y = Math.round(logicalHeight - OVERLAY_HEIGHT - BOTTOM_MARGIN);
    await getCurrentWindow().setPosition(new LogicalPosition(x, y));
  };

  // Wave animation ~30fps (only when actively recording)
  useEffect(() => {
    if (!isRecording) {
      smoothRef.current = 0;
      levelRef.current = 0;
      return;
    }
    let running = true;
    const animate = () => {
      if (!running) return;
      const target = levelRef.current;
      const alpha = target > smoothRef.current ? 0.35 : 0.08;
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

  const barColor = isError
    ? "hsl(var(--danger))"
    : isRecording
      ? "hsl(var(--recording))"
      : "hsl(var(--primary))";

  return (
    <div className="w-full h-screen flex flex-col items-center select-none">
      {/* ── Subtitle: uses remaining height, bubble caps at 3 lines ── */}
      <div
        className="w-full flex-1 min-h-0 px-3 pt-2 pb-1.5 flex items-end justify-center"
      >
        {subtitleEnabled && displayText && (
          <div
            className="max-w-full rounded-xl bg-[hsl(var(--bg-card)/0.9)] backdrop-blur-lg shadow-[0_2px_12px_hsl(var(--shadow-color)/0.08)] border border-[hsl(var(--border)/0.3)] overflow-hidden mb-1.5"
            style={{
              maxHeight: SUBTITLE_BUBBLE_MAX,
            }}
          >
            <div
              className="px-4 py-1.5"
            >
              <div
                ref={scrollRef}
                className="overflow-y-auto scrollbar-none text-center text-[hsl(var(--fg))]"
                style={{
                  maxHeight: SUBTITLE_VISIBLE_TEXT_MAX,
                  fontSize: `${SUBTITLE_FONT_SIZE}px`,
                  lineHeight: `${SUBTITLE_LINE_HEIGHT_PX}px`,
                  wordBreak: "break-word",
                }}
              >
                {committed && <span>{committed}</span>}
                {interim && (
                  <span className="opacity-60">
                    {committed ? " " : ""}{interim}
                  </span>
                )}
              </div>
            </div>
          </div>
        )}
      </div>

      {/* ── Indicator pill: draggable, hover feedback, right-click to reset ── */}
      <div
        data-tauri-drag-region
        onContextMenu={handleContextMenu}
        className="shrink-0 mt-1 mb-1 flex items-center gap-2 px-3.5 py-1.5 rounded-full bg-[hsl(var(--bg-card)/0.85)] backdrop-blur-md shadow-[0_2px_12px_hsl(var(--shadow-color)/0.08)] border border-[hsl(var(--border)/0.3)] cursor-grab active:cursor-grabbing transition-transform duration-150 hover:scale-[1.04] hover:shadow-[0_2px_16px_hsl(var(--shadow-color)/0.15)] hover:border-[hsl(var(--border)/0.5)]"
      >
        <span className="w-1.5 h-1.5 rounded-full shrink-0 animate-pulse pointer-events-none" style={{ backgroundColor: barColor }} />
        <div className="flex items-center gap-[3px] pointer-events-none" style={{ height: MAX_H }}>
          {heights.map((h, i) => (
            <span
              key={i}
              className="w-[3px] rounded-full"
              style={{
                height: isRecording ? h : MIN_H,
                transition: isRecording ? "height 60ms ease-out" : "height 300ms ease-in-out",
                backgroundColor: barColor,
                animation: isProcessing
                  ? isOptimizing
                    ? `wave-gentle 2.5s ease-in-out ${i * 0.18}s infinite`
                    : `wave-medium 1.8s ease-in-out ${i * 0.15}s infinite`
                  : isError
                    ? `wave-gentle 1.4s ease-in-out ${i * 0.1}s infinite`
                    : "none",
              }}
            />
          ))}
        </div>
        {isProcessing && (
          <span className="text-[11px] whitespace-nowrap pointer-events-none" style={{ color: "hsl(var(--primary))" }}>
            {isOptimizing ? "AI 优化中" : slow ? "正在努力识别中…" : "识别中"}
          </span>
        )}
        {isError && (
          <span
            className="text-[11px] whitespace-nowrap pointer-events-none"
            style={{ color: "hsl(var(--danger))" }}
          >
            识别失败
          </span>
        )}
      </div>
    </div>
  );
}
