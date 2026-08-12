import { ActionsSection } from "@/components/menu/song-list/details/actions-section";
import type { SongStatusInfo } from "@/components/menu/song-list/shared/song-status";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import type { Song } from "@/types/Song";
import { Button } from "@/components/ui/button";
import { ChevronDown, MoreHorizontal, PencilRuler, Settings } from "lucide-react";
import { useState } from "react";
import { useSongEditorAction } from "@/hooks/use-song-editor-action";

interface SongActionsMenuProps {
  song: Song;
  status: SongStatusInfo;
  analysisBusy: boolean;
  supportsAnalysisActions: boolean;
  compact?: boolean;
}

export const SongActionsMenu = ({
  song,
  status,
  analysisBusy,
  supportsAnalysisActions,
  compact = false,
}: SongActionsMenuProps) => {
  const [open, setOpen] = useState(false);
  const editorAction = useSongEditorAction(song, status, analysisBusy);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          className={
            compact
              ? "inline-flex size-8 items-center justify-center rounded-full text-muted-foreground transition hover:bg-muted hover:text-foreground"
              : "inline-flex h-9 items-center gap-2 rounded-full border border-border/55 bg-background/32 px-4 text-xs font-medium backdrop-blur-xl transition hover:border-foreground/16 hover:bg-foreground/[0.045] data-[state=open]:bg-foreground/[0.055]"
          }
          aria-label={compact ? `Actions for ${song.title}` : undefined}
        >
          {compact ? (
            <MoreHorizontal className="size-4" />
          ) : (
            <>
              Actions <ChevronDown className="size-3.5" />
            </>
          )}
        </button>
      </PopoverTrigger>
      <PopoverContent
        align={compact ? "end" : "start"}
        sideOffset={8}
        onClick={(event) => {
          const target = event.target as HTMLElement;
          if (target.closest("button:not(:disabled)")) window.setTimeout(() => setOpen(false), 0);
        }}
        className="song-actions-popover max-h-[min(72vh,34rem)] w-[min(88vw,18rem)] gap-0 overflow-y-auto overscroll-contain rounded-md p-0 shadow-2xl ring-0"
      >
        <div className="px-3 py-2.5">
          <p className="truncate text-xs font-medium">{song.title}</p>
          <p className="mt-0.5 truncate text-[9px] text-muted-foreground">
            {song.artist || "Unknown artist"} · Production actions
          </p>
        </div>
        <Separator className="bg-border/55" />
        <div className="px-2 py-2">
          <Button
            type="button"
            variant="ghost"
            size="lg"
            disabled={editorAction.disabled}
            className="h-9 w-full justify-start gap-2.5 rounded-sm px-2.5 text-left hover:bg-foreground/[0.055] hover:text-foreground"
            onClick={editorAction.trigger}
          >
            <PencilRuler className="size-4" />
            <span className="text-xs font-medium">{editorAction.label}</span>
          </Button>
          {editorAction.needsSetup ? (
            <Button
              type="button"
              variant="ghost"
              size="sm"
              className="mt-1 w-full justify-start text-muted-foreground hover:bg-foreground/[0.045] hover:text-foreground"
              onClick={editorAction.openAnalysisSettings}
            >
              <Settings /> Open analysis settings
            </Button>
          ) : null}
        </div>
        <Separator className="bg-border/55" />
        <ActionsSection
          song={song}
          status={status}
          analysisBusy={analysisBusy}
          supportsAnalysisActions={supportsAnalysisActions}
          menuMode
        />
      </PopoverContent>
    </Popover>
  );
};
