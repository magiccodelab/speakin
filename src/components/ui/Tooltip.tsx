import { useState, useRef, useCallback, useLayoutEffect, type ReactElement } from "react";
import { createPortal } from "react-dom";
import { cn } from "../../lib/utils";

interface TooltipProps {
  children: ReactElement;
  content: React.ReactNode;
  side?: "top" | "bottom" | "left" | "right";
  delayMs?: number;
}

const OFFSET = 6; // gap between trigger and tooltip

export function Tooltip({ children, content, side = "top", delayMs = 500 }: TooltipProps) {
  const [visible, setVisible] = useState(false);
  const [coords, setCoords] = useState({ x: 0, y: 0 });
  const triggerRef = useRef<HTMLDivElement>(null);
  const tooltipRef = useRef<HTMLDivElement>(null);
  const timerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const show = useCallback(() => {
    timerRef.current = setTimeout(() => setVisible(true), delayMs);
  }, [delayMs]);

  const hide = useCallback(() => {
    clearTimeout(timerRef.current);
    setVisible(false);
  }, []);

  useLayoutEffect(() => {
    if (!visible || !triggerRef.current || !tooltipRef.current) return;

    const trigger = triggerRef.current.getBoundingClientRect();
    const tip = tooltipRef.current.getBoundingClientRect();

    let x: number, y: number;

    switch (side) {
      case "bottom":
        x = trigger.left + trigger.width / 2 - tip.width / 2;
        y = trigger.bottom + OFFSET;
        break;
      case "left":
        x = trigger.left - tip.width - OFFSET;
        y = trigger.top + trigger.height / 2 - tip.height / 2;
        break;
      case "right":
        x = trigger.right + OFFSET;
        y = trigger.top + trigger.height / 2 - tip.height / 2;
        break;
      default: // top
        x = trigger.left + trigger.width / 2 - tip.width / 2;
        y = trigger.top - tip.height - OFFSET;
    }

    // Clamp to viewport
    x = Math.max(4, Math.min(x, window.innerWidth - tip.width - 4));
    y = Math.max(4, Math.min(y, window.innerHeight - tip.height - 4));

    setCoords({ x, y });
  }, [visible, side]);

  return (
    <>
      <div
        ref={triggerRef}
        onMouseEnter={show}
        onMouseLeave={hide}
        onFocus={show}
        onBlur={hide}
        className="inline-flex"
      >
        {children}
      </div>
      {visible &&
        createPortal(
          <div
            ref={tooltipRef}
            role="tooltip"
            className={cn(
              "fixed pointer-events-none z-[100]",
              "px-2.5 py-1.5 text-xs font-medium rounded-md",
              "bg-fg text-surface shadow-md",
              "animate-[tooltip-in_0.15s_ease-out]"
            )}
            style={{ left: coords.x, top: coords.y }}
          >
            {content}
          </div>,
          document.body
        )}
    </>
  );
}
