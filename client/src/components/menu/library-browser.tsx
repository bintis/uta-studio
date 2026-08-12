import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useLibrarySourceActions } from "@/hooks/use-library-source-actions";
import { useSongsMeta } from "@/queries/use-songs";
import { FolderOpen, FolderSearch, RefreshCw, Trash2 } from "lucide-react";

interface LibraryBrowserProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

export const LibraryBrowser = ({ open, onOpenChange }: LibraryBrowserProps) => {
  const { data: meta } = useSongsMeta();
  const { config, hasSource, selectFolder, rescan, disconnectSource, isPending, rescanDisabled } =
    useLibrarySourceActions();
  const path = config?.library_source?.kind === "folders" ? config.library_source.paths[0] : null;

  return (
    <Sheet open={open} onOpenChange={onOpenChange}>
      <SheetContent className="glass-panel w-[min(94vw,34rem)] sm:max-w-[34rem]" side="left">
        <SheetHeader className="border-b border-border/55">
          <div className="flex items-center gap-2">
            <FolderSearch className="size-4 text-primary" />
            <SheetTitle>Browse files</SheetTitle>
          </div>
          <SheetDescription>
            One place for the local folder Uta Studio watches, scans, and searches.
          </SheetDescription>
        </SheetHeader>
        <div className="space-y-5 p-5">
          <section className="rounded-md border border-border/55 bg-card/28 p-4">
            <p className="text-[9px] font-semibold uppercase tracking-[0.18em] text-primary">
              Storage
            </p>
            <div className="mt-3 flex items-start gap-3">
              <span className="rounded-md bg-primary/12 p-2.5 text-primary">
                <FolderOpen className="size-5" />
              </span>
              <div className="min-w-0 flex-1">
                <h2 className="text-sm font-semibold">
                  {hasSource ? "Local music folder" : "No folder connected"}
                </h2>
                <p className="mt-1 break-all text-[10px] leading-relaxed text-muted-foreground">
                  {path ??
                    "Choose the folder that contains your audio, video, LRC, and UltraStar files."}
                </p>
                {hasSource ? (
                  <p className="mt-2 text-[10px] text-muted-foreground">
                    {meta?.processed_count ?? 0} indexed · {meta?.analyzed_count ?? 0} analyzed
                  </p>
                ) : null}
              </div>
            </div>
          </section>

          <div className="grid gap-2 sm:grid-cols-2">
            <Button className="justify-start" disabled={isPending} onClick={selectFolder}>
              <FolderOpen /> {hasSource ? "Change folder…" : "Choose folder…"}
            </Button>
            <Button
              variant="outline"
              className="justify-start"
              disabled={rescanDisabled}
              onClick={rescan}
            >
              <RefreshCw /> Rescan now
            </Button>
          </div>

          {hasSource ? (
            <>
              <Separator />
              <div className="flex items-center justify-between gap-4">
                <div>
                  <p className="text-xs font-medium">Remove storage location</p>
                  <p className="mt-1 text-[10px] text-muted-foreground">
                    Your media files remain untouched.
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="sm"
                  className="text-destructive hover:text-destructive"
                  disabled={isPending}
                  onClick={() => {
                    if (
                      window.confirm(
                        "Remove this folder from Uta Studio? Your files will not be deleted.",
                      )
                    ) {
                      disconnectSource();
                    }
                  }}
                >
                  <Trash2 /> Remove
                </Button>
              </div>
            </>
          ) : null}
        </div>
      </SheetContent>
    </Sheet>
  );
};
