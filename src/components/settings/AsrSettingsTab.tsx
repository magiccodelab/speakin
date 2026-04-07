import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { enable as enableAutostart, disable as disableAutostart, isEnabled as isAutoStartEnabled } from "@tauri-apps/plugin-autostart";
import { Eye, EyeOff, RefreshCw, Keyboard, Plus, Pencil, Trash2, ArrowRight, Check, X as XIcon } from "lucide-react";
import { cn } from "../../lib/utils";
import { THEME_PRESETS, applyThemeColor } from "../../lib/theme-colors";
import { buildHotkeyString, normalizeHotkeyString, validateHotkeyString } from "../../lib/hotkey";
import { Tooltip } from "../ui/Tooltip";
import { RadioIndicator } from "../ui/RadioGroup";
import { Select } from "../ui/Select";
import { ToggleCard } from "../ui/Toggle";
import type { AppSettings, DoubaoProviderSettings, DashScopeProviderSettings, QwenProviderSettings } from "../../App";
import type { TextReplacement, TextReplacementsFile } from "../../lib/replacements";

interface AsrSettingsTabProps {
  form: AppSettings;
  handleChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  hotkeyError: string | null;
  onHotkeyChange: (value: string) => boolean;
}

const PROVIDER_OPTIONS = [
  { label: "豆包大模型语音识别（火山引擎）", value: "doubao" },
  { label: "Paraformer 语音识别（阿里云百炼）", value: "dashscope" },
  { label: "Qwen3 语音识别（阿里云百炼）", value: "qwen" },
];

const DASHSCOPE_MODEL_OPTIONS = [
  { label: "Paraformer 实时 v2", value: "paraformer-realtime-v2" },
  { label: "Fun-ASR 实时", value: "fun-asr-realtime" },
];

const REGION_OPTIONS = [
  { label: "北京（中国大陆）", value: "beijing" },
  { label: "新加坡（海外）", value: "singapore" },
];

const QWEN_MODEL_OPTIONS = [
  { label: "Qwen3 ASR Flash 实时", value: "qwen3-asr-flash-realtime" },
];

const QWEN_LANGUAGE_OPTIONS = [
  { label: "中文", value: "zh" },
  { label: "英文", value: "en" },
  { label: "日语", value: "ja" },
  { label: "韩语", value: "ko" },
  { label: "粤语", value: "yue" },
];

const RESOURCE_OPTIONS = [
  { label: "豆包大模型语音识别 2.0（按时长计费）", value: "volc.seedasr.sauc.duration" },
];

const ASR_MODE_OPTIONS = [
  { label: "双向流式·二遍优化（实时出字，中英文+方言）", value: "bistream" },
  { label: "流式输入（分句返回，支持方言/25种外语）", value: "nostream" },
];

const PRESET_HOTKEYS = [
  "Ctrl+Shift+V",
  "Ctrl+Alt+R",
  "Ctrl+Shift+Space",
  "F2",
];

const inputClass = cn(
  "w-full px-3 py-2 text-sm rounded-lg",
  "bg-surface border border-edge text-fg",
  "placeholder:text-fg-3/60",
  "focus:border-[hsl(var(--primary)/0.5)] focus:shadow-[0_0_0_3px_hsl(var(--primary)/0.14)] focus:outline-none",
  "transition-all duration-[var(--t-fast)]"
);

