import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "motion/react";
import { cn } from "../lib/utils";
import { openUrl } from "@tauri-apps/plugin-opener";
import { getVersion } from "@tauri-apps/api/app";

interface AboutDialogProps {
  open: boolean;
  onClose: () => void;
}

export function AboutDialog({ open: isOpen, onClose }: AboutDialogProps) {
  const [version, setVersion] = useState("1.0.0");

  useEffect(() => {
    getVersion().then(setVersion).catch(() => {});
  }, []);

  useEffect(() => {
    if (!isOpen) return;
    const handler = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        onClose();
      }
    };
    window.addEventListener("keydown", handler, true);
    return () => window.removeEventListener("keydown", handler, true);
  }, [isOpen, onClose]);

  const handleOpenUrl = async (url: string) => {
    try {
      await openUrl(url);
    } catch (e) {
      console.warn("Failed to open URL:", url, e);
    }
  };

  return (
    <AnimatePresence>
      {isOpen && (
        <>
          <motion.div
            key="about-backdrop"
            className="absolute inset-0 z-[120] bg-black/20 backdrop-blur-sm"
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.15 }}
            onClick={onClose}
          />
          <motion.div
            key="about-dialog"
            role="dialog"
            aria-modal="true"
            aria-label="关于 SpeakIn声入"
            className="absolute inset-0 z-[120] flex items-center justify-center pointer-events-none"
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.95 }}
            transition={{ type: "spring", damping: 30, stiffness: 350 }}
          >
            <div
              className="pointer-events-auto w-[380px] bg-surface-card border border-edge rounded-2xl shadow-2xl p-6"
              onClick={(e) => e.stopPropagation()}
            >
              {/* Header */}
              <div className="text-center mb-4">
                <h3 className="text-lg font-bold text-fg">SpeakIn声入</h3>
                <div className="flex items-center justify-center gap-2 mt-0.5">
                  <p className="text-sm text-fg-3">v{version}</p>
                  <span className="text-fg-3/40">·</span>
                  <button
                    type="button"
                    onClick={() => handleOpenUrl("https://github.com/magiccodelab/speakin/releases")}
                    className="text-xs text-primary hover:underline active:scale-95 transition-all"
                  >
                    检查更新
                  </button>
                </div>
                <p className="text-xs text-fg-3 mt-1">多平台 ASR 语音输入工具</p>
              </div>

              {/* Info */}
              <div className="space-y-1.5 text-xs text-fg-3 mb-4">
                <div className="flex justify-between">
                  <span>许可证</span>
                  <span className="text-fg-2 font-medium">GPL-3.0</span>
                </div>
                <div className="flex justify-between">
                  <span>作者</span>
                  <span className="text-fg-2 font-medium">AFengBlog.com</span>
                </div>
                <div className="flex justify-between">
                  <span>项目地址</span>
                  <a
                    href="#"
                    onClick={(e) => { e.preventDefault(); handleOpenUrl("https://github.com/magiccodelab/speakIn"); }}
                    className="text-primary hover:underline font-medium"
                  >
                    GitHub
                  </a>
                </div>
              </div>

              {/* Disclaimer */}
              <div className="px-3 py-2.5 rounded-lg bg-surface-inset border border-edge text-[11px] leading-relaxed text-fg-3 mb-4">
                <p className="font-semibold text-fg-2 mb-1">免责声明</p>
                <p>
                  本软件仅提供语音识别转写及文本处理的技术工具功能。用户通过本软件生成、处理、传输或发布的一切内容，
                  均由用户自行承担全部法律责任。软件作者及开源贡献者不对用户生成的任何内容承担审核、监管或连带责任。
                </p>
                <p className="mt-1.5">
                  用户应自觉遵守中华人民共和国相关法律法规，包括但不限于《网络安全法》《数据安全法》《个人信息保护法》
                  及《互联网信息服务管理办法》等，不得利用本软件制作、复制、发布、传播任何违反法律法规的内容。
                  因用户不当使用本软件而产生的一切法律纠纷及后果，均与本软件作者及开源项目无关。
                </p>
                <p className="mt-1.5">
                  本软件按"现状"提供，不作任何明示或暗示的保证，包括但不限于对适销性、特定用途适用性及不侵权的保证。
                </p>
              </div>

              {/* Buttons */}
              <div className="flex gap-2">
                <button
                  onClick={() => handleOpenUrl("https://afengblog.com/speakIn")}
                  className={cn(
                    "flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-all",
                    "border border-edge text-fg-2 hover:bg-surface-subtle active:scale-[0.98]"
                  )}
                >
                  前往作者主页
                </button>
                <button
                  onClick={onClose}
                  className={cn(
                    "flex-1 px-4 py-2 rounded-lg text-sm font-medium transition-all",
                    "bg-primary text-primary-fg hover:opacity-90 active:scale-[0.98]"
                  )}
                >
                  关闭
                </button>
              </div>
            </div>
          </motion.div>
        </>
      )}
    </AnimatePresence>
  );
}
