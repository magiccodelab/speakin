import { Select, type SelectGroup } from "../ui/Select";
import { ToggleCard } from "../ui/Toggle";
import type { AppSettings, AiOptimizeSettings } from "../../App";
import type { AiProvider } from "../../lib/ai-providers";
import type { PromptTemplate } from "../../lib/prompts";

interface AiOptimizeTabProps {
  form: AppSettings;
  handleChange: <K extends keyof AppSettings>(key: K, value: AppSettings[K]) => void;
  providers: AiProvider[];
  prompts: PromptTemplate[];
  providersLoaded?: boolean;
  validationError?: string | null;
}

export function AiOptimizeTab({
  form,
  handleChange,
  providers,
  prompts,
  providersLoaded = true,
  validationError,
}: AiOptimizeTabProps) {
  const handleAiChange = <K extends keyof AiOptimizeSettings>(key: K, value: AiOptimizeSettings[K]) => {
    handleChange("ai_optimize" as keyof AppSettings, { ...form.ai_optimize, [key]: value } as any);
  };

  const hasProviders = providers.length > 0;
  const activeProviderExists = providers.some((p) => p.id === form.ai_optimize.active_provider_id);
  const selectedProviderId = activeProviderExists ? form.ai_optimize.active_provider_id : "";
  const providerOptions = [
    { label: "请选择 AI 供应商", value: "" },
    ...providers.map((p) => ({ label: p.name, value: p.id })),
  ];
  const canToggleAiOptimize = form.ai_optimize.enabled || (providersLoaded && hasProviders);
  const enableDescription = !providersLoaded
    ? "正在加载 AI 供应商"
    : !hasProviders
      ? "先在「供应商」页签添加 AI 供应商后才能开启"
      : form.ai_optimize.enabled
        ? "录音结束后自动通过 AI 优化转写文本"
        : "关闭后直接输出原始转写文本";

  // Group prompts by category, preserving first-seen order
  const promptGroups: SelectGroup[] = (() => {
    const map = new Map<string, SelectGroup>();
    for (const p of prompts) {
      const cat = p.category?.trim() || "未分类";
      let group = map.get(cat);
      if (!group) {
        group = { label: cat, options: [] };
        map.set(cat, group);
      }
      group.options.push({ label: p.name, value: p.id });
    }
    return Array.from(map.values());
  })();

  return (
    <div className="space-y-5">
      {/* Enable toggle */}
      <ToggleCard
        checked={form.ai_optimize.enabled}
        onChange={(v) => handleAiChange("enabled", v)}
        label="启用 AI 优化"
        description={enableDescription}
        disabled={!canToggleAiOptimize}
      />

      {form.ai_optimize.enabled && (
        <div className="space-y-4">
          {/* Provider Selection */}
          <section>
            <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">AI 供应商</h3>
            {hasProviders ? (
              <Select value={selectedProviderId} options={providerOptions}
                onChange={(v) => handleAiChange("active_provider_id", v)} />
            ) : (
              <p className="text-sm text-fg-3 py-2">
                {providersLoaded ? "还没有 AI 供应商，前往「供应商」页签添加" : "正在加载 AI 供应商..."}
              </p>
            )}
            {validationError && (
              <p className="text-xs text-danger-muted-fg mt-1.5">{validationError}</p>
            )}
          </section>

          {/* Prompt Selection */}
          <section>
            <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">提示词</h3>
            {prompts.length > 0 ? (
              <Select value={form.ai_optimize.active_prompt_id} groups={promptGroups}
                onChange={(v) => handleAiChange("active_prompt_id", v)} />
            ) : (
              <p className="text-sm text-fg-3 py-2">还没有提示词，前往「提示词」页签创建</p>
            )}
            <p className="text-xs text-fg-3 mt-1.5">在「提示词」页签中管理提示词模板</p>
          </section>

          {/* Timeout Settings */}
          <section>
            <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest mb-3">超时设置</h3>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm text-fg">连接超时</p>
                  <p className="text-xs text-fg-3">建立初始连接的最大等待时间</p>
                </div>
                <div className="flex items-center gap-1.5">
                  <input
                    type="number"
                    min={1} max={30}
                    value={form.ai_optimize.connect_timeout_secs}
                    onChange={(e) => handleAiChange("connect_timeout_secs", Math.max(1, Math.min(30, Number(e.target.value) || 5)))}
                    className="w-16 text-center text-sm px-2 py-1.5 rounded-lg bg-surface-subtle border border-edge focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                  <span className="text-sm text-fg-3">秒</span>
                </div>
              </div>
              <div className="flex items-center justify-between">
                <div>
                  <p className="text-sm text-fg">最大请求时间</p>
                  <p className="text-xs text-fg-3">整个 AI 请求的最大耗时</p>
                </div>
                <div className="flex items-center gap-1.5">
                  <input
                    type="number"
                    min={5} max={300}
                    value={form.ai_optimize.max_request_secs}
                    onChange={(e) => handleAiChange("max_request_secs", Math.max(5, Math.min(300, Number(e.target.value) || 60)))}
                    className="w-16 text-center text-sm px-2 py-1.5 rounded-lg bg-surface-subtle border border-edge focus:outline-none focus:ring-1 focus:ring-primary"
                  />
                  <span className="text-sm text-fg-3">秒</span>
                </div>
              </div>
            </div>
          </section>
        </div>
      )}
    </div>
  );
}
