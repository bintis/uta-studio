import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Separator } from "@/components/ui/separator";
import { Table, TableBody, TableCell, TableRow } from "@/components/ui/table";
import { useDialog } from "@/hooks/use-dialog";
import { useDialogNav } from "@/hooks/navigation/use-dialog-nav";
import { useRef } from "react";
import { cn } from "@/lib/utils";
import { version } from "../../../../package.json";
import logoSrc from "../../../../../client/src-tauri/icons/icon.png";

const attributions = [
  { name: "Lyrics data", value: "LRCLIB (lrclib.net)" },
  {
    name: "Stem separation",
    value: "UVR — MIT / Demucs by Meta Research — MIT",
  },
  { name: "Speech recognition", value: "WhisperX / OpenAI Whisper, NVIDIA Parakeet" },
  { name: "Forced alignment", value: "WhisperX, torchaudio, Qwen3-ForcedAligner" },
  { name: "CJK romanization", value: "fugashi, pypinyin, hangul-romanize, ToJyutping" },
];

export const InfoDialog = () => {
  const { mode, close } = useDialog();

  const containerRef = useRef<HTMLDivElement>(null);

  const open = mode === "about";

  const { focusedIndex } = useDialogNav({
    open,
    itemCount: 1,
    onBack: close,
    containerRef,
  });

  return (
    <Dialog open={open} onOpenChange={close}>
      <DialogContent className="glass-panel border-white/15 sm:max-w-lg">
        <div ref={containerRef} className="contents">
          <DialogHeader>
            <div className="flex items-center gap-4">
              <img
                src={logoSrc}
                alt=""
                className="h-16 w-24 rounded-2xl object-contain shadow-lg shadow-primary/20"
              />
              <div>
                <DialogTitle className="text-2xl">Uta Studio</DialogTitle>
                <p className="mt-1 text-sm text-muted-foreground">
                  AI generation · precise chart editing · interoperable export
                </p>
              </div>
            </div>
            <div className="text-sm text-muted-foreground">
              <p>Version {version}</p>
              <p>License: GPL-3.0-or-later</p>
            </div>
          </DialogHeader>
          <Separator />
          <div className="space-y-2">
            <h4 className="text-xs font-medium uppercase tracking-wide text-muted-foreground">
              Attributions
            </h4>
            <Table>
              <TableBody>
                {attributions.map((attr) => (
                  <TableRow key={attr.name}>
                    <TableCell className="text-muted-foreground">{attr.name}</TableCell>
                    <TableCell className="font-medium">{attr.value}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              onClick={close}
              className={cn(
                "focus-visible:ring-0 focus-visible:border-transparent",
                focusedIndex === 0 && "ring-2 ring-primary",
              )}
            >
              Close
            </Button>
          </DialogFooter>
        </div>
      </DialogContent>
    </Dialog>
  );
};
