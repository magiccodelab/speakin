/**
 * Manages the system-level recording overlay window.
 * Creates a small transparent, always-on-top Tauri window at the bottom
 * center of the screen showing a voice wave animation.
 */
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";
import { currentMonitor } from "@tauri-apps/api/window";

const OVERLAY_LABEL = "recording-overlay";
const OVERLAY_WIDTH = 200;
const OVERLAY_HEIGHT = 48;
const BOTTOM_MARGIN = 48;

let overlayWindow: WebviewWindow | null = null;

export async function showOverlay() {
  // Don't create if already exists
  if (overlayWindow) return;

  try {
    const monitor = await currentMonitor();
    const screenWidth = monitor?.size.width ?? 1920;
    const screenHeight = monitor?.size.height ?? 1080;
    const scaleFactor = monitor?.scaleFactor ?? 1;

    // Calculate position: bottom center of screen (in logical pixels)
    const logicalWidth = screenWidth / scaleFactor;
    const logicalHeight = screenHeight / scaleFactor;
    const x = Math.round((logicalWidth - OVERLAY_WIDTH) / 2);
    const y = Math.round(logicalHeight - OVERLAY_HEIGHT - BOTTOM_MARGIN);

    overlayWindow = new WebviewWindow(OVERLAY_LABEL, {
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

    overlayWindow.once("tauri://error", () => {
      overlayWindow = null;
    });
  } catch (e) {
    console.warn("Failed to create overlay:", e);
    overlayWindow = null;
  }
}

export async function hideOverlay() {
  if (!overlayWindow) return;
  try {
    await overlayWindow.destroy();
  } catch {
    // Window may already be closed
  }
  overlayWindow = null;
}
