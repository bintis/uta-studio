import { Button } from "@/components/ui/button";
import { DialogFooter } from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { ARIA_DISABLED_CLASS, ringFor } from "./parts";

interface EditLyricsFooterProps {
  onCancel: () => void;
  onSave: () => void;
  saving: boolean;
  canSave: boolean;
  saveLabel: string;
  hint?: string;
  isFocused: (slot: number) => boolean;
}

export const EditLyricsFooter = ({
  onCancel,
  onSave,
  saving,
  canSave,
  saveLabel,
  hint,
  isFocused,
}: EditLyricsFooterProps) => {
  const cancelFocused = isFocused(0);
  const saveFocused = isFocused(1);
  return (
    <DialogFooter className="sm:items-center">
      {hint ? (
        <p className="text-[11px] text-muted-foreground sm:mr-auto sm:text-left">{hint}</p>
      ) : null}
      <Button
        variant="outline"
        onClick={() => {
          if (saving) return;
          onCancel();
        }}
        aria-disabled={saving}
        className={cn(ARIA_DISABLED_CLASS, ringFor(cancelFocused))}
      >
        Cancel
      </Button>
      <Button
        onClick={() => {
          if (!canSave) return;
          onSave();
        }}
        aria-disabled={!canSave}
        className={cn(ARIA_DISABLED_CLASS, ringFor(saveFocused))}
      >
        {saving ? "Saving…" : saveLabel}
      </Button>
    </DialogFooter>
  );
};
