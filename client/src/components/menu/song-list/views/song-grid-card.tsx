import { cn } from "@/lib/utils";
import { memo } from "react";
import { formatSeconds } from "@/utils/format-duration";
import { AlbumArt } from "../shared/album-art";
import { LanguageBadge } from "../shared/language-badge";
import { StatusBadge } from "../shared/status-badge";
import { getSongStatusInfo } from "../shared/song-status";
import { SongActionsMenu } from "../song-actions-menu";
import { SongContextMenu } from "../song-context-menu";
import type { SongItemProps } from "../types";

export const SongGridCard = memo(
  ({ song, queueStatus, index, isFocused, isSelected, onSelect }: SongItemProps) => {
    const status = getSongStatusInfo(song, queueStatus);
    const analysisBusy = queueStatus === "Queued" || Boolean(status.isAnalyzing);
    return (
      <SongContextMenu
        song={song}
        status={status}
        analysisBusy={analysisBusy}
        supportsAnalysisActions={song.is_analyzed && song.transcript_source !== "Usdx"}
      >
        <article
          className={cn(
            "group relative min-w-0 rounded-md transition-all duration-200 hover:-translate-y-0.5 hover:bg-primary/[0.05]",
            (isFocused || isSelected) &&
              "bg-primary/[0.07] ring-1 ring-foreground/14 shadow-lg shadow-primary/6",
          )}
        >
          <button
            type="button"
            data-song-index={index}
            aria-pressed={isSelected}
            onClick={onSelect}
            className="block w-full cursor-pointer rounded-md p-1.5 text-left outline-none focus-visible:ring-1 focus-visible:ring-foreground/18"
          >
            <AlbumArt
              song={song}
              className="aspect-square w-full rounded-sm shadow-[0_18px_38px_-20px_rgba(0,0,0,.75)] ring-1 ring-border/45"
              fallbackIconClassName="size-8"
              showVideoBadge
            />
            <div className="min-w-0 px-0.5 pb-0.5 pt-2.5">
              <div className="truncate text-xs font-semibold leading-snug">{song.title}</div>
              <p className="mt-1 truncate text-[10px] text-muted-foreground">
                {song.artist || "Unknown artist"}
              </p>
              <div className="mt-2 flex min-w-0 items-center justify-between gap-2">
                <span className="text-xs tabular-nums text-muted-foreground">
                  {formatSeconds(song.duration_secs)}
                </span>
                <div className="flex min-w-0 items-center gap-1">
                  <LanguageBadge language={song.language} />
                  <StatusBadge song={song} queueStatus={queueStatus} />
                </div>
              </div>
            </div>
          </button>
          <div className="absolute right-2 top-2 z-10 rounded-full bg-background/82 opacity-0 shadow-md backdrop-blur-xl transition-opacity group-focus-within:opacity-100 group-hover:opacity-100">
            <SongActionsMenu
              song={song}
              status={status}
              analysisBusy={analysisBusy}
              supportsAnalysisActions={song.is_analyzed && song.transcript_source !== "Usdx"}
              compact
            />
          </div>
        </article>
      </SongContextMenu>
    );
  },
);