function HotkeyRecorder({
  value,
  error,
  onChange,
}: {
  value: string;
  error: string | null;
  onChange: (v: string) => boolean;
}) {
  const [recording, setRecording] = useState(false);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();
    const combo = buildHotkeyString(e);
    if (!combo) return;
    if (onChange(combo)) {
      setRecording(false);
    }
  }, [onChange]);

  useEffect(() => {
    if (!recording) return;
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [recording, handleKeyDown]);

  return (
    <div className="space-y-2">
      <div
        onClick={() => setRecording((prev) => !prev)}
        className={cn(
          "w-full px-3 py-2 text-sm rounded-lg cursor-pointer select-none",
          "border text-center font-mono transition-all duration-[var(--t-fast)]",
          recording
            ? "bg-primary/10 border-primary text-primary shadow-[0_0_0_3px_hsl(var(--primary)/0.14)]"
            : error
              ? "bg-danger-muted/60 border-danger text-danger"
            : "bg-surface border-edge text-fg hover:border-[hsl(var(--primary)/0.5)]"
        )}
      >
        {recording ? (
          <span className="flex items-center justify-center gap-2">
            <Keyboard size={14} className="animate-pulse" />
            请按下快捷键...
          </span>
        ) : (
          <span>{value || "未设置"}</span>
        )}
      </div>
      <div className="flex gap-1.5 flex-wrap">
        {PRESET_HOTKEYS.map((key) => (
          <button
            key={key}
            type="button"
            onClick={() => { if (onChange(key)) setRecording(false); }}
            className={cn(
              "px-2 py-1 text-xs rounded-md border transition-all duration-[var(--t-fast)]",
              value === key
                ? "bg-primary/10 border-primary text-primary"
                : "bg-surface-subtle border-edge text-fg-3 hover:text-fg-2 hover:border-edge-strong active:scale-95"
            )}
          >
            {key}
          </button>
        ))}
      </div>
      <div className={cn("text-xs", error ? "text-danger" : "text-fg-3")}>
        {error ?? (recording ? "按下组合键进行录制，再次点击可取消" : "普通输入键请至少搭配 Ctrl、Alt 或 Shift")}
      </div>
    </div>
  );
}

function RadioCard({
  name, value, checked, onChange, children,
}: {
  name: string; value: string; checked: boolean;
  onChange: () => void; children: React.ReactNode;
}) {
  return (
    <label className="group flex items-center gap-3 p-3 rounded-lg border border-edge hover:bg-surface-subtle hover:border-edge-strong active:scale-[0.98] transition-all duration-[var(--t-fast)] cursor-pointer focus-within:ring-2 focus-within:ring-primary focus-within:ring-offset-2 focus-within:ring-offset-surface">
      <input type="radio" name={name} value={value} checked={checked} onChange={onChange}
        className="fixed opacity-0 pointer-events-none" />
      <RadioIndicator checked={checked} />
      <div className="flex-1 min-w-0">{children}</div>
    </label>
  );
}

const VISIBLE_COUNT = 5;

