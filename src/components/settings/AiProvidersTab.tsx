import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pencil, Trash2, Plus, Eye, EyeOff, Check, X as XIcon, Sparkles } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "../../lib/utils";
import type { AiProvider } from "../../lib/ai-providers";
import type { AiOptimizeSettings } from "../../App";

const AI_PROVIDER_GUIDE_URL = "https://afengblog.com/blog/speakin-ai.html";

interface AiProvidersTabProps {
  providers: AiProvider[];
  onProvidersChange: (providers: AiProvider[]) => void;
  onAiSettingsSync: (settings: AiOptimizeSettings) => void;
}

const inputClass = cn(
  "w-full px-3 py-2 text-sm rounded-lg",
  "bg-surface border border-edge text-fg",
  "placeholder:text-fg-3/60",
  "focus:border-[hsl(var(--primary)/0.5)] focus:shadow-[0_0_0_3px_hsl(var(--primary)/0.14)] focus:outline-none",
  "transition-all duration-[var(--t-fast)]"
);

const textareaClass = cn(inputClass, "min-h-[80px] resize-y font-mono text-xs");

interface EditForm {
  name: string;
  protocol: "openai" | "gemini";
  api_endpoint: string;
  api_key: string;
  model: string;
  stream: boolean;
  extra_body_text: string;
}

function emptyForm(): EditForm {
  return {
    name: "",
    protocol: "openai",
    api_endpoint: "",
    api_key: "",
    model: "",
    stream: true,
    extra_body_text: "{}",
  };
}

function providerToForm(p: AiProvider): EditForm {
  return {
    name: p.name,
    protocol: p.protocol ?? "openai",
    api_endpoint: p.api_endpoint,
    api_key: "",
    model: p.model,
    stream: p.stream,
    extra_body_text: JSON.stringify(p.extra_body, null, 2),
  };
}

interface TestResult {
  content: string;
  model: string;
  status: number;
  headers_time_ms: number;
  total_time_ms: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
}

const smallInputClass = cn(
  "flex-1 min-w-0 px-2.5 py-1.5 text-xs rounded-md",
  "bg-surface border border-edge text-fg",
  "placeholder:text-fg-3/60",
  "focus:border-[hsl(var(--primary)/0.5)] focus:shadow-[0_0_0_3px_hsl(var(--primary)/0.14)] focus:outline-none",
  "transition-all duration-[var(--t-fast)]"
);

