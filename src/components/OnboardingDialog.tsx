import { useState, useEffect } from "react";
import { AnimatePresence, motion } from "motion/react";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ShieldCheck,
  Lock,
  KeyRound,
  Mic,
  Languages,
  Wand2,
  ListChecks,
  ArrowLeft,
  ArrowRight,
} from "lucide-react";
import { cn } from "../lib/utils";

interface OnboardingDialogProps {
  open: boolean;
  onClose: () => void;
  onConfigure: (provider: ProviderKey) => void;
}

const BLOG_URL = "https://afengblog.com/blog/speakin-settings-guide.html";

export type ProviderKey = "doubao" | "dashscope" | "qwen";

interface ProviderOption {
  key: ProviderKey;
  name: string;
  desc: string;
  badge?: string;
}

const PROVIDERS: ProviderOption[] = [
  { key: "doubao", name: "豆包", desc: "火山引擎 · 免费额度充足", badge: "推荐" },
  { key: "dashscope", name: "百炼", desc: "DashScope · 阿里云出品" },
  { key: "qwen", name: "千问", desc: "Qwen3 ASR · 实时流式" },
];

const PRIVACY_ITEMS = [
  {
    icon: ShieldCheck,
    label: "本地处理",
    text: "录音在你的电脑上预处理后，再发送至你选定的服务商",
  },
  {
    icon: Lock,
    label: "零数据留存",
    text: "我们不收集、不存储、不上传任何用户数据",
  },
  {
    icon: KeyRound,
    label: "凭据安全",
    text: "API 密钥由操作系统的原生密钥管理器加密保管",
  },
];

const AI_SCENARIOS = [
  {
    icon: Languages,
    label: "跨语言输出",
    text: "中文说话，英文输出，无缝对接海外 AI 工具",
  },
  {
    icon: Wand2,
    label: "口语转书面",
    text: "自动纠错与润色，把口头语整理成完整表达",
  },
  {
    icon: ListChecks,
    label: "结构化整理",
    text: "把口述内容整理成条目、Markdown 或代码片段",
  },
];

const TOTAL_STEPS = 3;

