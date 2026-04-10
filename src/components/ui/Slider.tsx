import * as SliderPrimitive from "@radix-ui/react-slider";
import { cn } from "../../lib/utils";

interface SliderProps {
  value: number;
  onValueChange: (value: number) => void;
  min: number;
  max: number;
  step?: number;
  disabled?: boolean;
  className?: string;
}

export function Slider({ value, onValueChange, min, max, step = 1, disabled, className }: SliderProps) {
  return (
    <SliderPrimitive.Root
      value={[value]}
      onValueChange={([v]) => onValueChange(v)}
      min={min}
      max={max}
      step={step}
      disabled={disabled}
      className={cn(
        "relative flex w-full touch-none select-none items-center",
        disabled && "opacity-50 cursor-not-allowed",
        className,
      )}
    >
      <SliderPrimitive.Track className="relative h-1.5 w-full grow overflow-hidden rounded-full bg-fg-3/20">
        <SliderPrimitive.Range className="absolute h-full bg-primary" />
      </SliderPrimitive.Track>
      <SliderPrimitive.Thumb
        className={cn(
          "block h-4 w-4 rounded-full border-2 border-primary bg-white shadow-sm",
          "transition-colors duration-[var(--t-fast)]",
          "hover:border-primary/80 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface",
          "active:scale-110",
          disabled && "pointer-events-none",
        )}
      />
    </SliderPrimitive.Root>
  );
}

interface SliderCardProps {
  value: number;
  onValueChange: (value: number) => void;
  min: number;
  max: number;
  step?: number;
  label: React.ReactNode;
  description?: string;
  valueLabel?: (value: number) => string;
  disabled?: boolean;
  className?: string;
}

export function SliderCard({
  value, onValueChange, min, max, step = 1,
  label, description, valueLabel, disabled, className,
}: SliderCardProps) {
  return (
    <div
      className={cn(
        "p-3 rounded-lg border border-edge",
        "transition-all duration-[var(--t-fast)]",
        disabled && "opacity-50 cursor-not-allowed",
        className,
      )}
    >
      <div className="flex items-center justify-between mb-2">
        <div className="flex-1 mr-3">
          <div className="text-sm font-medium text-fg">{label}</div>
          {description && <div className="text-xs text-fg-3 mt-0.5">{description}</div>}
        </div>
        <span className="text-sm font-mono font-medium text-primary tabular-nums min-w-[3ch] text-right">
          {valueLabel ? valueLabel(value) : value}
        </span>
      </div>
      <Slider
        value={value}
        onValueChange={onValueChange}
        min={min}
        max={max}
        step={step}
        disabled={disabled}
      />
    </div>
  );
}
