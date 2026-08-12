import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { useDialogNav } from "@/hooks/navigation/use-dialog-nav";
import { useClearCache } from "@/hooks/use-clear-cache";
import { useDialog, type ClearCacheTarget } from "@/hooks/use-dialog";
import { cn } from "@/lib/utils";
import { useCallback } from "react";

const RING = "ring-1 ring-foreground/25";
const NO_FOCUS_RING = "focus-visible:ring-0 focus-visible:border-transparent";

const COPY: Record<ClearCacheTarget, { title: string; description: string }> = {
  all: {
    title: "Clear all cache?",
    description:
      "This permanently deletes generated stems, charts, playback previews, and downloaded models. Your source media and library folders are not affected.",
  },
  models: {
    title: "Clear models cache?",
    description:
      "This permanently deletes the cached analysis models. Analysis stays unavailable until you explicitly download and set them up again in Settings > Models & runtime.",
  },
};

export const ClearCacheDialog = () => {
  const { close, mode } = useDialog();
  const clearCache = useClearCache();

  const open = typeof mode === "object" && mode !== null && mode.mode === "clear-cache";
  const target: ClearCacheTarget | null = open ? mode.target : null;

  const runClear = useCallback(() => {
    if (!target) return;
    clearCache[target]();
    close();
  }, [target, clearCache, close]);

  const onConfirm = useCallback(
    (index: number) => {
      if (index === 0) {
        close();
      } else {
        runClear();
      }
    },
    [close, runClear],
  );

  const { focusedIndex } = useDialogNav({
    open,
    itemCount: 2,
    onConfirm,
    onBack: close,
  });

  const copy = target ? COPY[target] : null;

  return (
    <AlertDialog open={open} onOpenChange={close}>
      <AlertDialogContent onEscapeKeyDown={(e) => e.preventDefault()}>
        <AlertDialogHeader>
          <AlertDialogTitle>{copy?.title ?? "Clear cache?"}</AlertDialogTitle>
          <AlertDialogDescription>{copy?.description ?? ""}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel
            onClick={close}
            className={cn(NO_FOCUS_RING, open && focusedIndex === 0 && RING)}
          >
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            onClick={runClear}
            className={cn(NO_FOCUS_RING, open && focusedIndex === 1 && RING)}
          >
            Clear cache
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
};
