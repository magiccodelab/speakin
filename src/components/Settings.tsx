import { useState, useEffect, useRef, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Save, Eye, EyeOff, RefreshCw, Keyboard, ChevronDown } from "lucide-react";
import { motion } from "motion/react";
import { cn } from "../lib/utils";
import { Tooltip } from "./ui/Tooltip";
import { RadioIndicator } from "./ui/RadioGroup";
import type { AppSettings } from "../App";

interface SettingsProps {
  settings: AppSettings;
  onSave: (settings: AppSettings) => void;
  onClose: () => void;
}

const RESOURCE_OPTIONS = [
  { label: "ASR 1.0 小时版", value: "volc.bigasr.sauc.duration" },
  { label: "ASR 1.0 并发版", value: "volc.bigasr.sauc.concurrent" },
  { label: "ASR 2.0 小时版", value: "volc.seedasr.sauc.duration" },
  { label: "ASR 2.0 并发版", value: "volc.seedasr.sauc.concurrent" },
];

const PRESET_HOTKEYS = [
  "Ctrl+Shift+V",
  "Ctrl+Alt+R",
  "Ctrl+Shift+Space",
  "F2",
];

// Keys that should not be used as the sole trigger key
const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta"]);

// System-reserved combos to block
const RESERVED_COMBOS = new Set([
  "Ctrl+C", "Ctrl+V", "Ctrl+X", "Ctrl+Z", "Ctrl+A", "Ctrl+S",
  "Ctrl+W", "Ctrl+T", "Ctrl+N", "Ctrl+P", "Ctrl+F", "Ctrl+H",
  "Alt+F4", "Alt+Tab",
]);

/** Map KeyboardEvent.key to the display name used in hotkey strings */
function keyToName(e: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(e.key)) return null;
  if (e.key.startsWith("F") && /^F\d+$/.test(e.key)) return e.key;
  if (e.key.length === 1 && /[a-zA-Z0-9]/.test(e.key)) return e.key.toUpperCase();
  const map: Record<string, string> = {
    " ": "Space", Tab: "Tab", Enter: "Enter", Escape: "Escape",
    Backspace: "Backspace", Delete: "Delete", Insert: "Insert",
    Home: "Home", End: "End", PageUp: "PageUp", PageDown: "PageDown",
    ArrowUp: "Up", ArrowDown: "Down", ArrowLeft: "Left", ArrowRight: "Right",
    CapsLock: "CapsLock", NumLock: "NumLock", ScrollLock: "ScrollLock",
    PrintScreen: "PrintScreen", Pause: "Pause",
    "`": "`", "-": "-", "=": "=", "[": "[", "]": "]", "\\": "\\",
    ";": ";", "'": "'", ",": ",", ".": ".", "/": "/",
  };
  return map[e.key] ?? null;
}

/** Build a hotkey string from a KeyboardEvent */
function buildHotkeyString(e: KeyboardEvent): string | null {
  const keyName = keyToName(e);
  if (!keyName) return null;

  const parts: string[] = [];
  if (e.ctrlKey) parts.push("Ctrl");
  if (e.altKey) parts.push("Alt");
  if (e.shiftKey) parts.push("Shift");
  parts.push(keyName);

  return parts.join("+");
}