function ReplacementsEditor() {
  const [replacements, setReplacements] = useState<TextReplacement[]>([]);
  const [showAll, setShowAll] = useState(false);
  const [editing, setEditing] = useState<{ index: number; from: string; to: string } | null>(null);
  const [adding, setAdding] = useState(false);
  const [addFrom, setAddFrom] = useState("");
  const [addTo, setAddTo] = useState("");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<TextReplacementsFile>("get_replacements")
      .then((data) => setReplacements(data.replacements))
      .catch(() => {});
  }, []);

  const save = async (updated: TextReplacement[]) => {
    try {
      await invoke("save_replacements", { data: { replacements: updated } });
      setReplacements(updated);
      setError(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleAdd = async () => {
    const from = addFrom.trim();
    const to = addTo.trim();
    if (!from) { setError("原文不能为空"); return; }
    if (from === to) { setError("原文和替换内容不能相同"); return; }
    await save([...replacements, { from, to }]);
    setAddFrom("");
    setAddTo("");
    setAdding(false);
  };

  const handleEditSave = async () => {
    if (!editing) return;
    const from = editing.from.trim();
    const to = editing.to.trim();
    if (!from) { setError("原文不能为空"); return; }
    if (from === to) { setError("原文和替换内容不能相同"); return; }
    const updated = [...replacements];
    updated[editing.index] = { from, to };
    await save(updated);
    setEditing(null);
  };

  const handleDelete = async (index: number) => {
    if (editing?.index === index) setEditing(null);
    await save(replacements.filter((_, i) => i !== index));
  };

  const visible = showAll ? replacements : replacements.slice(0, VISIBLE_COUNT);
  const hiddenCount = replacements.length - VISIBLE_COUNT;

  return (
    <div className="mt-3 space-y-2">
      {visible.map((r, i) => (
        <div key={`${r.from}-${i}`}>
          {editing?.index === i ? (
            <div className="flex items-center gap-1.5 p-2 rounded-lg border border-primary/30 bg-primary/5">
              <input value={editing.from} onChange={(e) => setEditing({ ...editing, from: e.target.value })}
                placeholder="原文" className="flex-1 min-w-0 px-2 py-1 text-sm rounded bg-surface border border-edge text-fg" />
              <ArrowRight size={12} className="text-fg-3 shrink-0" />
              <input value={editing.to} onChange={(e) => setEditing({ ...editing, to: e.target.value })}
                placeholder="替换为" className="flex-1 min-w-0 px-2 py-1 text-sm rounded bg-surface border border-edge text-fg" />
              <button onClick={handleEditSave} className="p-1 text-ok hover:bg-ok-muted rounded active:scale-95 transition-all">
                <Check size={14} />
              </button>
              <button onClick={() => setEditing(null)} className="p-1 text-fg-3 hover:bg-surface-subtle rounded active:scale-95 transition-all">
                <XIcon size={14} />
              </button>
            </div>
          ) : (
            <div className="flex items-center gap-1.5 p-2 rounded-lg border border-edge hover:bg-surface-subtle/50 transition-colors group">
              <span className="flex-1 min-w-0 text-sm text-fg truncate">{r.from}</span>
              <ArrowRight size={12} className="text-fg-3 shrink-0" />
              <span className="flex-1 min-w-0 text-sm text-fg-2 truncate">{r.to || <span className="text-fg-3 italic">删除</span>}</span>
              <div className="flex gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
                <button onClick={() => setEditing({ index: i, from: r.from, to: r.to })}
                  className="p-1 text-fg-3 hover:text-fg hover:bg-surface-inset rounded active:scale-95 transition-all">
                  <Pencil size={12} />
                </button>
                <button onClick={() => handleDelete(i)}
                  className="p-1 text-fg-3 hover:text-danger hover:bg-danger-muted rounded active:scale-95 transition-all">
                  <Trash2 size={12} />
                </button>
              </div>
            </div>
          )}
        </div>
      ))}

      {!showAll && hiddenCount > 0 && (
        <button onClick={() => setShowAll(true)}
          className="w-full py-1.5 text-xs text-primary hover:bg-primary/5 rounded-lg transition-colors">
          展开更多 ({hiddenCount})
        </button>
      )}
      {showAll && replacements.length > VISIBLE_COUNT && (
        <button onClick={() => setShowAll(false)}
          className="w-full py-1.5 text-xs text-fg-3 hover:bg-surface-subtle rounded-lg transition-colors">
          收起
        </button>
      )}

      {adding ? (
        <div className="flex items-center gap-1.5 p-2 rounded-lg border border-primary/30 bg-primary/5">
          <input value={addFrom} onChange={(e) => setAddFrom(e.target.value)}
            placeholder="原文" autoFocus
            className="flex-1 min-w-0 px-2 py-1 text-sm rounded bg-surface border border-edge text-fg" />
          <ArrowRight size={12} className="text-fg-3 shrink-0" />
          <input value={addTo} onChange={(e) => setAddTo(e.target.value)}
            placeholder="替换为"
            className="flex-1 min-w-0 px-2 py-1 text-sm rounded bg-surface border border-edge text-fg"
            onKeyDown={(e) => { if (e.key === "Enter") handleAdd(); }} />
          <button onClick={handleAdd} className="p-1 text-ok hover:bg-ok-muted rounded active:scale-95 transition-all">
            <Check size={14} />
          </button>
          <button onClick={() => { setAdding(false); setAddFrom(""); setAddTo(""); setError(null); }}
            className="p-1 text-fg-3 hover:bg-surface-subtle rounded active:scale-95 transition-all">
            <XIcon size={14} />
          </button>
        </div>
      ) : (
        <button onClick={() => setAdding(true)}
          className="inline-flex items-center gap-1 px-2.5 py-1.5 text-xs font-medium text-primary hover:bg-primary/5 rounded-lg transition-colors active:scale-95">
          <Plus size={13} />
          添加替换词
        </button>
      )}

      {error && (
        <p className="text-xs text-danger px-1">{error}</p>
      )}
    </div>
  );
}

export function AsrSettingsTab({ form, handleChange, hotkeyError, onHotkeyChange }: AsrSettingsTabProps) {
  const [showToken, setShowToken] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);
  const [loadingDevices, setLoadingDevices] = useState(false);
  const [autoStart, setAutoStart] = useState(false);
  const [showAutoStopWarning, setShowAutoStopWarning] = useState(false);

  useEffect(() => {
    isAutoStartEnabled().then(setAutoStart).catch(() => {});
  }, []);

  const handleAutoStartChange = async (v: boolean) => {
    try {
      if (v) { await enableAutostart(); } else { await disableAutostart(); }
      setAutoStart(v);
    } catch (e) {
      console.error("autostart toggle failed:", e);
    }
  };

  const loadDevices = async () => {
    setLoadingDevices(true);
    try {
      const list = await invoke<string[]>("list_audio_devices");
      setDevices(list);
    } catch {
      setDevices([]);
    }
    setLoadingDevices(false);
  };

  useEffect(() => { loadDevices(); }, []);

  const handleDoubaoChange = <K extends keyof DoubaoProviderSettings>(key: K, value: DoubaoProviderSettings[K]) => {
    handleChange("doubao" as keyof AppSettings, { ...form.doubao, [key]: value } as any);
  };

  const handleDashScopeChange = <K extends keyof DashScopeProviderSettings>(key: K, value: DashScopeProviderSettings[K]) => {
    handleChange("dashscope" as keyof AppSettings, { ...form.dashscope, [key]: value } as any);
  };

  const handleQwenChange = <K extends keyof QwenProviderSettings>(key: K, value: QwenProviderSettings[K]) => {
    handleChange("qwen" as keyof AppSettings, { ...form.qwen, [key]: value } as any);
  };

  return (
    <div className="space-y-5">
      {/* Appearance */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">外观</h3>
        <div className="space-y-3">
          <div>
            <label className="text-sm font-medium text-fg-2 mb-2 block">主题色</label>
            <div className="flex gap-2">
              {THEME_PRESETS.map((preset) => {
                const isActive = form.theme_color === preset.id;
                const isDark = document.documentElement.classList.contains("dark");
                return (
                  <button key={preset.id} type="button"
                    onClick={() => {
                      handleChange("theme_color", preset.id);
                      applyThemeColor(preset.id, isDark, form.recording_follows_theme);
                    }}
                    className={cn(
                      "w-7 h-7 rounded-full transition-all duration-[var(--t-fast)]",
                      "hover:scale-110 active:scale-95",
                      isActive
                        ? "ring-2 ring-offset-2 ring-offset-surface"
                        : "ring-1 ring-edge hover:ring-edge-strong"
                    )}
                    style={{
                      backgroundColor: `hsl(${isDark ? preset.dark : preset.light})`,
                      ...(isActive ? { boxShadow: `0 0 0 2px hsl(var(--bg)), 0 0 0 4px hsl(${isDark ? preset.dark : preset.light})` } : {}),
                    }}
                    title={preset.name}
                  />
                );
              })}
            </div>
          </div>
          <ToggleCard
            checked={form.recording_follows_theme}
            onChange={(v) => {
              handleChange("recording_follows_theme", v);
              const isDark = document.documentElement.classList.contains("dark");
              applyThemeColor(form.theme_color, isDark, v);
            }}
            label="录音颜色跟随主题色"
            description="关闭后录音状态保持红色"
          />
          <ToggleCard
            checked={form.show_overlay}
            onChange={(v) => handleChange("show_overlay", v)}
            label="桌面悬浮窗"
            description="录音时在桌面底部显示波形和转写文字"
          />
        </div>
      </section>

      {/* AI Provider */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">AI 供应商</h3>
        <div className="space-y-1.5 mb-4">
          <label className="text-sm font-medium text-fg-2">供应商</label>
          <Select value={form.provider} options={PROVIDER_OPTIONS}
            onChange={(v) => handleChange("provider", v)} />
        </div>

        {form.provider === "doubao" && (
          <div className="border-t border-edge pt-4 space-y-4">
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">App ID</label>
              <input type="text" value={form.doubao.app_id} onChange={(e) => handleDoubaoChange("app_id", e.target.value)}
                placeholder="输入 App ID" className={inputClass} />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">Access Token</label>
              <div className="relative">
                <input type={showToken ? "text" : "password"} value={form.doubao.access_token}
                  onChange={(e) => handleDoubaoChange("access_token", e.target.value)}
                  placeholder="输入 Access Token" className={cn(inputClass, "pr-10")} />
                <button type="button" onClick={() => setShowToken(!showToken)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all">
                  {showToken ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">资源 ID</label>
              <Select value={form.doubao.resource_id} options={RESOURCE_OPTIONS}
                onChange={(v) => handleDoubaoChange("resource_id", v)} />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">识别模式</label>
              <Select value={form.doubao.asr_mode ?? "bistream"} options={ASR_MODE_OPTIONS}
                onChange={(v) => handleDoubaoChange("asr_mode", v)} />
              <p className="text-xs text-fg-3 mt-1">
                {(form.doubao.asr_mode ?? "bistream") === "bistream"
                  ? "边说边出字，二遍识别优化准确率，支持中英文及方言"
                  : "说完分句返回，准确率更高，支持粤语、四川话等方言及25种外语"}
              </p>
            </div>
          </div>
        )}

        {form.provider === "dashscope" && (
          <div className="border-t border-edge pt-4 space-y-4">
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">API Key</label>
              <div className="relative">
                <input type={showToken ? "text" : "password"} value={form.dashscope.api_key}
                  onChange={(e) => handleDashScopeChange("api_key", e.target.value)}
                  placeholder="输入 DashScope API Key" className={cn(inputClass, "pr-10")} />
                <button type="button" onClick={() => setShowToken(!showToken)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all">
                  {showToken ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">模型</label>
              <Select value={form.dashscope.model} options={DASHSCOPE_MODEL_OPTIONS}
                onChange={(v) => handleDashScopeChange("model", v)} />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">区域</label>
              <Select value={form.dashscope.region} options={REGION_OPTIONS}
                onChange={(v) => handleDashScopeChange("region", v)} />
              <p className="text-xs text-fg-3 mt-1">注意：北京和新加坡地域使用不同的 API Key</p>
            </div>
          </div>
        )}

        {form.provider === "qwen" && (
          <div className="border-t border-edge pt-4 space-y-4">
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">API Key</label>
              <div className="relative">
                <input type={showToken ? "text" : "password"} value={form.qwen.api_key}
                  onChange={(e) => handleQwenChange("api_key", e.target.value)}
                  placeholder="输入千问 API Key" className={cn(inputClass, "pr-10")} />
                <button type="button" onClick={() => setShowToken(!showToken)}
                  className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all">
                  {showToken ? <EyeOff size={16} /> : <Eye size={16} />}
                </button>
              </div>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">模型</label>
              <Select value={form.qwen.model} options={QWEN_MODEL_OPTIONS}
                onChange={(v) => handleQwenChange("model", v)} />
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">区域</label>
              <Select value={form.qwen.region} options={REGION_OPTIONS}
                onChange={(v) => handleQwenChange("region", v)} />
              <p className="text-xs text-fg-3 mt-1">注意：北京和新加坡地域使用不同的 API Key</p>
            </div>
            <div className="space-y-1.5">
              <label className="text-sm font-medium text-fg-2">语言</label>
              <Select value={form.qwen.language} options={QWEN_LANGUAGE_OPTIONS}
                onChange={(v) => handleQwenChange("language", v)} />
            </div>
          </div>
        )}
      </section>

      {/* Audio Source */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">音频来源</h3>
        <div className="space-y-2 mb-3">
          <RadioCard name="audio_source" value="microphone" checked={form.audio_source !== "system"}
            onChange={() => handleChange("audio_source", "microphone")}>
            <div className="text-sm font-medium text-fg">麦克风</div>
            <div className="text-xs text-fg-3">通过麦克风录入语音</div>
          </RadioCard>
          <RadioCard name="audio_source" value="system" checked={form.audio_source === "system"}
            onChange={() => handleChange("audio_source", "system")}>
            <div className="text-sm font-medium text-fg">系统声音</div>
            <div className="text-xs text-fg-3">录制系统正在播放的声音</div>
          </RadioCard>
        </div>

        {form.audio_source !== "system" && (
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-sm font-medium text-fg-2">麦克风</label>
              <Tooltip content="刷新设备列表">
                <button onClick={loadDevices} disabled={loadingDevices}
                  className="p-1 text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all disabled:opacity-50">
                  <RefreshCw size={14} className={loadingDevices ? "animate-spin" : ""} />
                </button>
              </Tooltip>
            </div>
            <Select
              value={form.device_name}
              options={[
                { label: "系统默认", value: "" },
                ...devices.map((dev) => ({ label: dev, value: dev })),
              ]}
              onChange={(v) => handleChange("device_name", v)}
            />
          </div>
        )}

        {form.audio_source !== "system" && (
          <ToggleCard
            checked={form.mic_always_on}
            onChange={(v) => handleChange("mic_always_on", v)}
            label="保持麦克风就绪"
            description={form.mic_always_on
              ? "麦克风常驻后台，录音响应更快"
              : "每次录音时临时打开，录音前有短暂延迟"}
            className="mt-3"
          />
        )}

        {form.audio_source === "system" && (
          <>
            <ToggleCard
              checked={form.system_no_auto_stop}
              onChange={(v) => {
                if (v && !form.system_no_auto_stop) {
                  setShowAutoStopWarning(true);
                } else {
                  handleChange("system_no_auto_stop", v);
                }
              }}
              label={<span className="flex items-center gap-1.5">禁用自动停止 <span className="text-[10px] px-1.5 py-0.5 rounded-full bg-warn-muted text-warn-muted-fg font-medium">实验性</span></span>}
              description="不再 30 秒自动取消，适合有长静音间隔的场景"
              className="mt-3"
            />
            {showAutoStopWarning && (
              <div className="mt-2 p-3 rounded-lg bg-warn-muted border border-warn-muted-fg/20 text-sm space-y-2">
                <p className="font-medium text-warn-muted-fg">确认开启实验性功能？</p>
                <p className="text-xs text-warn-muted-fg/80">开启后，系统声音录制将不再自动取消。这可能导致：</p>
                <ul className="text-xs text-warn-muted-fg/80 list-disc list-inside space-y-0.5">
                  <li>云端 ASR 产生超出预期的计费</li>
                  <li>录音会话长时间运行直到手动停止</li>
                </ul>
                <p className="text-xs text-warn-muted-fg/80">建议开启后实时关注云端 ASR 用量。</p>
                <div className="flex gap-2 pt-1">
                  <button
                    onClick={() => { setShowAutoStopWarning(false); handleChange("system_no_auto_stop", true); }}
                    className="px-3 py-1.5 text-xs font-medium rounded-md bg-warn text-white hover:opacity-90 active:scale-95 transition-all"
                  >
                    确认开启
                  </button>
                  <button
                    onClick={() => setShowAutoStopWarning(false)}
                    className="px-3 py-1.5 text-xs font-medium rounded-md text-fg-3 hover:text-fg hover:bg-surface-subtle active:scale-95 transition-all"
                  >
                    取消
                  </button>
                </div>
              </div>
            )}
          </>
        )}
      </section>

      {/* Input Settings */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">输入设置</h3>
        <div className="space-y-1.5 mb-4">
          <label className="text-sm font-medium text-fg-2">全局热键</label>
          <HotkeyRecorder value={form.hotkey} error={hotkeyError} onChange={onHotkeyChange} />
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium text-fg-2">录音模式</label>
          <div className="space-y-2">
            <RadioCard name="input_mode" value="toggle" checked={form.input_mode === "toggle"}
              onChange={() => handleChange("input_mode", "toggle")}>
              <div className="text-sm font-medium text-fg">切换模式</div>
              <div className="text-xs text-fg-3">按一次开始录音，再按一次停止</div>
            </RadioCard>
            <RadioCard name="input_mode" value="hold" checked={form.input_mode === "hold"}
              onChange={() => handleChange("input_mode", "hold")}>
              <div className="text-sm font-medium text-fg">按住模式</div>
              <div className="text-xs text-fg-3">按住热键说话，松开即停止</div>
            </RadioCard>
          </div>
        </div>
        <div className="mt-4">
          <ToggleCard
            checked={form.esc_abort_enabled}
            onChange={(v) => handleChange("esc_abort_enabled", v)}
            label="按 Esc 强制结束会话"
            description="录音中按 Esc 取消本次；识别或 AI 优化中按 Esc 立即终止并丢弃本次结果"
          />
        </div>
        <div className="space-y-2 mt-4">
          <label className="text-sm font-medium text-fg-2">输出方式</label>
          <div className="space-y-2">
            <RadioCard name="output_mode" value="none" checked={form.output_mode === "none"}
              onChange={() => handleChange("output_mode", "none")}>
              <div className="text-sm font-medium text-fg">仅显示</div>
              <div className="text-xs text-fg-3">转写结果仅显示在窗口内</div>
            </RadioCard>
            <RadioCard name="output_mode" value="paste" checked={form.output_mode === "paste"}
              onChange={() => handleChange("output_mode", "paste")}>
              <div className="text-sm font-medium text-fg">粘贴输入</div>
              <div className="text-xs text-fg-3">录音结束后自动粘贴到当前输入框（快速）</div>
            </RadioCard>
            <RadioCard name="output_mode" value="type" checked={form.output_mode === "type"}
              onChange={() => handleChange("output_mode", "type")}>
              <div className="text-sm font-medium text-fg">模拟键入</div>
              <div className="text-xs text-fg-3">录音结束后逐字输入到当前输入框</div>
            </RadioCard>
          </div>
        </div>
        <div className="mt-4 space-y-2">
          <ToggleCard
            checked={form.copy_to_clipboard}
            onChange={(v) => handleChange("copy_to_clipboard", v)}
            label="自动复制到剪贴板"
            description={form.output_mode === "paste"
              ? "粘贴后保留转写文本在剪贴板中"
              : "转写完成后自动复制文本到剪贴板"}
          />
          {form.output_mode === "paste" && !form.copy_to_clipboard && (
            <ToggleCard
              checked={form.paste_restore_clipboard}
              onChange={(v) => handleChange("paste_restore_clipboard", v)}
              label="粘贴后恢复剪贴板"
              description="粘贴完成后自动恢复之前复制的内容。关闭可避免短时间内新复制的内容被意外覆盖"
            />
          )}
        </div>
      </section>

      {/* Text Processing */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">文本处理</h3>
        <div className="space-y-2">
          <ToggleCard
            checked={form.filler_enabled}
            onChange={(v) => handleChange("filler_enabled", v)}
            label="过滤语气词"
            description="自动过滤嗯、呃、额等纯语气词"
          />
          <ToggleCard
            checked={form.replacement_enabled}
            onChange={(v) => handleChange("replacement_enabled", v)}
            label="文本替换"
            description="自动替换常见 ASR 误识别词"
          />
          {form.replacement_enabled && (
            <ToggleCard
              checked={form.replacement_ignore_case}
              onChange={(v) => handleChange("replacement_ignore_case", v)}
              label="忽略大小写"
              description="匹配时不区分大小写"
            />
          )}
        </div>
        <p className="text-xs text-fg-3 mt-1.5 px-3">
          开启 AI 优化后，语气词过滤将自动失效
        </p>

        {form.replacement_enabled && (
          <ReplacementsEditor />
        )}
      </section>

      {/* General Settings */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">通用</h3>
        <div className="space-y-2">
          <ToggleCard
            checked={autoStart}
            onChange={handleAutoStartChange}
            label="登录后自动启动"
            description="系统登录后自动运行 SpeakIn"
          />
        </div>
        <div className="space-y-1.5 mt-4">
          <label className="text-sm font-medium text-fg-2">关闭窗口时</label>
          <Select
            value={form.close_behavior}
            options={[
              { label: "每次询问", value: "ask" },
              { label: "最小化到托盘", value: "minimize" },
              { label: "退出应用", value: "quit" },
            ]}
            onChange={(v) => handleChange("close_behavior", v as "ask" | "minimize" | "quit")}
          />
        </div>
      </section>

      {/* Advanced */}
      <section>
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">高级</h3>
        <ToggleCard
          checked={form.debug_mode}
          onChange={(v) => handleChange("debug_mode", v)}
          label="调试模式"
          description="显示网络日志和 AI 请求日志"
        />
      </section>
    </div>
  );
}
