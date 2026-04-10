import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { X, Save, Mic, Sparkles, Server, MessageSquareText, BarChart3 } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "../lib/utils";
import { normalizeHotkeyString, validateHotkeyString } from "../lib/hotkey";
import { AsrSettingsTab } from "./settings/AsrSettingsTab";
import { AiOptimizeTab } from "./settings/AiOptimizeTab";
import { AiProvidersTab } from "./settings/AiProvidersTab";
import { PromptsTab } from "./settings/PromptsTab";
import { StatsTab } from "./settings/StatsTab";
import type { AppSettings } from "../App";
import type { AiProvider, AiProvidersFile } from "../lib/ai-providers";
import type { PromptTemplate, PromptsFile } from "../lib/prompts";

interface SettingsProps {
  settings: AppSettings;
  onSave: (settings: AppSettings) => Promise<AppSettings>;
  onClose: () => void;
  initialTab?: string;
  isRecording?: boolean;
}

type TabId = "asr" | "ai" | "ai-providers" | "prompts" | "stats";

const TABS: { id: TabId; label: string; icon: LucideIcon }[] = [
  { id: "asr", label: "识别", icon: Mic },
  { id: "ai", label: "AI 优化", icon: Sparkles },
  { id: "ai-providers", label: "供应商", icon: Server },
  { id: "prompts", label: "提示词", icon: MessageSquareText },
  { id: "stats", label: "统计", icon: BarChart3 },
];

function getAiOptimizeValidationError(
  form: AppSettings,
  aiProviders: AiProvider[],
  providersLoaded: boolean,
): string | null {
  if (!form.ai_optimize.enabled) return null;
  if (!providersLoaded) return "正在加载 AI 供应商，请稍后再保存";
  if (aiProviders.length === 0) return "启用 AI 优化前，请先添加 AI 供应商";
  if (!form.ai_optimize.active_provider_id.trim()) return "启用 AI 优化前，请先选择 AI 供应商";
  if (!aiProviders.some((p) => p.id === form.ai_optimize.active_provider_id)) {
    return "所选 AI 供应商不存在，请重新选择";
  }
  return null;
}