export function OnboardingDialog({ open, onClose, onConfigure }: OnboardingDialogProps) {
  const [step, setStep] = useState(0);
  const [direction, setDirection] = useState(1);
  const [provider, setProvider] = useState<ProviderKey>("doubao");

  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [open, onClose]);

  // Reset state when reopened
  useEffect(() => {
    if (open) {
      setStep(0);
      setDirection(1);
      setProvider("doubao");
    }
  }, [open]);

  const goNext = () => {
    if (step < TOTAL_STEPS - 1) {
      setDirection(1);
      setStep((s) => s + 1);
    } else {
      onConfigure(provider);
    }
  };

  const goBack = () => {
    if (step > 0) {
      setDirection(-1);
      setStep((s) => s - 1);
    }
  };

  const slideVariants = {
    enter: (dir: number) => ({ x: dir > 0 ? 32 : -32, opacity: 0 }),
    center: { x: 0, opacity: 1 },
    exit: (dir: number) => ({ x: dir > 0 ? -32 : 32, opacity: 0 }),
  };

  const ctaLabel = step === TOTAL_STEPS - 1 ? "开始使用 SpeakIn声入" : "下一步";

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            key="onboarding-backdrop"
            className="absolute inset-0 z-[120] bg-black/20 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
          />
          <motion.div
            key="onboarding-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="欢迎使用 SpeakIn声入"
            className="absolute inset-0 z-[120] flex items-center justify-center pointer-events-none"
            initial={{ opacity: 0, scale: 0.96 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.96 }}
            transition={{ type: "spring", damping: 30, stiffness: 350 }}
          >
            <div className="pointer-events-auto w-[420px] flex flex-col rounded-2xl bg-surface-card border border-edge shadow-2xl overflow-hidden">
              {/* Header — logo + progress bar */}
              <div className="px-6 pt-5 pb-4 flex items-center justify-between gap-4">
                <div className="flex items-center gap-2">
                  <div className="w-7 h-7 rounded-lg bg-primary flex items-center justify-center shadow-sm shadow-primary/20">
                    <Mic size={14} className="text-primary-fg" />
                  </div>
                  <span className="text-[15px] font-semibold tracking-tight text-fg">
                    SpeakIn声入
                  </span>
                </div>
                <div className="flex items-center gap-1.5">
                  {Array.from({ length: TOTAL_STEPS }).map((_, i) => (
                    <motion.div
                      key={i}
                      className={cn(
                        "h-1 rounded-full",
                        i === step
                          ? "bg-primary"
                          : i < step
                            ? "bg-primary/40"
                            : "bg-fg/15",
                      )}
                      animate={{ width: i === step ? 24 : 14 }}
                      transition={{ duration: 0.35, ease: [0.16, 1, 0.3, 1] }}
                    />
                  ))}
                </div>
              </div>

              {/* Content — fixed-height stage so steps don't reflow.
                  Padding lives on the abs child via inset-x-6 (parent padding
                  doesn't affect absolutely positioned children). */}
              <div className="relative h-[330px] overflow-hidden">
                <AnimatePresence mode="wait" custom={direction}>
                  <motion.div
                    key={step}
                    custom={direction}
                    variants={slideVariants}
                    initial="enter"
                    animate="center"
                    exit="exit"
                    transition={{ duration: 0.28, ease: [0.16, 1, 0.3, 1] }}
                    className="absolute inset-x-6 top-0"
                  >
                    {step === 0 && <PrivacyStep />}
                    {step === 1 && (
                      <ProviderStep selected={provider} onSelect={setProvider} />
                    )}
                    {step === 2 && <AIStep />}
                  </motion.div>
                </AnimatePresence>
              </div>

              {/* Footer */}
              <div className="px-6 pt-3 pb-5 flex items-center justify-between gap-3">
                {step > 0 ? (
                  <button
                    onClick={goBack}
                    className="inline-flex items-center gap-1 px-3 py-2 rounded-lg text-sm font-medium text-fg-2 hover:text-fg hover:bg-surface-subtle transition-all duration-[var(--t-fast)] active:scale-[0.98]"
                  >
                    <ArrowLeft size={14} />
                    返回
                  </button>
                ) : (
                  <button
                    onClick={onClose}
                    className="px-3 py-2 rounded-lg text-sm font-medium text-fg-3 hover:text-fg-2 transition-all duration-[var(--t-fast)]"
                  >
                    跳过
                  </button>
                )}
                <div className="flex items-center gap-1">
                  {step === TOTAL_STEPS - 1 && (
                    <button
                      onClick={onClose}
                      className="px-3 py-2 rounded-lg text-sm font-medium text-fg-3 hover:text-fg-2 transition-all duration-[var(--t-fast)]"
                    >
                      稍后
                    </button>
                  )}
                  <button
                    onClick={goNext}
                    className="inline-flex items-center gap-1 px-5 py-2 rounded-lg text-sm font-semibold bg-primary text-primary-fg shadow-sm hover:opacity-90 active:scale-[0.98] transition-all duration-[var(--t-fast)]"
                  >
                    {ctaLabel}
                    {step < TOTAL_STEPS - 1 && <ArrowRight size={14} />}
                  </button>
                </div>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}

// ─── Step header ──────────────────────────────────────────────
function StepHeader({ tag, title, desc }: { tag: string; title: string; desc: string }) {
  return (
    <div className="mb-4">
      <span className="inline-block text-[10.5px] font-semibold text-primary bg-primary/10 px-2 py-0.5 rounded-full tracking-[0.08em]">
        {tag}
      </span>
      <h2 className="mt-2.5 text-[19px] font-semibold tracking-tight text-fg leading-tight">
        {title}
      </h2>
      <p className="mt-1.5 text-[12.5px] text-fg-2 leading-relaxed">{desc}</p>
    </div>
  );
}

