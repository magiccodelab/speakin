import { useState, useEffect } from "react";
import { Sun, Moon } from "lucide-react";
import { cn } from "../lib/utils";
import { Tooltip } from "./ui/Tooltip";
import { applyThemeColor } from "../lib/theme-colors";

export function ThemeToggle() {
  const [dark, setDark] = useState(() => {
    if (typeof window !== "undefined") {
      const stored = localStorage.getItem("theme");
      if (stored) return stored === "dark";
      return window.matchMedia("(prefers-color-scheme: dark)").matches;
    }
    return false;
  });

  useEffect(() => {
    const root = document.documentElement;
    if (dark) {
      root.classList.add("dark");
    } else {
      root.classList.remove("dark");
    }
    localStorage.setItem("theme", dark ? "dark" : "light");
    // Sync theme color for the new light/dark mode
    const colorId = localStorage.getItem("theme-color-id") || "blue";
    const recordingFollows = localStorage.getItem("theme-recording-follows") === "1";
    applyThemeColor(colorId, dark, recordingFollows);
  }, [dark]);

  return (
    <Tooltip content={dark ? "切换到亮色模式" : "切换到暗色模式"} side="bottom">
      <button
        onClick={() => setDark(!dark)}
        className={cn(
          "group p-1.5 rounded-md transition-all duration-[var(--t-base)]",
          "text-fg-2 hover:text-fg hover:bg-surface-subtle",
          "active:scale-95"
        )}
      >
        {dark ? (
          <Sun size={16} className="transition-transform duration-[var(--t-slow)] group-hover:rotate-45" />
        ) : (
          <Moon size={16} className="transition-transform duration-[var(--t-slow)] group-hover:-rotate-12" />
        )}
      </button>
    </Tooltip>
  );
}
