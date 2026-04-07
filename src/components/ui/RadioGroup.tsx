import { cn } from "../../lib/utils";

interface RadioIndicatorProps {
  checked: boolean;
}

export function RadioIndicator({ checked }: RadioIndicatorProps) {
  return (
    <div
      className={cn(
        "w-4 h-4 rounded-full border-2 flex items-center justify-center shrink-0",
        "transition-colors duration-[var(--t-fast)]",
        checked ? "border-primary" : "border-edge-strong group-hover:border-primary/50"
      )}
    >
      <div
        className={cn(
          "w-2 h-2 rounded-full bg-primary",
          "transition-transform duration-[var(--t-fast)]",
          checked ? "scale-100" : "scale-0"
        )}
      />
    </div>
  );
}
