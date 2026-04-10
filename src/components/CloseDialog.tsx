import { useState, useEffect, useRef } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "../lib/utils";

interface CloseDialogProps {
  open: boolean;
  onChoice: (behavior: "minimize" | "quit", remember: boolean) => void;
  onCancel: () => void;
}

export function CloseDialog({ open, onChoice, onCancel }: CloseDialogProps) {
  const [remember, setRemember] = useState(false);
  const minimizeBtnRef = useRef<HTMLButtonElement>(null);

  // Reset remember state and focus safe option on open
  useEffect(() => {
    if (!open) return;
    setRemember(false);
    const id = setTimeout(() => minimizeBtnRef.current?.focus(), 100);
    return () => clearTimeout(id);
  }, [open]);

  // ESC closes dialog without action
  useEffect(() => {
    if (!open) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onCancel();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [open, onCancel]);

  return (
    <AnimatePresence>
      {open && (
        <>
          <motion.div
            key="close-backdrop"
            className="absolute inset-0 z-[120] bg-black/20 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            onClick={onCancel}
          />
          <motion.div
            key="close-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="关闭应用"
            className="absolute inset-0 z-[120] flex items-center justify-center pointer-events-none"
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ type: "spring", damping: 30, stiffness: 350 }}
          >
            <div
              className="pointer-events-auto w-[320px] bg-surface-card border border-edge rounded-2xl shadow-2xl p-6"
              onClick={(e) => e.stopPropagation()}
            >
              <h3 className="text-base font-semibold text-fg mb-1">关闭应用</h3>
              <p className="text-sm text-fg-3 mb-5">选择关闭窗口后的行为</p>

              <div className="space-y-2 mb-5">
                <button
                  ref={minimizeBtnRef}
                  onClick={() => onChoice("minimize", remember)}
                  className={cn(
                    "w-full px-4 py-3 rounded-xl text-sm font-medium text-left",
                    "border border-edge hover:border-primary/50 hover:bg-surface-subtle",
                    "active:scale-[0.98] transition-all duration-[var(--t-fast)]",
                    "focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface-card outline-none",
                  )}
                >
                  <div className="text-fg">最小化到托盘</div>
                  <div className="text-xs text-fg-3 mt-0.5">窗口隐藏，在系统托盘中继续运行</div>
                </button>
                <button
                  onClick={() => onChoice("quit", remember)}
                  className={cn(
                    "w-full px-4 py-3 rounded-xl text-sm font-medium text-left",
                    "border border-edge hover:border-danger/50 hover:bg-danger-muted",
                    "active:scale-[0.98] transition-all duration-[var(--t-fast)]",
                    "focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface-card outline-none",
                  )}
                >
                  <div className="text-fg">退出应用</div>
                  <div className="text-xs text-fg-3 mt-0.5">完全关闭 SpeakIn声入</div>
                </button>
              </div>

              <label className="group flex items-center gap-2.5 cursor-pointer select-none py-1">
                <input
                  type="checkbox"
                  checked={remember}
                  onChange={(e) => setRemember(e.target.checked)}
                  className="peer fixed opacity-0 pointer-events-none"
                />
                <div className={cn(
                  "flex items-center justify-center w-4 h-4 rounded border-[1.5px] shrink-0",
                  "transition-all duration-[var(--t-fast)]",
                  remember
                    ? "bg-primary border-primary"
                    : "border-edge-strong group-hover:border-primary/50",
                )}>
                  {remember && (
                    <svg width="10" height="10" viewBox="0 0 10 10" fill="none" className="text-primary-fg">
                      <path d="M2 5.5L4 7.5L8 3" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
                    </svg>
                  )}
                </div>
                <span className="text-xs text-fg-3 group-hover:text-fg-2 transition-colors duration-[var(--t-fast)]">不再提示</span>
              </label>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
