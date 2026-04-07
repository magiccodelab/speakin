import { useState, useRef, useEffect } from "react";
import { ChevronUp, ChevronDown, Trash2 } from "lucide-react";
import { cn } from "../lib/utils";
import type { LogEntry } from "../App";

interface NetworkLogProps {
  logs: LogEntry[];
  onClear: () => void;
}

const LEVEL_COLORS: Record<string, string> = {
  info: "text-info",
  send: "text-primary",
  recv: "text-ok",
  warn: "text-warn",
  error: "text-danger",
};

export function NetworkLog({ logs, onClear }: NetworkLogProps) {
  const [expanded, setExpanded] = useState(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (expanded && scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [logs, expanded]);

  return (
    <div className={cn(
      "shrink-0 border-t border-edge bg-surface-card overflow-hidden",
      "transition-[max-height] duration-[var(--t-slow)] ease-out",
      expanded ? "max-h-[200px]" : "max-h-8"
    )}>
      {/* Header bar */}
      <div
        className="flex items-center justify-between px-3 h-8 cursor-pointer hover:bg-surface-subtle transition-colors"
        onClick={() => setExpanded(!expanded)}
      >
        <div className="flex items-center gap-2">
          {expanded ? <ChevronDown size={14} className="text-fg-3" /> : <ChevronUp size={14} className="text-fg-3" />}
          <span className="text-xs font-medium text-fg-3 uppercase tracking-wider">Network</span>
          <span className="text-xs text-fg-3">({logs.length})</span>
          {logs.length > 0 && !expanded && (
            <span className="text-xs text-fg-3 truncate max-w-[200px] font-mono">
              {logs[logs.length - 1].msg}
            </span>
          )}
        </div>
        {expanded && (
          <button
            onClick={(e) => { e.stopPropagation(); onClear(); }}
            className="p-1 rounded text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all"
            title="清除日志"
          >
            <Trash2 size={12} />
          </button>
        )}
      </div>

      {/* Log content */}
      {expanded && (
        <div
          ref={scrollRef}
          className="max-h-[168px] overflow-y-auto px-3 pb-2 font-mono text-xs leading-5 select-text"
        >
          {logs.length === 0 ? (
            <p className="text-fg-3 text-center mt-4">暂无日志</p>
          ) : (
            logs.map((log, i) => (
              <div key={i} className="flex gap-2 hover:bg-surface-subtle/50 px-1 rounded">
                <span className="text-fg-3 shrink-0 w-[72px]">{log.ts}</span>
                <span className={cn("shrink-0 w-[36px] uppercase font-semibold", LEVEL_COLORS[log.level] || "text-fg-3")}>
                  {log.level}
                </span>
                <span className="text-fg-2 break-all">{log.msg}</span>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  );
}
