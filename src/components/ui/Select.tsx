import {
  Select as ShadSelect,
  SelectContent,
  SelectGroup as ShadSelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "./select-primitives";

export interface SelectOption {
  label: string;
  value: string;
}

export interface SelectGroup {
  label: string;
  options: SelectOption[];
}

interface SelectProps {
  value: string;
  options?: SelectOption[];
  groups?: SelectGroup[];
  onChange: (value: string) => void;
  placeholder?: string;
  className?: string;
}

// Radix Select 不允许 value="" 作为 SelectItem 的值，需要做哨兵替换。
const EMPTY_SENTINEL = "__empty__";
const encode = (v: string) => (v === "" ? EMPTY_SENTINEL : v);
const decode = (v: string) => (v === EMPTY_SENTINEL ? "" : v);

export function Select({
  value,
  options,
  groups,
  onChange,
  placeholder,
  className,
}: SelectProps) {
  return (
    <ShadSelect value={encode(value)} onValueChange={(v) => onChange(decode(v))}>
      <SelectTrigger className={className}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent>
        {groups
          ? groups.map((g, gi) => (
              <ShadSelectGroup key={`${g.label}-${gi}`}>
                <SelectLabel>{g.label}</SelectLabel>
                {g.options.map((o) => (
                  <SelectItem key={o.value} value={encode(o.value)}>
                    {o.label}
                  </SelectItem>
                ))}
              </ShadSelectGroup>
            ))
          : (options ?? []).map((o) => (
              <SelectItem key={o.value} value={encode(o.value)}>
                {o.label}
              </SelectItem>
            ))}
      </SelectContent>
    </ShadSelect>
  );
}
