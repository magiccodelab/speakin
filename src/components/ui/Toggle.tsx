import { cn } from "../../lib/utils";

interface ToggleProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  disabled?: boolean;
  className?: string;
}

export function Toggle({ checked, onChange, disabled, className }: ToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "relative shrink-0 w-9 h-5 rounded-full transition-colors duration-[var(--t-fast)]",
        "focus-visible:ring-2 focus-visible:ring-primary focus-visible:ring-offset-2 focus-visible:ring-offset-surface outline-none",
        checked ? "bg-primary hover:bg-primary/85" : "bg-fg-3/30 hover:bg-fg-3/40",
        disabled && "opacity-50 cursor-not-allowed",
        className,
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 left-0.5 w-4 h-4 rounded-full bg-white shadow-sm",
          "transition-transform duration-[var(--t-fast)]",
          checked && "translate-x-4",
        )}
      />
    </button>
  );
}

interface ToggleCardProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: React.ReactNode;
  description?: string;
  disabled?: boolean;
  className?: string;
}

export function ToggleCard({ checked, onChange, label, description, disabled, className }: ToggleCardProps) {
  return (
    <label
      className={cn(
        "flex items-center justify-between p-3 rounded-lg border border-edge cursor-pointer",
        "hover:bg-surface-subtle active:scale-[0.98] transition-all duration-[var(--t-fast)]",
        "focus-within:ring-2 focus-within:ring-primary focus-within:ring-offset-2 focus-within:ring-offset-surface",
        disabled && "opacity-50 cursor-not-allowed",
        className,
      )}
    >
      <div className="flex-1 mr-3">
        <div className="text-sm font-medium text-fg">{label}</div>
        {description && <div className="text-xs text-fg-3">{description}</div>}
      </div>
      <Toggle checked={checked} onChange={onChange} disabled={disabled} />
    </label>
  );
}
