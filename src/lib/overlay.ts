/**
 * Manages the system-level recording overlay window.
 * Creates a small transparent, always-on-top Tauri window at the bottom
 * center of the screen showing a voice wave animation.
 *
 * Position is persisted in localStorage("overlay-position") as logical
 * coordinates. The overlay component tracks moves and saves; right-click
 * resets to the default bottom-center position.
 */
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { currentMonitor } from "@tauri-apps/api/window";

const OVERLAY_LABEL = "recording-overlay";
export const OVERLAY_WIDTH = 480;
// Leave extra vertical room so the subtitle bubble can fully show 3 lines
// above the draggable pill without clipping at the window edge.
export const OVERLAY_HEIGHT = 140;
export const BOTTOM_MARGIN = 96; // well above Windows taskbar
export const OVERLAY_POSITION_KEY = "overlay-position";

let overlayWindow: WebviewWindow | null = null;
let generation = 0; // Guards against show/hide async race

/** Calculate the default bottom-center position in logical pixels. */
async function getDefaultPosition(): Promise<{ x: number; y: number }> {
  const monitor = await currentMonitor();
  const screenWidth = monitor?.size.width ?? 1920;
  const screenHeight = monitor?.size.height ?? 1080;
  const scaleFactor = monitor?.scaleFactor ?? 1;
  const logicalWidth = screenWidth / scaleFactor;
  const logicalHeight = screenHeight / scaleFactor;
  return {
    x: Math.round((logicalWidth - OVERLAY_WIDTH) / 2),
    y: Math.round(logicalHeight - OVERLAY_HEIGHT - BOTTOM_MARGIN),
  };
}

export async function showOverlay() {
  // Don't create if already exists
  if (overlayWindow) return;
  const thisGen = ++generation;

  try {
    // Destroy any residual window with the same label (e.g. from a previous
    // hideOverlay() whose destroy() hadn't finished yet)
    const existing = await WebviewWindow.getByLabel(OVERLAY_LABEL);
    if (existing) {
      try { await existing.destroy(); } catch {}
    }

    // Abort if a hide was requested while we were awaiting
    if (thisGen !== generation) return;

    // Read saved position or fall back to default
    const def = await getDefaultPosition();
    let x = def.x, y = def.y;
    const saved = localStorage.getItem(OVERLAY_POSITION_KEY);
    if (saved) {
      try {
        const pos = JSON.parse(saved);
        if (Number.isFinite(pos.x) && Number.isFinite(pos.y)) {
          x = pos.x;
          y = pos.y;
        }
      } catch {
        // Malformed JSON — use default
      }
    }

    // Abort if a hide was requested while we were awaiting
    if (thisGen !== generation) return;

    const win = new WebviewWindow(OVERLAY_LABEL, {
      url: "/#overlay",
      width: OVERLAY_WIDTH,
      height: OVERLAY_HEIGHT,
      x,
      y,
      transparent: true,
      decorations: false,
      alwaysOnTop: true,
      skipTaskbar: true,
      resizable: false,
      focus: false,
      shadow: false,
    });
    overlayWindow = win;

    // Only clear if this window is still the active one (prevents a stale
    // error callback from an old window nulling a newer window's reference)
    win.once("tauri://error", () => {
      if (overlayWindow === win) overlayWindow = null;
    });
  } catch (e) {
    console.warn("Failed to create overlay:", e);
    overlayWindow = null;
  }
}

export async function hideOverlay() {
  generation++; // Invalidate any pending showOverlay
  const win = overlayWindow;
  overlayWindow = null; // Null FIRST so showOverlay() won't see a stale ref
  if (!win) return;
  try {
    await win.destroy();
  } catch {
    // Window may already be closed
  }
}
