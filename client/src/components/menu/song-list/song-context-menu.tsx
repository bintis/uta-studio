import { ActionsSection } from "@/components/menu/song-list/details/actions-section";
import type { SongStatusInfo } from "@/components/menu/song-list/shared/song-status";
import { ContextMenu, ContextMenuContent, ContextMenuTrigger } from "@/components/ui/context-menu";
import { Separator } from "@/components/ui/separator";
import type { Song } from "@/types/Song";
import { ExternalLink, PencilRuler, Settings } from "lucide-react";
import type { ReactElement } from "react";
import { useNavigate } from "react-router";
import { useSongEditorAction } from "@/hooks/use-song-editor-action";

interface SongContextMenuProps {
  children: ReactElement;
  song: Song;
  status: SongStatusInfo;
  analysisBusy: boolean;
  supportsAnalysisActions: boolean;
}

export const SongContextMenu = ({
  children,
  song,
  status,
  analysisBusy,
  supportsAnalysisActions,
}: SongContextMenuProps) => {
  const navigate = useNavigate();
  const editorAction = useSongEditorAction(song, status, analysisBusy);
  const itemClass =
    "flex min-h-9 w-full items-center gap-2.5 rounded-sm px-2.5 py-1.5 text-left text-xs font-medium text-foreground outline-none transition hover:bg-foreground/[0.055] disabled:pointer-events-none disabled:opacity-38";

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>{children}</ContextMenuTrigger>
      <ContextMenuContent
        className="song-actions-popover max-h-[min(72vh,34rem)] w-[min(88vw,18rem)] overflow-y-auto p-0 ring-0"
        onClick={(event) => {
          if ((event.target as HTMLElement).closest("button:not(:disabled)")) {
            event.currentTarget.dispatchEvent(
              new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
            );
          }
        }}
      >
        <div className="px-3 py-2.5">
          <p className="truncate text-xs font-medium">{song.title}</p>
          <p className="mt-0.5 truncate text-[9px] text-muted-foreground">
            {song.artist || "Unknown artist"} · Track actions
          </p>
        </div>
        <Separator className="bg-border/55" />
        <div className="space-y-0.5 px-2 py-2">
          <button
            type="button"
            className={itemClass}
            disabled={editorAction.disabled}
            onClick={editorAction.trigger}
          >
            <PencilRuler className="size-4" />
            <span>{editorAction.label}</span>
          </button>
          {editorAction.needsSetup ? (
            <button type="button" className={itemClass} onClick={editorAction.openAnalysisSettings}>
              <Settings className="size-4" />
              <span>Set up analysis</span>
            </button>
          ) : null}
          <button
            type="button"
            className={itemClass}
            onClick={() => navigate("/song", { state: { song } })}
          >
            <ExternalLink className="size-4" />
            <span>Open track page</span>
          </button>
        </div>
        <Separator className="bg-border/55" />
        <ActionsSection
          song={song}
          status={status}
          analysisBusy={analysisBusy}
          supportsAnalysisActions={supportsAnalysisActions}
          menuMode
        />
      </ContextMenuContent>
    </ContextMenu>
  );
};
