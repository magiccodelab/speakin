import { useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, Copy, X } from "lucide-react";
import { cn } from "../lib/utils";
import { Tooltip } from "./ui/Tooltip";

const appWindow = getCurrentWindow();

interface TitleBarProps {
  children?: React.ReactNode;
  onClose?: () => void;
}

export function TitleBar({ children, onClose }: TitleBarProps) {
  const [isMaximized, setIsMaximized] = useState(false);

  useEffect(() => {
    appWindow.isMaximized().then(setIsMaximized);
    const unlisten = appWindow.onResized(() => {
      appWindow.isMaximized().then(setIsMaximized);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, []);

  return (
    <header
      className="flex items-center h-8 bg-surface-card border-b border-edge shrink-0 select-none"
    >
      {/* Left: app title (drag region) */}
      <div
        className="flex items-center gap-2 px-3 pointer-events-none"
        data-tauri-drag-region
      >
        <svg viewBox="0 0 512 512" className="w-5 h-5 shrink-0" aria-hidden="true">
          <rect x="218" y="128" width="76" height="136" rx="38" className="fill-primary" />
          <path d="M176 264c0 44.2 35.8 80 80 80s80-35.8 80-80" fill="none" className="stroke-primary" strokeWidth="20" strokeLinecap="round" />
          <line x1="256" y1="344" x2="256" y2="378" className="stroke-primary" strokeWidth="20" strokeLinecap="round" />
          <line x1="230" y1="378" x2="282" y2="378" className="stroke-primary" strokeWidth="20" strokeLinecap="round" />
          <rect x="310" y="180" width="5.5" height="60" rx="2.75" className="fill-primary opacity-60" />
          <path d="M152 214c-12-18-12-46 0-64" fill="none" className="stroke-primary opacity-40" strokeWidth="10" strokeLinecap="round" />
          <path d="M124 228c-22-32-22-80 0-112" fill="none" className="stroke-primary opacity-25" strokeWidth="10" strokeLinecap="round" />
        </svg>
        <h1 className="text-xs font-semibold text-fg tracking-tight">SpeakIn声入</h1>
      </div>

      {/* Spacer: fills empty area, acts as drag region */}
      <div className="flex-1 h-full" data-tauri-drag-region />

      {/* Right: app controls + window controls (no drag region) */}
      <div className="flex items-center h-full">
        {/* App controls (theme, settings) */}
        <div className="flex items-center gap-0.5 px-1.5">
          {children}
        </div>
        <div className="w-px h-4 bg-edge mx-0.5" />
        {/* Window controls */}
        <Tooltip content="最小化" side="bottom">
          <button
            tabIndex={-1}
            onClick={() => appWindow.minimize()}
            className={cn(
              "h-8 w-11 flex items-center justify-center outline-none",
              "text-fg-3 hover:text-fg hover:bg-surface-subtle active:bg-surface-inset",
              "transition-colors duration-[var(--t-fast)]"
            )}
          >
            <Minus size={14} />
          </button>
        </Tooltip>
        <Tooltip content={isMaximized ? "还原" : "最大化"} side="bottom">
          <button
            tabIndex={-1}
            onClick={() => appWindow.toggleMaximize()}
            className={cn(
              "h-8 w-11 flex items-center justify-center outline-none",
              "text-fg-3 hover:text-fg hover:bg-surface-subtle active:bg-surface-inset",
              "transition-colors duration-[var(--t-fast)]"
            )}
          >
            {isMaximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
        </Tooltip>
        <Tooltip content="关闭" side="bottom">
          <button
            tabIndex={-1}
            onClick={() => onClose ? onClose() : appWindow.close()}
            className={cn(
              "h-8 w-11 flex items-center justify-center outline-none",
              "text-fg-3 hover:text-white hover:bg-[#e81123] active:bg-[#c50f1f]",
              "transition-colors duration-[var(--t-fast)]"
            )}
          >
            <X size={14} />
          </button>
        </Tooltip>
      </div>
    </header>
  );
}
