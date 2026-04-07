export interface ThemeColorPreset {
  id: string;
  name: string;
  light: string; // HSL values e.g. "221 83% 53%"
  dark: string;
}

export const THEME_PRESETS: ThemeColorPreset[] = [
  { id: "blue",   name: "蓝色", light: "221 83% 53%", dark: "217 85% 62%" },
  { id: "purple", name: "紫色", light: "262 83% 58%", dark: "262 85% 65%" },
  { id: "green",  name: "绿色", light: "152 69% 40%", dark: "152 65% 50%" },
  { id: "orange", name: "橙色", light: "25 95% 53%",  dark: "25 90% 60%" },
  { id: "rose",   name: "玫红", light: "340 82% 52%", dark: "340 80% 62%" },
  { id: "cyan",   name: "青色", light: "186 72% 42%", dark: "186 70% 52%" },
];

const DEFAULT_ID = "blue";

/** Apply theme color to the document root and write localStorage cache for flash prevention. */
export function applyThemeColor(colorId: string, isDark: boolean, recordingFollows = false) {
  const preset = THEME_PRESETS.find((p) => p.id === colorId) || THEME_PRESETS[0];
  const resolvedId = preset.id; // Sanitize: invalid IDs fall back to blue
  const hsl = isDark ? preset.dark : preset.light;
  const root = document.documentElement;

  if (resolvedId === DEFAULT_ID) {
    root.style.removeProperty("--primary");
  } else {
    root.style.setProperty("--primary", hsl);
  }

  if (recordingFollows && resolvedId !== DEFAULT_ID) {
    root.style.setProperty("--recording", hsl);
    root.style.setProperty("--recording-pulse", hsl);
  } else {
    root.style.removeProperty("--recording");
    root.style.removeProperty("--recording-pulse");
  }

  // Write localStorage cache for index.html flash-prevention script
  // Always use resolvedId (not raw colorId) to prevent invalid IDs in cache
  localStorage.setItem("theme-color-light", preset.light);
  localStorage.setItem("theme-color-dark", preset.dark);
  localStorage.setItem("theme-color-id", resolvedId);
  localStorage.setItem("theme-recording-follows", recordingFollows ? "1" : "0");
}