function ExtraBodyEditor({ value, onChange }: { value: string; onChange: (v: string) => void }) {
  const [newKey, setNewKey] = useState("");
  const [newValue, setNewValue] = useState("");
  const [addError, setAddError] = useState<string | null>(null);

  // Parse current JSON to show existing entries
  let parsed: Record<string, unknown> = {};
  let parseOk = true;
  try {
    const obj = JSON.parse(value);
    if (typeof obj === "object" && obj !== null && !Array.isArray(obj)) {
      parsed = obj;
    } else {
      parseOk = false;
    }
  } catch {
    parseOk = false;
  }

  const entries = parseOk ? Object.entries(parsed) : [];

  const handleAdd = () => {
    const key = newKey.trim();
    if (!key) {
      setAddError("参数名不能为空");
      return;
    }

    // Smart value parsing: try JSON first, fallback to string
    let parsedValue: unknown;
    const raw = newValue.trim();
    if (raw === "") {
      parsedValue = "";
    } else if (raw === "true") {
      parsedValue = true;
    } else if (raw === "false") {
      parsedValue = false;
    } else if (raw === "null") {
      parsedValue = null;
    } else if (/^-?\d+(\.\d+)?$/.test(raw)) {
      parsedValue = Number(raw);
    } else {
      // Try JSON parse (for arrays/objects), else treat as string
      try {
        parsedValue = JSON.parse(raw);
      } catch {
        parsedValue = raw;
      }
    }

    const updated = { ...parsed, [key]: parsedValue };
    onChange(JSON.stringify(updated, null, 2));
    setNewKey("");
    setNewValue("");
    setAddError(null);
  };

  const handleRemoveEntry = (key: string) => {
    const updated = { ...parsed };
    delete updated[key];
    onChange(JSON.stringify(updated, null, 2));
  };

  return (
    <div className="space-y-1.5">
      <label className="text-sm font-medium text-fg-2">自定义参数 (JSON)</label>

      {/* Existing entries as tags */}
      {entries.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {entries.map(([k, v]) => (
            <span key={k} className="inline-flex items-center gap-1 px-2 py-1 rounded-md bg-surface-subtle border border-edge text-xs">
              <span className="font-medium text-fg-2">{k}</span>
              <span className="text-fg-3">=</span>
              <span className="text-fg-3 max-w-[120px] truncate">{JSON.stringify(v)}</span>
              <button onClick={() => handleRemoveEntry(k)} aria-label={`删除参数 ${k}`}
                className="ml-0.5 p-0.5 rounded text-fg-3 hover:text-danger transition-colors">
                <XIcon size={10} />
              </button>
            </span>
          ))}
        </div>
      )}

      {/* Quick add row */}
      {!parseOk && (
        <p className="text-xs text-danger">JSON 格式错误，请在下方手动修正后再添加参数</p>
      )}
      <div className="flex gap-1.5 items-start">
        <input type="text" value={newKey} onChange={(e) => { setNewKey(e.target.value); setAddError(null); }}
          placeholder="参数名" className={smallInputClass} disabled={!parseOk}
          onKeyDown={(e) => e.key === "Enter" && handleAdd()} />
        <input type="text" value={newValue} onChange={(e) => { setNewValue(e.target.value); setAddError(null); }}
          placeholder="值（如 0.7, true）" className={smallInputClass} disabled={!parseOk}
          onKeyDown={(e) => e.key === "Enter" && handleAdd()} />
        <button onClick={handleAdd} disabled={!parseOk} aria-label="添加参数"
          className={cn("shrink-0 px-2.5 py-1.5 rounded-md text-xs font-medium transition-all",
            "bg-primary/10 text-primary hover:bg-primary/20 active:scale-95",
            "disabled:opacity-50 disabled:cursor-not-allowed")}>
          <Plus size={12} />
        </button>
      </div>
      {addError && <p className="text-xs text-danger">{addError}</p>}

      {/* Raw JSON textarea for advanced users */}
      <details className="group" open={!parseOk || undefined}>
        <summary className="text-xs text-fg-3 cursor-pointer hover:text-fg-2 transition-colors select-none">
          手动编辑 JSON
        </summary>
        <textarea value={value} onChange={(e) => onChange(e.target.value)}
          placeholder='{"temperature": 0.7, "top_p": 0.8}' className={cn(textareaClass, "mt-1.5")} rows={4} />
      </details>

      <p className="text-xs text-fg-3">
        将合并到请求体中。可配置 temperature、top_p、enable_thinking 等参数
      </p>
    </div>
  );
}

