import ReactDOM from "react-dom/client";
import "./index.css";

// Disable the WebView2 native context menu (Reload / View source / Inspect …)
// in production. Kept enabled in dev so we can still inspect element.
// Tauri 2 has no built-in config switch for this — JS prevention is the
// recommended path. Single global listener is enough for both windows
// (main + overlay) since they share this entry file.
if (!import.meta.env.DEV) {
  window.addEventListener("contextmenu", (e) => e.preventDefault());
}

// Disable Ctrl+A / Cmd+A (Select All) globally — drag-selection still works.
// Inputs / textareas / contenteditable keep native Ctrl+A behavior so users
// can still select API key fields, prompt textareas, etc.
window.addEventListener("keydown", (e) => {
  if ((e.ctrlKey || e.metaKey) && (e.key === "a" || e.key === "A")) {
    const t = e.target as HTMLElement | null;
    const tag = t?.tagName;
    const editable =
      tag === "INPUT" ||
      tag === "TEXTAREA" ||
      (t?.isContentEditable ?? false);
    if (!editable) {
      e.preventDefault();
    }
  }
});

const root = document.getElementById("root")!;

// The overlay window loads with #overlay hash.
// Transparent background is enforced by `html.overlay-window` CSS rules
// (set synchronously in index.html head) — no runtime style mutation needed,
// which prevents the first-launch black flash.
if (window.location.hash === "#overlay") {
  import("./components/RecordingOverlay").then(({ RecordingOverlay }) => {
    ReactDOM.createRoot(root).render(<RecordingOverlay />);
  });
} else {
  import("./App").then(({ default: App }) => {
    ReactDOM.createRoot(root).render(<App />);
    // Main window starts hidden (tauri.conf.json visible:false) to avoid the
    // native white flash before WebView2 paints. Show it once the first frame
    // with correct theme has been rendered.
    requestAnimationFrame(() => {
      requestAnimationFrame(async () => {
        try {
          const { getCurrentWindow } = await import("@tauri-apps/api/window");
          await getCurrentWindow().show();
        } catch (e) {
          console.warn("Failed to show main window:", e);
        }
      });
    });
  });
}