export function Settings({ settings, onSave, onClose, initialTab, isRecording = false }: SettingsProps) {
  const [activeTab, setActiveTab] = useState<TabId>((initialTab as TabId) || "asr");
  const [form, setForm] = useState<AppSettings>({ ...settings });
  const [saved, setSaved] = useState(false);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [hotkeyError, setHotkeyError] = useState<string | null>(null);
  const [prompts, setPrompts] = useState<PromptTemplate[]>([]);
  const [aiProviders, setAiProviders] = useState<AiProvider[]>([]);
  const [aiProvidersLoaded, setAiProvidersLoaded] = useState(false);
  const savedTimerRef = useRef<number | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);

  // Load prompts and AI providers on mount
  useEffect(() => {
    invoke<PromptsFile>("get_prompts")
      .then((data) => setPrompts(data.prompts))
      .catch(() => {});
    invoke<AiProvidersFile>("get_ai_providers")
      .then((data) => setAiProviders(data.providers))
      .catch(() => {})
      .finally(() => setAiProvidersLoaded(true));
  }, []);

  useEffect(() => () => {
    if (savedTimerRef.current !== null) {
      window.clearTimeout(savedTimerRef.current);
    }
  }, []);

  const handleChange = <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => {
    setForm((prev) => ({ ...prev, [key]: value }));
    setSaved(false);
    setSaveError(null);
  };

  const handleHotkeyChange = (value: string) => {
    const normalized = normalizeHotkeyString(value);
    const error = validateHotkeyString(value);
    if (!normalized || error) {
      setHotkeyError(error ?? "快捷键格式无效，请重新录制");
      setSaved(false);
      setSaveError(null);
      return false;
    }
    setHotkeyError(null);
    handleChange("hotkey", normalized);
    return true;
  };

  const handleSaveSettings = async () => {
    if (activeTab === "asr") {
      const error = validateHotkeyString(form.hotkey);
      if (error) {
        setHotkeyError(error);
        setSaveError(error);
        return;
      }
    }

    const aiError = getAiOptimizeValidationError(form, aiProviders, aiProvidersLoaded);
    if (aiError) {
      setSaveError(aiError);
      setSaved(false);
      return;
    }

    setSaving(true);
    try {
      const savedSettings = await onSave(form);
      setForm(savedSettings);
      setHotkeyError(null);
      setSaveError(null);
      setSaved(true);
      if (savedTimerRef.current !== null) {
        window.clearTimeout(savedTimerRef.current);
      }
      savedTimerRef.current = window.setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
      setSaved(false);
    } finally {
      setSaving(false);
    }
  };

  const handleSavePrompts = async (updatedPrompts: PromptTemplate[]) => {
    await invoke("save_prompts", {
      promptsData: { prompts: updatedPrompts },
    });
    setPrompts(updatedPrompts);
    invoke("rebuild_tray_menu_cmd").catch(() => {});
  };

  const showSaveButton = activeTab === "asr" || activeTab === "ai";
  const aiOptimizeError = getAiOptimizeValidationError(form, aiProviders, aiProvidersLoaded);
  const saveDisabled = saving || !!hotkeyError || !!aiOptimizeError;

  return (
    <div className="flex flex-col flex-1 min-h-0 overflow-hidden">
      {/* Header */}
      <div className="flex items-center justify-between px-5 py-4 border-b border-edge shrink-0">
        <h2 className="text-base font-semibold text-fg">设置</h2>
        <button onClick={onClose} aria-label="关闭设置" className="p-1.5 rounded-md text-fg-3 hover:text-fg hover:bg-surface-subtle active:scale-95 transition-all">
          <X size={18} />
        </button>
      </div>

      {/* Tab Bar */}
      <div role="tablist" className="flex justify-center shrink-0 px-4 py-2 border-b border-edge">
        <div className="inline-flex items-center gap-0.5 p-0.5 rounded-lg bg-surface-inset">
          {TABS.map((tab) => (
            <button
              key={tab.id}
              role="tab"
              aria-selected={activeTab === tab.id}
              aria-controls={`tabpanel-${tab.id}`}
              onClick={() => { setActiveTab(tab.id); scrollRef.current?.scrollTo(0, 0); }}
              className={cn(
                "inline-flex items-center gap-1.5 px-3 py-1.5 text-[13px] font-medium whitespace-nowrap rounded-md transition-all duration-[var(--t-base)]",
                activeTab === tab.id
                  ? "bg-surface text-primary shadow-sm"
                  : "text-fg-3 hover:text-fg active:scale-[0.97]"
              )}
            >
              <tab.icon size={14} className="shrink-0" />
              {tab.label}
            </button>
          ))}
        </div>
      </div>

      {/* Tab Content */}
      <div ref={scrollRef} className="flex-1 min-h-0 overflow-y-auto relative">
        <AnimatePresence initial={false}>
          <motion.div
            key={activeTab}
            role="tabpanel"
            id={`tabpanel-${activeTab}`}
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            transition={{ duration: 0.15 }}
            className="px-5 py-4"
          >
            {activeTab === "asr" && (
              <AsrSettingsTab
                form={form}
                handleChange={handleChange}
                hotkeyError={hotkeyError}
                onHotkeyChange={handleHotkeyChange}
                isRecording={isRecording}
              />
            )}
            {activeTab === "ai" && (
              <AiOptimizeTab
                form={form}
                handleChange={handleChange}
                providers={aiProviders}
                prompts={prompts}
                providersLoaded={aiProvidersLoaded}
                validationError={aiOptimizeError}
              />
            )}
            {activeTab === "ai-providers" && (
              <AiProvidersTab
                providers={aiProviders}
                onProvidersChange={(providers) => {
                  setAiProviders(providers);
                  setSaveError(null);
                }}
                onAiSettingsSync={(aiSettings) => {
                  setForm((prev) => ({ ...prev, ai_optimize: aiSettings }));
                  setSaveError(null);
                }}
              />
            )}
            {activeTab === "prompts" && (
              <PromptsTab
                prompts={prompts}
                activePromptId={form.ai_optimize.active_prompt_id}
                onActivePromptIdChange={(id) => {
                  setForm((prev) => {
                    const updated = {
                      ...prev,
                      ai_optimize: { ...prev.ai_optimize, active_prompt_id: id },
                    };
                    // Persist immediately since prompts tab has no save button
                    onSave(updated).catch(() => {});
                    return updated;
                  });
                }}
                onSave={handleSavePrompts}
              />
            )}
            {activeTab === "stats" && <StatsTab />}
          </motion.div>
        </AnimatePresence>
      </div>

      {/* Footer — save button for settings tabs */}
      <AnimatePresence initial={false}>
        {showSaveButton && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            transition={{ duration: 0.15 }}
            className="shrink-0 overflow-hidden"
          >
            <div className="px-5 py-4 border-t border-edge">
              {(saveError || aiOptimizeError) && (
                <p className="mb-2 text-sm text-danger-muted-fg">{saveError || aiOptimizeError}</p>
              )}
              <button onClick={handleSaveSettings} disabled={saveDisabled}
                className={cn("w-full flex items-center justify-center gap-2 px-4 py-2.5 rounded-lg text-sm font-medium transition-all duration-[var(--t-base)]",
                  saved ? "bg-ok-muted text-ok-muted-fg" : "bg-primary text-primary-fg hover:shadow-[0_0_24px_-4px_hsl(var(--primary)/0.4)] hover:-translate-y-px active:translate-y-0 active:shadow-none",
                  "disabled:opacity-60 disabled:cursor-not-allowed disabled:hover:translate-y-0 disabled:hover:shadow-none")}>
                <Save size={16} />
                {saving ? "保存中..." : saved ? "已保存" : "保存设置"}
              </button>
            </div>
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  );
}