function HotkeyRecorder({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [recording, setRecording] = useState(false);
  const inputRef = useRef<HTMLDivElement>(null);

  const handleKeyDown = useCallback((e: KeyboardEvent) => {
    e.preventDefault();
    e.stopPropagation();

    if (e.key === "Escape") {
      setRecording(false);
      return;
    }

    const combo = buildHotkeyString(e);
    if (!combo) return;

    if (RESERVED_COMBOS.has(combo)) return;

    onChange(combo);
    setRecording(false);
  }, [onChange]);

  useEffect(() => {
    if (!recording) return;
    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [recording, handleKeyDown]);

  return (
    <div className="space-y-2">
      <div
        ref={inputRef}
        onClick={() => setRecording(true)}
        className={cn(
          "w-full px-3 py-2 text-sm rounded-lg cursor-pointer select-none",
          "border text-center font-mono transition-all duration-[var(--t-fast)]",
          recording
            ? "bg-primary/10 border-primary text-primary shadow-[0_0_0_3px_hsl(var(--primary)/0.14)]"
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
            onClick={() => { onChange(key); setRecording(false); }}
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
    </div>
  );
}

const inputClass = cn(
  "w-full px-3 py-2 text-sm rounded-lg",
  "bg-surface border border-edge text-fg",
  "placeholder:text-fg-3/60",
  "focus:border-[hsl(var(--primary)/0.5)] focus:shadow-[0_0_0_3px_hsl(var(--primary)/0.14)] focus:outline-none",
  "transition-all duration-[var(--t-fast)]"
);

/** Radio option card — uses a visually-hidden input that doesn't cause scroll jumps. */
function RadioCard({
  name, value, checked, onChange, children,
}: {
  name: string; value: string; checked: boolean;
  onChange: () => void; children: React.ReactNode;
}) {
  return (
    <label className="flex items-center gap-3 p-3 rounded-lg border border-edge hover:bg-surface-subtle transition-colors cursor-pointer">
      {/* Hidden radio: use fixed positioning to prevent browser scroll-to-focus */}
      <input
        type="radio"
        name={name}
        value={value}
        checked={checked}
        onChange={onChange}
        className="fixed opacity-0 pointer-events-none"
        tabIndex={-1}
      />
      <RadioIndicator checked={checked} />
      <div className="flex-1 min-w-0">{children}</div>
    </label>
  );
}

export function Settings({ settings, onSave, onClose }: SettingsProps) {
  const [form, setForm] = useState<AppSettings>({ ...settings });
  const [showToken, setShowToken] = useState(false);
  const [saved, setSaved] = useState(false);
  const [devices, setDevices] = useState<string[]>([]);
  const [loadingDevices, setLoadingDevices] = useState(false);

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

  const handleSave = () => {
    onSave(form);
    setSaved(true);
    setTimeout(() => setSaved(false), 2000);
  };

  const handleChange = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
  };

  return (
    <div className="flex flex-col h-full overflow-hidden">
      <div className="flex items-center justify-between px-5 py-4 border-b border-edge shrink-0">
        <h2 className="text-base font-semibold text-fg">设置</h2>
        <button onClick={onClose} className="p-1.5 rounded-md text-fg-3 hover:text-fg hover:bg-surface-subtle active:scale-95 transition-all">
          <X size={18} />
        </button>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto px-5 py-4 space-y-5">
        {/* API Configuration */}
        <section>
          <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">API 配置</h3>

          <div className="space-y-1.5 mb-4">
            <label className="text-sm font-medium text-fg-2">App ID</label>
            <input type="text" value={form.app_id} onChange={(e) => handleChange("app_id", e.target.value)}
              placeholder="输入 App ID" className={inputClass} />
          </div>

          <div className="space-y-1.5 mb-4">
            <label className="text-sm font-medium text-fg-2">Access Token</label>
            <div className="relative">
              <input type={showToken ? "text" : "password"} value={form.access_token}
                onChange={(e) => handleChange("access_token", e.target.value)}
                placeholder="输入 Access Token" className={cn(inputClass, "pr-10")} />
              <button type="button" onClick={() => setShowToken(!showToken)}
                className="absolute right-2 top-1/2 -translate-y-1/2 p-1 text-fg-3 hover:text-fg-2 active:scale-90 transition-all">
                {showToken ? <EyeOff size={16} /> : <Eye size={16} />}
              </button>
            </div>
          </div>

          <div className="space-y-1.5">
            <label className="text-sm font-medium text-fg-2">资源 ID</label>
            <div className="relative">
              <select value={form.resource_id} onChange={(e) => handleChange("resource_id", e.target.value)}
                className={cn(inputClass, "appearance-none pr-9 cursor-pointer")}>
                {RESOURCE_OPTIONS.map((opt) => (
                  <option key={opt.value} value={opt.value}>{opt.label}</option>
                ))}
              </select>
              <ChevronDown size={14} className="absolute right-3 top-1/2 -translate-y-1/2 text-fg-3 pointer-events-none" />
            </div>
          </div>
        </section>

        {/* Audio Device */}
        <section>
          <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">音频设备</h3>
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label className="text-sm font-medium text-fg-2">麦克风</label>
              <Tooltip content="刷新设备列表">
                <button onClick={loadDevices} disabled={loadingDevices}
                  className="p-1 text-fg-3 hover:text-fg-2 active:scale-90 transition-all disabled:opacity-50">
                  <RefreshCw size={14} className={loadingDevices ? "animate-spin" : ""} />
                </button>
              </Tooltip>
            </div>
            <div className="relative">
              <select value={form.device_name} onChange={(e) => handleChange("device_name", e.target.value)}
                className={cn(inputClass, "appearance-none pr-9 cursor-pointer")}>
                <option value="">系统默认</option>
                {devices.map((dev) => (
                  <option key={dev} value={dev}>{dev}</option>
                ))}
              </select>
              <ChevronDown size={14} className="absolute right-3 top-1/2 -translate-y-1/2 text-fg-3 pointer-events-none" />
            </div>
          </div>

          <label className="flex items-center justify-between p-3 mt-3 rounded-lg border border-edge hover:bg-surface-subtle transition-colors cursor-pointer">
            <div className="flex-1 mr-3">
              <div className="text-sm font-medium text-fg">保持麦克风就绪</div>
              <div className="text-xs text-fg-3">
                {form.mic_always_on
                  ? "麦克风常驻后台，录音响应更快"
                  : "每次录音时临时打开，录音前有短暂延迟"}
              </div>
            </div>
            <div className="relative shrink-0">
              <input type="checkbox" checked={form.mic_always_on}
                onChange={(e) => handleChange("mic_always_on", e.target.checked)}
                className="fixed opacity-0 pointer-events-none" tabIndex={-1} />
              <div className={cn(
                "w-9 h-5 rounded-full transition-colors duration-[var(--t-fast)]",
                form.mic_always_on ? "bg-primary" : "bg-fg-3/30"
              )} />
              <div className={cn(
                "absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow-sm",
                "transition-transform duration-[var(--t-fast)]",
                form.mic_always_on ? "translate-x-4" : ""
              )} />
            </div>
          </label>
        </section>

        {/* Input Settings */}
        <section>
          <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">输入设置</h3>

          <div className="space-y-1.5 mb-4">
            <label className="text-sm font-medium text-fg-2">全局热键</label>
            <HotkeyRecorder
              value={form.hotkey}
              onChange={(v) => handleChange("hotkey", v)}
            />
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
        </section>
      </div>

      <div className="px-5 py-4 border-t border-edge shrink-0">
        <button onClick={handleSave}
          className={cn("w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium transition-all duration-[var(--t-base)]",
            saved ? "bg-ok-muted text-ok-muted-fg" : "bg-primary text-primary-fg hover:shadow-[0_0_24px_-4px_hsl(var(--primary)/0.4)] hover:-translate-y-px active:translate-y-0 active:shadow-none")}>
          <Save size={16} />
          {saved ? "已保存" : "保存设置"}
        </button>
      </div>
    </div>
  );
}
