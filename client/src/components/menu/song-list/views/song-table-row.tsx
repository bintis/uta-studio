import { cn } from "@/lib/utils";
import { memo, type KeyboardEvent } from "react";
import { SONG_COLUMNS } from "../song-columns";
import type { SongItemProps } from "../types";
import { getSongStatusInfo } from "../shared/song-status";
import { SongContextMenu } from "../song-context-menu";

export const SongTableRow = memo(
  ({ song, queueStatus, index, isFocused, isSelected, onSelect }: SongItemProps) => {
    const status = getSongStatusInfo(song, queueStatus);
    const analysisBusy = queueStatus === "Queued" || Boolean(status.isAnalyzing);
    const onKeyDown = (event: KeyboardEvent<HTMLTableRowElement>) => {
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      onSelect();
    };

    return (
      <SongContextMenu
        song={song}
        status={status}
        analysisBusy={analysisBusy}
        supportsAnalysisActions={song.is_analyzed && song.transcript_source !== "Usdx"}
      >
        <tr
          tabIndex={0}
          data-song-index={index}
          aria-selected={isSelected}
          onClick={onSelect}
          onKeyDown={onKeyDown}
          className={cn(
            "cursor-pointer border-b border-border/45 outline-none [&>td]:bg-transparent [&>td]:transition-colors hover:[&>td]:bg-primary/[0.055] focus-visible:[&>td]:bg-primary/12",
            (isFocused || isSelected) && "[&>td]:bg-primary/12 hover:[&>td]:bg-primary/15",
          )}
        >
          {SONG_COLUMNS.map((column) => (
            <td key={column.id} className={column.tdClassName}>
              {column.cell(song, queueStatus)}
            </td>
          ))}
        </tr>
      </SongContextMenu>
    );
  },
);
