/**
 * System-level recording indicator overlay.
 * Rendered in a separate Tauri window — transparent, frameless, always on top.
 * Wave bars respond to real-time audio RMS level from the backend.
 *
 * Theme is synced once on mount (reads localStorage). If the user switches
 * theme in the main window, the overlay picks up the new theme on the next
 * recording session — no real-time sync needed.
 */
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import "../index.css";

const BAR_COUNT = 5;
const MIN_H = 4;   // px — minimum bar height (idle)
const MAX_H = 28;  // px — maximum bar height (loud)

// Each bar gets a slightly different amplitude multiplier for organic feel
const BAR_WEIGHTS = [0.6, 0.85, 1.0, 0.8, 0.55];

/** Sync theme class from localStorage (written by ThemeToggle in main window). */
function syncTheme() {
  const stored = localStorage.getItem("theme");
  const prefersDark = window.matchMedia("(prefers-color-scheme: dark)").matches;
  const isDark = stored ? stored === "dark" : prefersDark;
  document.documentElement.classList.toggle("dark", isDark);
}

export function RecordingOverlay() {
  const [heights, setHeights] = useState<number[]>(() => Array(BAR_COUNT).fill(MIN_H));
  const levelRef = useRef(0);
  const rafRef = useRef(0);
  const smoothRef = useRef(0);

  // Sync theme once on mount
  useEffect(() => { syncTheme(); }, []);

  // Listen for audio-level events from backend
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    listen<number>("audio-level", (e) => {
      levelRef.current = e.payload;
    }).then((fn) => { unlisten = fn; });
    return () => unlisten?.();
  }, []);

  // Animation loop: smooth the level and update bar heights at ~30fps
  useEffect(() => {
    let running = true;
    const animate = () => {
      if (!running) return;

      const target = levelRef.current;
      // Exponential smoothing: fast attack (0.35), slow decay (0.08)
      const alpha = target > smoothRef.current ? 0.35 : 0.08;
      smoothRef.current += (target - smoothRef.current) * alpha;
      const level = smoothRef.current;

      const newHeights = BAR_WEIGHTS.map(
        (w) => MIN_H + (MAX_H - MIN_H) * Math.min(level * w, 1)
      );
      setHeights(newHeights);

      rafRef.current = requestAnimationFrame(animate);
    };
    rafRef.current = requestAnimationFrame(animate);
    return () => {
      running = false;
      cancelAnimationFrame(rafRef.current);
    };
  }, []);

  return (
    <div
      className="w-full h-full flex items-center justify-center select-none"
      data-tauri-drag-region
    >
      <div className="flex items-center gap-3 px-5 py-3 rounded-full bg-[rgba(255,255,255,0.8)] dark:bg-[rgba(0,0,0,0.6)] backdrop-blur-md shadow-[0_4px_24px_rgba(0,0,0,0.15)] dark:shadow-[0_4px_24px_rgba(0,0,0,0.4)] border border-[rgba(0,0,0,0.08)] dark:border-white/10">
        {/* Pulsing dot — uses theme recording color */}
        <span className="w-2 h-2 rounded-full bg-recording animate-pulse shrink-0" />

        {/* Reactive wave bars — theme-aware colors */}
        <div className="flex items-center gap-1" style={{ height: MAX_H }}>
          {heights.map((h, i) => (
            <span
              key={i}
              className="w-1 rounded-full bg-recording dark:bg-white/80"
              style={{ height: h, transition: "height 60ms ease-out" }}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
