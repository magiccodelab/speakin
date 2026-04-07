import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Copy, ChevronDown, ChevronUp, Trash2 } from "lucide-react";
import { cn } from "../lib/utils";

interface TranscriptRecord {
  id: string;
  timestamp: number;
  original: string;
  final_text: string;
  optimized?: string;
  duration_ms: number;
  /** "done" | "partial" | "aborted" — legacy records may be missing. */
  status?: string;
}

// Minimal badge for non-done records. `done` renders nothing so the
// common case stays visually clean; only exceptions get a small marker.
function StatusBadge({ status }: { status?: string }) {
  if (!status || status === "done") return null;
  if (status === "partial") {
    return (
      <span className="inline-flex items-center gap-1 text-warn/90" title="ASR 未完整 / AI 优化失败">
        <span className="w-1.5 h-1.5 rounded-full bg-warn" />
        未完成
      </span>
    );
  }
  if (status === "aborted") {
    return (
      <span className="inline-flex items-center gap-1 text-fg-3/55" title="用户按 ESC 中止">
        <span className="w-1.5 h-1.5 rounded-full bg-fg-3/50" />
        已中止
      </span>
    );
  }
  return null;
}

function formatRelativeTime(timestamp: number): string {
  const diff = Date.now() - timestamp;
  const seconds = Math.floor(diff / 1000);
  if (seconds < 60) return "刚刚";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} 分钟前`;
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return `${hours} 小时前`;
  const days = Math.floor(hours / 24);
  return `${days} 天前`;
}

interface RecentTranscriptsProps {
  refreshKey?: number;
  onToast?: (msg: string) => void;
}

export function RecentTranscripts({ refreshKey, onToast }: RecentTranscriptsProps) {
  const [records, setRecords] = useState<TranscriptRecord[]>([]);
  const [expandedId, setExpandedId] = useState<string | null>(null);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  const loadRecords = useCallback(() => {
    invoke<TranscriptRecord[]>("get_transcript_records")
      .then(setRecords)
      .catch(() => {});
  }, []);

  useEffect(() => { loadRecords(); }, [loadRecords, refreshKey]);

  const handleCopy = async (text: string, id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    try {
      await navigator.clipboard.writeText(text);
      setCopiedId(id);
      setTimeout(() => setCopiedId(null), 1500);
      onToast?.("已复制");
    } catch {
      onToast?.("复制失败");
    }
  };

  const handleClear = async () => {
    await invoke("clear_transcript_records").catch(() => {});
    setRecords([]);
  };

  if (records.length === 0) return null;

  return (
    <div className="space-y-1">
      <div className="flex items-center justify-between mb-2">
        <span className="text-[11px] font-medium text-fg-3/60 uppercase tracking-widest">最近转录</span>
        <button
          onClick={handleClear}
          className={cn(
            "inline-flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium rounded-md",
            "text-fg-3/50 hover:text-danger hover:bg-danger-muted active:scale-95 transition-all duration-[var(--t-fast)]"
          )}
        >
          <Trash2 size={10} />
          清空
        </button>
      </div>
      {records.map((record) => {
        const isExpanded = expandedId === record.id;
        const charCount = record.final_text.length;
        const preview = record.final_text.length > 60
          ? record.final_text.slice(0, 60) + "..."
          : record.final_text;
        const hasMultipleVersions = record.optimized || record.final_text !== record.original;

        return (
          <div
            key={record.id}
            className={cn(
              "rounded-lg border border-edge/60 transition-all duration-[var(--t-fast)]",
              isExpanded ? "bg-surface-subtle/50" : "hover:bg-surface-subtle/30"
            )}
          >
            {/* Header row */}
            <div
              className="flex items-start justify-between gap-2 px-3 py-2 cursor-pointer"
              onClick={() => setExpandedId(isExpanded ? null : record.id)}
            >
              <div className="flex-1 min-w-0">
                <div className="flex items-center gap-2 text-[11px] text-fg-3/60 mb-0.5">
                  <span>{formatRelativeTime(record.timestamp)}</span>
                  <span>·</span>
                  <span>{charCount} 字符</span>
                  {record.status && record.status !== "done" && (
                    <>
                      <span>·</span>
                      <StatusBadge status={record.status} />
                    </>
                  )}
                  {hasMultipleVersions && (
                    <span className="flex items-center gap-0.5">
                      {isExpanded ? <ChevronUp size={10} /> : <ChevronDown size={10} />}
                    </span>
                  )}
                </div>
                {!isExpanded && (
                  <p className="text-xs text-fg-2 truncate">{preview}</p>
                )}
              </div>
              <button
                onClick={(e) => handleCopy(record.final_text, record.id, e)}
                className={cn(
                  "shrink-0 p-1.5 rounded-md transition-all duration-[var(--t-fast)]",
                  copiedId === record.id
                    ? "text-ok bg-ok-muted"
                    : "text-fg-3/50 hover:text-fg-2 hover:bg-surface-subtle active:scale-90"
                )}
              >
                <Copy size={12} />
              </button>
            </div>

            {/* Expanded content */}
            {isExpanded && (
              <div className="px-3 pb-2.5 space-y-2 text-xs">
                {hasMultipleVersions && (
                  <>
                    <div>
                      <span className="text-[10px] font-medium text-fg-3/50 uppercase tracking-wider">原文</span>
                      <p className="text-fg-2 mt-0.5 whitespace-pre-wrap select-text">{record.original}</p>
                    </div>
                    {record.optimized && (
                      <div>
                        <span className="text-[10px] font-medium text-primary/60 uppercase tracking-wider">AI 优化</span>
                        <p className="text-fg-2 mt-0.5 whitespace-pre-wrap select-text">{record.optimized}</p>
                      </div>
                    )}
                    {record.final_text !== (record.optimized || record.original) && (
                      <div>
                        <span className="text-[10px] font-medium text-fg-3/50 uppercase tracking-wider">最终输出</span>
                        <p className="text-fg mt-0.5 whitespace-pre-wrap select-text">{record.final_text}</p>
                      </div>
                    )}
                  </>
                )}
                {!hasMultipleVersions && (
                  <p className="text-fg-2 whitespace-pre-wrap select-text">{record.final_text}</p>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}
