import { useState, useEffect, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Clock, Hash, Type, Zap, Gauge, Mic, RotateCcw } from "lucide-react";
import { cn } from "../../lib/utils";

interface UsageStats {
  total_sessions: number;
  total_recording_duration_ms: number;
  total_characters: number;
  total_chinese_chars: number;
}

const EMPTY_STATS: UsageStats = {
  total_sessions: 0,
  total_recording_duration_ms: 0,
  total_characters: 0,
  total_chinese_chars: 0,
};

function formatDuration(ms: number): string {
  if (ms === 0) return "0 秒";
  const totalSecs = Math.floor(ms / 1000);
  const h = Math.floor(totalSecs / 3600);
  const m = Math.floor((totalSecs % 3600) / 60);
  const s = totalSecs % 60;
  const parts: string[] = [];
  if (h > 0) parts.push(`${h} 小时`);
  if (m > 0) parts.push(`${m} 分`);
  if (s > 0 || parts.length === 0) parts.push(`${s} 秒`);
  return parts.join(" ");
}

function formatNumber(n: number): string {
  return n.toLocaleString("zh-CN");
}

/** Typing speed baseline: 50 Chinese chars per minute */
const TYPING_CPM = 50;

function computeTimeSaved(stats: UsageStats): string {
  if (stats.total_chinese_chars === 0 || stats.total_recording_duration_ms === 0) return "0 分";
  const typingMinutes = stats.total_chinese_chars / TYPING_CPM;
  const voiceMinutes = stats.total_recording_duration_ms / 60000;
  const savedMinutes = Math.max(0, typingMinutes - voiceMinutes);
  if (savedMinutes >= 60) {
    const h = Math.floor(savedMinutes / 60);
    const m = Math.round(savedMinutes % 60);
    return m > 0 ? `${h} 小时 ${m} 分` : `${h} 小时`;
  }
  return `${Math.round(savedMinutes)} 分`;
}

function computeCPM(stats: UsageStats): string {
  if (stats.total_recording_duration_ms === 0) return "—";
  const minutes = stats.total_recording_duration_ms / 60000;
  return Math.round(stats.total_characters / minutes).toString();
}

interface StatCardProps {
  icon: React.ReactNode;
  label: string;
  value: string;
  unit?: string;
}

function StatCard({ icon, label, value, unit }: StatCardProps) {
  return (
    <div className="bg-surface-subtle rounded-xl p-4 flex flex-col gap-2">
      <div className="flex items-center gap-2 text-fg-3">
        {icon}
        <span className="text-xs font-medium uppercase tracking-widest">{label}</span>
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-2xl font-bold text-fg">{value}</span>
        {unit && <span className="text-sm text-fg-3">{unit}</span>}
      </div>
    </div>
  );
}

export function StatsTab() {
  const [stats, setStats] = useState<UsageStats>(EMPTY_STATS);
  const [resetting, setResetting] = useState(false);

  const loadStats = useCallback(() => {
    invoke<UsageStats>("get_usage_stats")
      .then(setStats)
      .catch(() => {});
  }, []);

  useEffect(() => { loadStats(); }, [loadStats]);

  const handleReset = async () => {
    setResetting(true);
    try {
      await invoke("reset_usage_stats");
      setStats(EMPTY_STATS);
    } catch {}
    setResetting(false);
  };

  return (
    <div className="space-y-5">
      <div className="grid grid-cols-2 gap-3">
        <StatCard
          icon={<Clock size={14} />}
          label="累计录音时长"
          value={formatDuration(stats.total_recording_duration_ms)}
        />
        <StatCard
          icon={<Mic size={14} />}
          label="累计录音次数"
          value={formatNumber(stats.total_sessions)}
          unit="次"
        />
        <StatCard
          icon={<Type size={14} />}
          label="累计字数"
          value={formatNumber(stats.total_chinese_chars)}
          unit="字"
        />
        <StatCard
          icon={<Hash size={14} />}
          label="累计字符数"
          value={formatNumber(stats.total_characters)}
          unit="字符"
        />
        <StatCard
          icon={<Zap size={14} />}
          label="节省时间"
          value={`约 ${computeTimeSaved(stats)}`}
        />
        <StatCard
          icon={<Gauge size={14} />}
          label="输入效率"
          value={computeCPM(stats)}
          unit="CPM"
        />
      </div>

      <div className="pt-2 border-t border-edge">
        <p className="text-xs text-fg-3 mb-3">
          节省时间基于手打 {TYPING_CPM} 字/分钟估算，CPM 为每分钟字符数
        </p>
        <button
          onClick={handleReset}
          disabled={resetting || stats.total_sessions === 0}
          className={cn(
            "flex items-center gap-2 px-3 py-2 text-xs font-medium rounded-lg",
            "text-fg-3 hover:text-danger hover:bg-danger-muted",
            "active:scale-95 transition-all duration-[var(--t-fast)]",
            "disabled:opacity-40 disabled:cursor-not-allowed",
          )}
        >
          <RotateCcw size={13} />
          重置统计数据
        </button>
      </div>
    </div>
  );
}
