import { useState, useMemo } from "react";
import { Pencil, Trash2, Plus, BookOpen } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "../../lib/utils";
import type { PromptTemplate } from "../../lib/prompts";

const PROMPT_GUIDE_URL = "https://afengblog.com/blog/speakin-prompt.html";

interface PromptsTabProps {
  prompts: PromptTemplate[];
  activePromptId: string;
  onActivePromptIdChange: (id: string) => void;
  onSave: (prompts: PromptTemplate[]) => Promise<void>;
}

const inputClass = cn(
  "w-full px-3 py-2 text-sm rounded-lg",
  "bg-surface border border-edge text-fg",
  "placeholder:text-fg-3/60",
  "focus:border-[hsl(var(--primary)/0.5)] focus:shadow-[0_0_0_3px_hsl(var(--primary)/0.14)] focus:outline-none",
  "transition-all duration-[var(--t-fast)]"
);

const textareaClass = cn(
  inputClass,
  "min-h-[80px] resize-y"
);

function generateId(): string {
  return crypto.randomUUID ? crypto.randomUUID() : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

/** Extract unique categories from prompts, ordered by first appearance. */
function getCategories(prompts: PromptTemplate[]): string[] {
  const seen = new Set<string>();
  const result: string[] = [];
  for (const p of prompts) {
    const cat = p.category || "其他";
    if (!seen.has(cat)) {
      seen.add(cat);
      result.push(cat);
    }
  }
  return result;
}

const ALL_FILTER = "全部";

export function PromptsTab({ prompts, activePromptId, onActivePromptIdChange, onSave }: PromptsTabProps) {
  const [editing, setEditing] = useState<PromptTemplate | null>(null);
  const [isNew, setIsNew] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [activeFilter, setActiveFilter] = useState(ALL_FILTER);
  const [customCategory, setCustomCategory] = useState("");

  const categories = useMemo(() => getCategories(prompts), [prompts]);

  const filteredPrompts = useMemo(() => {
    if (activeFilter === ALL_FILTER) return prompts;
    return prompts.filter((p) => (p.category || "其他") === activeFilter);
  }, [prompts, activeFilter]);

  // Group prompts by category (for "全部" view)
  const groupedPrompts = useMemo(() => {
    if (activeFilter !== ALL_FILTER) return null;
    const groups: Record<string, PromptTemplate[]> = {};
    for (const p of prompts) {
      const cat = p.category || "其他";
      (groups[cat] ??= []).push(p);
    }
    return groups;
  }, [prompts, activeFilter]);

  const handleNew = () => {
    const defaultCategory = activeFilter !== ALL_FILTER ? activeFilter : "";
    setEditing({
      id: generateId(),
      name: "",
      category: defaultCategory,
      system_prompt: "",
      user_prompt_template: "<input>\n{{text}}\n</input>",
      is_builtin: false,
    });
    setIsNew(true);
    setError(null);
    setCustomCategory("");
  };

  const handleEdit = (prompt: PromptTemplate) => {
    setEditing({ ...prompt });
    setIsNew(false);
    setError(null);
    setCustomCategory("");
  };

  const handleDelete = async (prompt: PromptTemplate) => {
    if (prompt.is_builtin) return;
    const updated = prompts.filter((p) => p.id !== prompt.id);
    setSaving(true);
    try {
      await onSave(updated);
      if (prompt.id === activePromptId && updated.length > 0) {
        onActivePromptIdChange(updated[0].id);
      }
    } catch (e) {
      setError(String(e));
    }
    setSaving(false);
  };

  const handleSaveEdit = async () => {
    if (!editing) return;

    if (!editing.name.trim()) {
      setError("提示词名称不能为空");
      return;
    }
    if (!editing.user_prompt_template.includes("{{text}}")) {
      setError("用户提示词模板必须包含 {{text}} 占位符");
      return;
    }

    // Resolve category: custom input takes precedence
    const finalCategory = customCategory.trim() || editing.category || "其他";
    const toSave = { ...editing, category: finalCategory };

    setSaving(true);
    setError(null);
    try {
      let updated: PromptTemplate[];
      if (isNew) {
        updated = [...prompts, toSave];
      } else {
        updated = prompts.map((p) => (p.id === toSave.id ? toSave : p));
      }
      await onSave(updated);
      setEditing(null);
    } catch (e) {
      setError(String(e));
    }
    setSaving(false);
  };

  const handleCancel = () => {
    setEditing(null);
    setError(null);
  };

  // ── Editor view ──
  if (editing) {
    const isCustomCategory = customCategory !== "";
    return (
      <div className="space-y-4">
        <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest">
          {isNew ? "新建提示词" : "编辑提示词"}
        </h3>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">名称</label>
          <input type="text" value={editing.name}
            onChange={(e) => setEditing({ ...editing, name: e.target.value })}
            placeholder="例如：会议纪要" className={inputClass} />
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">分类</label>
          <div className="flex gap-2">
            <select
              value={isCustomCategory ? "__custom__" : (editing.category || "其他")}
              onChange={(e) => {
                if (e.target.value === "__custom__") {
                  setCustomCategory(editing.category || "");
                } else {
                  setEditing({ ...editing, category: e.target.value });
                  setCustomCategory("");
                }
              }}
              className={cn(inputClass, "flex-1")}
            >
              {categories.map((cat) => (
                <option key={cat} value={cat}>{cat}</option>
              ))}
              {!categories.includes(editing.category || "其他") && editing.category && (
                <option value={editing.category}>{editing.category}</option>
              )}
              <option value="__custom__">+ 新建分类</option>
            </select>
            {isCustomCategory && (
              <input type="text" value={customCategory}
                onChange={(e) => setCustomCategory(e.target.value)}
                placeholder="输入新分类名"
                className={cn(inputClass, "flex-1")}
                autoFocus />
            )}
          </div>
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">系统提示词</label>
          <textarea value={editing.system_prompt}
            onChange={(e) => setEditing({ ...editing, system_prompt: e.target.value })}
            placeholder="定义 AI 的角色和行为..." className={textareaClass} rows={3} />
        </div>

        <div className="space-y-1.5">
          <label className="text-sm font-medium text-fg-2">用户提示词模板</label>
          <textarea value={editing.user_prompt_template}
            onChange={(e) => setEditing({ ...editing, user_prompt_template: e.target.value })}
            placeholder="使用 {{text}} 引用转写文本" className={textareaClass} rows={3} />
          <p className="text-xs text-fg-3">使用 <code className="px-1 py-0.5 rounded bg-surface-inset font-mono">{"{{text}}"}</code> 引用转写文本；建议用 <code className="px-1 py-0.5 rounded bg-surface-inset font-mono">{"<input>...</input>"}</code> 包裹以明确边界、避免提示词注入</p>
        </div>

        {error && (
          <div className="px-3 py-2 rounded-lg bg-danger-muted text-danger-muted-fg text-sm">
            {error}
          </div>
        )}

        <div className="flex gap-2">
          <button onClick={handleSaveEdit} disabled={saving}
            className={cn("flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-all",
              "bg-primary text-primary-fg hover:opacity-90 active:scale-[0.98]",
              "disabled:opacity-50 disabled:cursor-not-allowed")}>
            {saving ? "保存中..." : "保存"}
          </button>
          <button onClick={handleCancel}
            className="px-4 py-2 rounded-lg text-sm font-medium border border-edge text-fg-2 hover:bg-surface-subtle active:scale-[0.98] transition-all">
            取消
          </button>
        </div>
      </div>
    );
  }

  // ── Prompt card ──
  const PromptCard = ({ prompt }: { prompt: PromptTemplate }) => (
    <div className="flex items-start gap-3 p-3 rounded-lg border border-edge hover:bg-surface-subtle hover:border-edge-strong transition-colors">
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-sm font-medium text-fg">{prompt.name}</span>
          {prompt.is_builtin && (
            <span className="px-1.5 py-0.5 text-[10px] rounded bg-primary/10 text-primary font-medium">
              内置
            </span>
          )}
        </div>
        <p className="text-xs text-fg-3 mt-0.5 truncate">
          {prompt.system_prompt || "无系统提示词"}
        </p>
      </div>
      <div className="flex gap-1 shrink-0">
        <button onClick={() => handleEdit(prompt)}
          className="p-1.5 rounded-md text-fg-3 hover:text-fg hover:bg-surface-inset active:scale-95 transition-all">
          <Pencil size={13} />
        </button>
        {!prompt.is_builtin && (
          <button onClick={() => handleDelete(prompt)} disabled={saving}
            className="p-1.5 rounded-md text-fg-3 hover:text-danger hover:bg-danger-muted active:scale-95 transition-all disabled:opacity-50">
            <Trash2 size={13} />
          </button>
        )}
      </div>
    </div>
  );

  // ── List view ──
  return (
    <div className="space-y-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-3">
          <h3 className="text-xs font-semibold text-fg-3 uppercase tracking-widest leading-none">提示词管理</h3>
          <button
            onClick={() => openUrl(PROMPT_GUIDE_URL).catch(() => {})}
            className="inline-flex items-center gap-1 text-[11px] leading-none text-fg-3 hover:text-primary transition-colors"
          >
            <BookOpen size={11} />
            编写教程 ↗
          </button>
        </div>
        <button onClick={handleNew}
          className="flex items-center gap-1 px-2.5 py-1 rounded-md text-xs font-medium text-primary bg-primary/10 hover:bg-primary/20 active:scale-95 transition-all">
          <Plus size={12} />
          新建
        </button>
      </div>

      {/* Category filter chips */}
      {categories.length > 1 && (
        <div className="flex flex-wrap gap-1.5">
          {[ALL_FILTER, ...categories].map((cat) => (
            <button key={cat} onClick={() => setActiveFilter(cat)}
              className={cn(
                "px-2.5 py-1 rounded-md text-xs font-medium transition-all active:scale-95",
                activeFilter === cat
                  ? "bg-primary text-primary-fg"
                  : "bg-surface-subtle text-fg-2 hover:bg-surface-inset"
              )}>
              {cat}
            </button>
          ))}
        </div>
      )}

      {error && (
        <div className="px-3 py-2 rounded-lg bg-danger-muted text-danger-muted-fg text-sm">
          {error}
        </div>
      )}

      {/* Fixed-height list area: prevents the Settings modal from resizing
          when switching between categories of different lengths. Overflow
          is contained internally instead of propagating to the outer scroll. */}
      <div className="h-[420px] overflow-y-auto pr-1 -mr-1">
        {filteredPrompts.length === 0 ? (
          <div className="h-full flex items-center justify-center text-fg-3 text-sm">
            还没有提示词，点击「新建」创建
          </div>
        ) : groupedPrompts ? (
          // "全部" view: grouped by category
          <div className="space-y-4">
            {Object.entries(groupedPrompts).map(([cat, items]) => (
              <div key={cat} className="space-y-2">
                <h4 className="text-xs font-medium text-fg-3 pl-1">{cat}</h4>
                {items.map((p) => <PromptCard key={p.id} prompt={p} />)}
              </div>
            ))}
          </div>
        ) : (
          // Filtered view: flat list
          <div className="space-y-2">
            {filteredPrompts.map((p) => <PromptCard key={p.id} prompt={p} />)}
          </div>
        )}
      </div>
    </div>
  );
}
