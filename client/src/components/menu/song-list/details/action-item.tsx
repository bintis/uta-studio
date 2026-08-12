import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { LucideIcon } from "lucide-react";

export interface ActionItemProps {
  icon: LucideIcon;
  title: string;
  description: string;
  onClick: () => void | Promise<void>;
  disabled?: boolean;
  destructive?: boolean;
  menuMode?: boolean;
}

export const ActionItem = ({
  icon: Icon,
  title,
  description,
  onClick,
  disabled,
  destructive,
  menuMode,
}: ActionItemProps) => (
  <Button
    type="button"
    variant={destructive ? "destructive" : "ghost"}
    size="lg"
    className={cn(
      "h-auto w-full justify-start gap-2.5 px-2.5 text-left whitespace-normal",
      menuMode ? "min-h-9 items-center rounded-sm py-1.5" : "min-h-12 items-start rounded-md py-2",
      menuMode
        ? "text-foreground hover:bg-foreground/[0.055] hover:text-foreground hover:ring-0"
        : "hover:z-10 hover:bg-foreground/[0.04]",
      !menuMode &&
        (destructive
          ? "hover:bg-destructive/10 dark:hover:bg-destructive/20"
          : "hover:bg-primary/[0.07] dark:hover:bg-primary/[0.09]"),
    )}
    disabled={disabled}
    onClick={onClick}
  >
    <Icon className={cn("size-4", !menuMode && "mt-0.5")} />
    <span className="min-w-0">
      <span className="block text-xs font-medium leading-tight">{title}</span>
      {!menuMode ? (
        <span
          className={
            destructive
              ? "mt-0.5 block text-[0.625rem] leading-tight text-destructive/70"
              : "mt-0.5 block text-[0.625rem] leading-tight text-muted-foreground"
          }
        >
          {description}
        </span>
      ) : null}
    </span>
  </Button>
);