// ─── Step 1: Privacy ──────────────────────────────────────────
function PrivacyStep() {
  return (
    <div>
      <StepHeader
        tag="隐私承诺"
        title="你的数据，只属于你"
        desc="隐私不是功能，是底线"
      />
      <div className="space-y-2">
        {PRIVACY_ITEMS.map((item) => {
          const Icon = item.icon;
          return (
            <div
              key={item.label}
              className="flex items-start gap-3 px-3.5 py-2.5 rounded-xl bg-surface-subtle/60 border border-edge/60"
            >
              <div className="w-8 h-8 rounded-lg bg-primary/10 flex items-center justify-center shrink-0 mt-px">
                <Icon size={15} className="text-primary" />
              </div>
              <div className="min-w-0">
                <div className="text-[13px] font-semibold text-fg leading-tight">
                  {item.label}
                </div>
                <div className="mt-0.5 text-[11.5px] text-fg-3 leading-relaxed">
                  {item.text}
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ─── Step 2: Provider selection ───────────────────────────────
function ProviderStep({
  selected,
  onSelect,
}: {
  selected: ProviderKey;
  onSelect: (key: ProviderKey) => void;
}) {
  return (
    <div>
      <StepHeader
        tag="选择服务商"
        title="连接语音识别服务"
        desc="数据只会发往你选定的服务商，可在设置中随时切换"
      />
      <div className="space-y-2">
        {PROVIDERS.map((p) => {
          const active = selected === p.key;
          return (
            <button
              key={p.key}
              type="button"
              onClick={() => onSelect(p.key)}
              className={cn(
                "w-full text-left px-3.5 py-2.5 rounded-xl border transition-all duration-[var(--t-fast)]",
                active
                  ? "bg-primary/10 border-primary"
                  : "bg-surface-subtle/40 border-edge/60 hover:border-edge hover:bg-surface-subtle/80",
              )}
            >
              <div className="flex items-center justify-between">
                <div className="flex items-center gap-2">
                  <span className="text-[13.5px] font-semibold text-fg">{p.name}</span>
                  {p.badge && (
                    <span className="text-[10px] font-semibold text-primary-fg bg-primary px-1.5 py-0.5 rounded tracking-wider leading-none">
                      {p.badge}
                    </span>
                  )}
                </div>
                <div
                  className={cn(
                    "w-4 h-4 rounded-full border-[1.5px] flex items-center justify-center transition-colors duration-[var(--t-fast)]",
                    active ? "border-primary" : "border-fg/25",
                  )}
                >
                  {active && <div className="w-1.5 h-1.5 rounded-full bg-primary" />}
                </div>
              </div>
              <div className="mt-1 text-[11.5px] text-fg-3">{p.desc}</div>
            </button>
          );
        })}
      </div>
      <button
        type="button"
        onClick={() => openUrl(BLOG_URL).catch(() => {})}
        className="mt-3 text-[12px] text-primary hover:underline"
      >
        查看详细配置指南 ↗
      </button>
    </div>
  );
}

// ─── Step 3: AI optimize (informational only) ─────────────────
function AIStep() {
  return (
    <div>
      <StepHeader
        tag="可选增强"
        title="AI 文字优化"
        desc="语音原文之外，AI 还能再加工一道"
      />
      <div className="space-y-2">
        {AI_SCENARIOS.map((item) => {
          const Icon = item.icon;
          return (
            <div
              key={item.label}
              className="flex items-start gap-3 px-3.5 py-2.5 rounded-xl bg-surface-subtle/60 border border-edge/60"
            >
              <div className="w-8 h-8 rounded-lg bg-primary/10 flex items-center justify-center shrink-0 mt-px">
                <Icon size={15} className="text-primary" />
              </div>
              <div className="min-w-0">
                <div className="text-[13px] font-semibold text-fg leading-tight">
                  {item.label}
                </div>
                <div className="mt-0.5 text-[11.5px] text-fg-3 leading-relaxed">
                  {item.text}
                </div>
              </div>
            </div>
          );
        })}
      </div>
      <p className="mt-3 text-[11px] text-fg-3/80 leading-relaxed">
        兼容任何 OpenAI 格式的 AI 服务，需先在设置中配置供应商再开启
      </p>
    </div>
  );
}
