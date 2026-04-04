import { useState, useEffect } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Minus, Square, Copy, X } from "lucide-react";
import { cn } from "../lib/utils";
import { Tooltip } from "./ui/Tooltip";

const appWindow = getCurrentWindow();

interface TitleBarProps {
  children?: React.ReactNode;
}

export function TitleBar({ children }: TitleBarProps) {
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
        <div className="w-2 h-2 rounded-full bg-primary" />
        <h1 className="text-xs font-semibold text-fg tracking-tight">Voice Input</h1>
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
            onClick={() => appWindow.minimize()}
            className={cn(
              "h-8 w-11 flex items-center justify-center",
              "text-fg-3 hover:text-fg hover:bg-surface-subtle",
              "transition-colors duration-[var(--t-fast)]"
            )}
          >
            <Minus size={14} />
          </button>
        </Tooltip>
        <Tooltip content={isMaximized ? "还原" : "最大化"} side="bottom">
          <button
            onClick={() => appWindow.toggleMaximize()}
            className={cn(
              "h-8 w-11 flex items-center justify-center",
              "text-fg-3 hover:text-fg hover:bg-surface-subtle",
              "transition-colors duration-[var(--t-fast)]"
            )}
          >
            {isMaximized ? <Copy size={12} /> : <Square size={12} />}
          </button>
        </Tooltip>
        <Tooltip content="关闭" side="bottom">
          <button
            onClick={() => appWindow.close()}
            className={cn(
              "h-8 w-11 flex items-center justify-center",
              "text-fg-3 hover:text-white hover:bg-[#e81123]",
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