export function AiProvidersTab({ providers, onProvidersChange, onAiSettingsSync }: AiProvidersTabProps) {
  const [editing, setEditing] = useState<{ form: EditForm; id: string | null } | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testError, setTestError] = useState<string | null>(null);
  const [keyStates, setKeyStates] = useState<Record<string, boolean>>({});
  const [showKeyInput, setShowKeyInput] = useState<string | null>(null);
  const [keyInput, setKeyInput] = useState("");
  const [showKey, setShowKey] = useState(false);

  // Load key states for all providers
  useEffect(() => {
    providers.forEach((p) => {
      invoke<boolean>("has_ai_provider_key", { id: p.id })
        .then((has) => setKeyStates((prev) => ({ ...prev, [p.id]: has })))
        .catch(() => {});
    });
  }, [providers]);

  const handleNew = () => {
    setEditing({ form: emptyForm(), id: null });
    setError(null);
  };

  const handleEdit = (p: AiProvider) => {
    setEditing({ form: providerToForm(p), id: p.id });
    setError(null);
  };

  const handleDelete = async (p: AiProvider) => {
    setSaving(true);
    setError(null);
    try {
      const updatedAiSettings = await invoke<AiOptimizeSettings>("delete_ai_provider", { id: p.id });
      onProvidersChange(providers.filter((x) => x.id !== p.id));
      // Sync form state: if the deleted provider was active, backend already disabled AI
      onAiSettingsSync(updatedAiSettings);
    } catch (e) {
      setError(String(e));
    }
    setSaving(false);
  };

  const handleSaveEdit = async () => {
    if (!editing) return;
    const { form, id } = editing;

    // Parse extra_body JSON
    let extra_body: Record<string, unknown>;
    try {
      extra_body = JSON.parse(form.extra_body_text);
      if (typeof extra_body !== "object" || Array.isArray(extra_body) || extra_body === null) {
        throw new Error("必须是 JSON 对象");
      }
    } catch (e) {
      setError(`自定义参数 JSON 格式错误: ${e}`);
      return;
    }

    setSaving(true);
    setError(null);
    try {
      const provider: AiProvider = {
        id: id ?? "",
        name: form.name,
        protocol: form.protocol,
        api_endpoint: form.api_endpoint,
        model: form.model,
        stream: form.stream,
        extra_body,
      };

      let savedId: string;
      if (id) {
        // Update existing
        await invoke("update_ai_provider", { provider });
        onProvidersChange(providers.map((p) => (p.id === id ? { ...provider, id } : p)));
        savedId = id;
      } else {
        // Add new (backend generates ID)
        const created = await invoke<AiProvider>("add_ai_provider", { provider });
        onProvidersChange([...providers, created]);
        savedId = created.id;
      }

      // Save API key if provided (non-empty means user entered a new key)
      if (form.api_key.trim()) {
        await invoke("set_ai_provider_key", { id: savedId, key: form.api_key.trim() });
        setKeyStates((prev) => ({ ...prev, [savedId]: true }));
      }

      setEditing(null);
    } catch (e) {
      setError(String(e));
    }
    setSaving(false);
  };

  const handleSaveKey = async (providerId: string) => {
    if (!keyInput.trim()) return;
    try {
      await invoke("set_ai_provider_key", { id: providerId, key: keyInput.trim() });
      setKeyStates((prev) => ({ ...prev, [providerId]: true }));
      setKeyInput("");
      setShowKeyInput(null);
    } catch (e) {
      setError(String(e));
    }
  };

  const handleClearKey = async (providerId: string) => {
    try {
      await invoke("clear_ai_provider_key", { id: providerId });
      setKeyStates((prev) => ({ ...prev, [providerId]: false }));
    } catch (e) {
      setError(String(e));
    }
  };

  // Editor view
  if (editing) {
    const { form } = editing;
    return (
      <div className="space-y-4">
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest">
          {editing.id ? "编辑 AI 供应商" : "新建 AI 供应商"}
        </h3>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">名称</label>
          <input type="text" value={form.name}
            onChange={(e) => setEditing({ ...editing, form: { ...form, name: e.target.value } })}
            placeholder="例如：阿里云 Qwen3.5 Flash" className={inputClass} />
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">协议</label>
          <div className="flex gap-2">
            {(["openai", "gemini"] as const).map((p) => (
              <button key={p} type="button"
                onClick={() => {
                  const updates: Partial<EditForm> = { protocol: p };
                  // Auto-fill defaults when switching protocol
                  if (p === "gemini" && form.protocol !== "gemini") {
                    updates.api_endpoint = "https://generativelanguage.googleapis.com/v1beta";
                    updates.model = "gemini-2.5-flash-lite";
                  } else if (p === "openai" && form.protocol !== "openai") {
                    if (form.api_endpoint === "https://generativelanguage.googleapis.com/v1beta") {
                      updates.api_endpoint = "";
                    }
                    if (form.model.startsWith("gemini")) {
                      updates.model = "";
                    }
                  }
                  setEditing({ ...editing, form: { ...form, ...updates } });
                }}
                className={cn(
                  "flex-1 px-3 py-2 text-sm font-medium rounded-lg border transition-all duration-[var(--t-fast)]",
                  form.protocol === p
                    ? "border-primary bg-primary/8 text-primary"
                    : "border-edge text-fg-3 hover:border-edge-strong hover:bg-surface-subtle"
                )}>
                {p === "openai" ? "OpenAI 兼容" : "Gemini"}
              </button>
            ))}
          </div>
          {form.protocol === "gemini" && (
            <p className="text-xs text-primary/80">
              为提升速度，建议使用 gemini-2.5-flash-lite 或 gemini-2.5-flash
            </p>
          )}
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">API 端点</label>
          <input type="text" value={form.api_endpoint}
            onChange={(e) => setEditing({ ...editing, form: { ...form, api_endpoint: e.target.value } })}
            placeholder={form.protocol === "gemini"
              ? "https://generativelanguage.googleapis.com/v1beta"
              : "https://api.openai.com/v1"}
            className={inputClass} />
          <p className="text-xs text-fg-3">
            {form.protocol === "gemini"
              ? "不需要包含 /models/ 路径"
              : "不需要包含 /chat/completions 路径"}
          </p>
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">API Key</label>
          <div className="relative">
            <input type={showKey ? "text" : "password"} value={form.api_key}
              onChange={(e) => setEditing({ ...editing, form: { ...form, api_key: e.target.value } })}
              placeholder={editing.id && keyStates[editing.id] ? "已配置（留空保持不变）" : "输入 API Key"}
              className={cn(inputClass, "pr-10")} />
            <button type="button" onClick={() => setShowKey(!showKey)}
              className="absolute right-2 top-1/2 -translate-y-1/2 p-1 rounded text-fg-3 hover:text-fg-2 hover:bg-surface-subtle active:scale-95 transition-all">
              {showKey ? <EyeOff size={14} /> : <Eye size={14} />}
            </button>
          </div>
          {editing.id && keyStates[editing.id] && (
            <p className="text-xs text-ok-muted-fg">已配置，留空则保持现有 Key 不变</p>
          )}
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">模型</label>
          <input type="text" value={form.model}
            onChange={(e) => setEditing({ ...editing, form: { ...form, model: e.target.value } })}
            placeholder={form.protocol === "gemini" ? "gemini-2.5-flash-lite" : "gpt-4o-mini"}
            className={inputClass} />
        </div>

        <label className="flex items-center justify-between p-3 rounded-lg border border-edge hover:bg-surface-subtle active:scale-[0.98] transition-all duration-[var(--t-fast)] cursor-pointer focus-within:ring-2 focus-within:ring-primary focus-within:ring-offset-2 focus-within:ring-offset-surface">
          <div>
            <div className="text-sm font-medium text-fg">流式输出</div>
            <div className="text-xs text-fg-3">实时显示优化结果</div>
          </div>
          <div className="relative shrink-0">
            <input type="checkbox" checked={form.stream}
              onChange={(e) => setEditing({ ...editing, form: { ...form, stream: e.target.checked } })}
              className="fixed opacity-0 pointer-events-none" />
            <div className={cn(
              "w-9 h-5 rounded-full transition-colors duration-[var(--t-fast)]",
              form.stream ? "bg-primary" : "bg-fg-3/30"
            )} />
            <div className={cn(
              "absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow-sm",
              "transition-transform duration-[var(--t-fast)]",
              form.stream ? "translate-x-4" : ""
            )} />
          </div>
        </label>

        <ExtraBodyEditor
          value={form.extra_body_text}
          onChange={(text) => setEditing({ ...editing, form: { ...form, extra_body_text: text } })}
        />

        {/* Test Section */}
        <div className="border-t border-edge pt-4 space-y-3">
          <div className="flex items-center justify-between">
            <h4 className="text-xs font-semibold text-fg-3 uppercase tracking-widest">模型测试</h4>
            <button
              onClick={async () => {
                // Parse extra_body for test
                let extra_body: Record<string, unknown> = {};
                try {
                  extra_body = JSON.parse(form.extra_body_text);
                } catch { /* ignore parse error for test */ }

                // Determine API key: use form input, or load from keyring if editing existing
                let testKey = form.api_key.trim();
                if (!testKey && editing.id && keyStates[editing.id]) {
                  // No new key entered but existing key configured — need to inform user
                  setTestError("请输入 API Key（已保存的 Key 不会加载到表单中）");
                  return;
                }
                if (!testKey) {
                  setTestError("请先输入 API Key");
                  return;
                }

                setTesting(true);
                setTestResult(null);
                setTestError(null);
                try {
                  const result = await invoke<TestResult>("test_ai_provider", {
                    provider: {
                      id: editing.id ?? "",
                      name: form.name,
                      protocol: form.protocol,
                      api_endpoint: form.api_endpoint,
                      model: form.model,
                      stream: form.stream,
                      extra_body,
                    },
                    apiKey: testKey,
                  });
                  setTestResult(result);
                } catch (e) {
                  setTestError(String(e));
                }
                setTesting(false);
              }}
              disabled={testing || !form.api_endpoint.trim() || !form.model.trim()}
              className={cn(
                "px-3 py-1.5 rounded-md text-xs font-medium transition-all",
                "bg-surface-subtle border border-edge text-fg-2",
                "hover:bg-surface-inset hover:text-fg active:scale-95",
                "disabled:opacity-50 disabled:cursor-not-allowed"
              )}
            >
              {testing ? "测试中..." : "发送测试"}
            </button>
          </div>

          {testError && (
            <div className="px-3 py-2 rounded-lg bg-danger-muted text-danger-muted-fg text-xs">
              {testError}
            </div>
          )}

          {testResult && (
            <div className="rounded-lg border border-edge bg-surface-subtle/60 overflow-hidden">
              <div className="px-3 py-2 space-y-1.5 text-xs">
                <div className="flex items-center gap-2">
                  <span className={cn(
                    "px-1.5 py-0.5 rounded font-medium",
                    testResult.status === 200 ? "bg-ok-muted text-ok-muted-fg" : "bg-danger-muted text-danger-muted-fg"
                  )}>
                    HTTP {testResult.status}
                  </span>
                  <span className="text-fg-3">{testResult.total_time_ms} ms</span>
                  <span className="text-fg-3">·</span>
                  <span className="text-fg-3">首包 {testResult.headers_time_ms} ms</span>
                </div>
                <div className="text-fg-2">
                  <span className="text-fg-3">模型：</span>{testResult.model}
                </div>
                <div className="text-fg-2">
                  <span className="text-fg-3">Tokens：</span>
                  {testResult.prompt_tokens} + {testResult.completion_tokens} = {testResult.total_tokens}
                </div>
                <div className="pt-1.5 border-t border-edge">
                  <span className="text-fg-3">响应：</span>
                  <span className="text-fg">{testResult.content}</span>
                </div>
              </div>
            </div>
          )}
        </div>

        {error && (
          <div className="px-3 py-2 rounded-lg bg-danger-muted text-danger-muted-fg text-sm">{error}</div>
        )}

        <div className="flex gap-2">
          <button onClick={handleSaveEdit} disabled={saving}
            className={cn("flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-all",
              "bg-primary text-primary-fg hover:opacity-90 active:scale-[0.98]",
              "disabled:opacity-50 disabled:cursor-not-allowed")}>
            {saving ? "保存中..." : "保存"}
          </button>
          <button onClick={() => { setEditing(null); setError(null); }}
            className="px-4 py-2 rounded-lg text-sm font-medium border border-edge text-fg-2 hover:bg-surface-subtle active:scale-[0.98] transition-all">
            取消
          </button>
        </div>
      </div>
    );
  }

  // List view
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest leading-none">AI 供应商管理</h3>
          <button
            onClick={() => openUrl(AI_PROVIDER_GUIDE_URL).catch(() => {})}
            className="inline-flex items-center gap-1 text-[11px] leading-none text-fg-3 hover:text-primary transition-colors"
          >
            <Sparkles size={11} />
            免费额度推荐 ↗
          </button>
        </div>
        <button onClick={handleNew}
          className="flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium text-primary bg-primary/10 hover:bg-primary/20 active:scale-95 transition-all">
          <Plus size={12} />
          新建
        </button>
      </div>

      {error && (
        <div className="px-3 py-2 rounded-lg bg-danger-muted text-danger-muted-fg text-sm">{error}</div>
      )}

      {providers.length === 0 ? (
        <div className="text-center py-8 text-fg-3 text-sm">
          还没有 AI 供应商，点击「新建」添加
        </div>
      ) : (
        <div className="space-y-2">
          {providers.map((p) => (
            <div key={p.id} className="p-3 rounded-lg border border-edge hover:bg-surface-subtle hover:border-edge-strong transition-colors">
              <div className="flex items-start gap-3">
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium text-fg">{p.name}</div>
                  <p className="text-xs text-fg-3 mt-0.5 truncate">{p.model} · {p.api_endpoint}</p>
                </div>
                <div className="flex gap-1 shrink-0">
                  <button onClick={() => handleEdit(p)}
                    className="p-1.5 rounded-md text-fg-3 hover:text-fg hover:bg-surface-inset active:scale-95 transition-all">
                    <Pencil size={13} />
                  </button>
                  <button onClick={() => handleDelete(p)} disabled={saving}
                    className="p-1.5 rounded-md text-fg-3 hover:text-danger hover:bg-danger-muted active:scale-95 transition-all disabled:opacity-50">
                    <Trash2 size={13} />
                  </button>
                </div>
              </div>

              {/* API Key management */}
              <div className="mt-2 pt-2 border-t border-edge">
                {keyStates[p.id] ? (
                  <div className="flex items-center gap-2">
                    <div className="flex items-center gap-1.5 text-xs text-ok-muted-fg">
                      <Check size={12} />
                      <span>API Key 已配置</span>
                    </div>
                    <button onClick={() => handleClearKey(p.id)}
                      className="ml-auto p-1 rounded text-fg-3 hover:text-danger transition-all">
                      <XIcon size={12} />
                    </button>
                  </div>
                ) : showKeyInput === p.id ? (
                  <div className="flex gap-2">
                    <div className="relative flex-1">
                      <input type={showKey ? "text" : "password"} value={keyInput}
                        onChange={(e) => setKeyInput(e.target.value)}
                        placeholder="输入 API Key" className={cn(inputClass, "pr-8 text-xs")} />
                      <button type="button" onClick={() => setShowKey(!showKey)}
                        className="absolute right-2 top-1/2 -translate-y-1/2 p-0.5 text-fg-3 hover:text-fg-2">
                        {showKey ? <EyeOff size={12} /> : <Eye size={12} />}
                      </button>
                    </div>
                    <button onClick={() => handleSaveKey(p.id)} disabled={!keyInput.trim()}
                      className={cn("px-2.5 py-1.5 rounded-lg text-xs font-medium transition-all",
                        "bg-primary text-primary-fg disabled:opacity-50")}>
                      保存
                    </button>
                    <button onClick={() => { setShowKeyInput(null); setKeyInput(""); }}
                      className="px-2 py-1.5 rounded-lg text-xs border border-edge text-fg-3 hover:text-fg-2 transition-all">
                      取消
                    </button>
                  </div>
                ) : (
                  <button onClick={() => { setShowKeyInput(p.id); setKeyInput(""); setShowKey(false); }}
                    className="text-xs text-primary hover:text-primary/80 transition-colors">
                    配置 API Key
                  </button>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
