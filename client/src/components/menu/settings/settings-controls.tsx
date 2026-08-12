import { Button } from "@/components/ui/button";
import { FieldDescription } from "@/components/ui/field";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useEffect, useState, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { NUMBER_PICKER_SIZE, type SettingsOption } from "./constants";
import { clampModelSettingNumber } from "./settings-logic";
import { Minus, Plus } from "lucide-react";

interface SettingsSelectProps {
  id?: string;
  label: string;
  placeholder: string;
  value: string;
  options: SettingsOption[];
  triggerClassName?: string;
  onValueChange: (value: string) => void;
}

export function SettingsSelect({
  id,
  label,
  placeholder,
  value,
  options,
  triggerClassName,
  onValueChange,
}: SettingsSelectProps) {
  return (
    <Select onValueChange={onValueChange} value={value}>
      <SelectTrigger id={id} className={cn("w-full", triggerClassName)}>
        <SelectValue placeholder={placeholder} />
      </SelectTrigger>
      <SelectContent position="popper" className="w-[var(--radix-select-trigger-width)]">
        <SelectGroup>
          <SelectLabel>{label}</SelectLabel>
          {options.map((option) => (
            <SelectItem key={option.value} value={option.value} description={option.description}>
              {option.label}
            </SelectItem>
          ))}
        </SelectGroup>
      </SelectContent>
    </Select>
  );
}

interface NumberButtonGroupProps {
  name: string;
  value: number;
  segment: number;
  getFocusClassName: (segment: number, slot?: number) => string;
  onChange: (value: number) => void;
}

export function NumberButtonGroup({
  name,
  value,
  segment,
  getFocusClassName,
  onChange,
}: NumberButtonGroupProps) {
  const [draft, setDraft] = useState(String(value));
  useEffect(() => setDraft(String(value)), [value]);

  const commit = () => {
    const next = clampModelSettingNumber(draft, value);
    setDraft(String(next));
    if (next !== value) onChange(next);
  };

  return (
    <div
      data-slot="setting-number-control"
      className="inline-grid h-8 w-36 grid-cols-[2rem_minmax(0,1fr)_2rem] items-center overflow-hidden rounded-full border border-border/55 bg-background/38 p-0.5 shadow-inner shadow-black/[0.04]"
    >
      <Button
        aria-label={`Decrease ${name.replace("_", " ")}`}
        variant="ghost"
        size="icon-sm"
        disabled={value <= 1}
        className={`size-7 rounded-full text-muted-foreground ${getFocusClassName(segment, 0)}`}
        onClick={() => onChange(Math.max(1, value - 1))}
      >
        <Minus />
      </Button>
      <input
        type="number"
        inputMode="numeric"
        min={1}
        max={16}
        step={1}
        value={draft}
        aria-label={name.replace(/_/g, " ")}
        className={`h-7 min-w-0 appearance-none border-0 bg-transparent px-1 text-center text-xs font-medium tabular-nums outline-none focus-visible:text-foreground [&::-webkit-inner-spin-button]:appearance-none [&::-webkit-outer-spin-button]:appearance-none ${getFocusClassName(segment, 1)}`}
        onChange={(event) => setDraft(event.target.value)}
        onBlur={commit}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            commit();
            event.currentTarget.blur();
          } else if (event.key === "Escape") {
            setDraft(String(value));
            event.currentTarget.blur();
          }
        }}
      />
      <Button
        aria-label={`Increase ${name.replace("_", " ")}`}
        variant="ghost"
        size="icon-sm"
        disabled={value >= 16}
        className={`size-7 rounded-full text-muted-foreground ${getFocusClassName(segment, NUMBER_PICKER_SIZE - 1)}`}
        onClick={() => onChange(Math.min(16, value + 1))}
      >
        <Plus />
      </Button>
    </div>
  );
}

export function PageHeader() {
  return (
    <div className="space-y-1 border-b border-border/55 pb-4">
      <p className="text-[9px] font-semibold uppercase tracking-[0.2em] text-primary">Uta Studio</p>
      <h1 className="text-xl font-semibold tracking-tight sm:text-2xl">Settings</h1>
      <p className="max-w-3xl text-[11px] leading-relaxed text-muted-foreground">
        Workspace, storage, and AI generation controls are grouped above so every setting stays
        predictable and easy to revisit.
      </p>
    </div>
  );
}

export function Hint({ children }: { children: ReactNode }) {
  return <FieldDescription>{children}</FieldDescription>;
}
