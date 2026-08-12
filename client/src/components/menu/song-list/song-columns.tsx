import type { QueuedStatus } from "@/types/QueuedStatus";
import type { Song } from "@/types/Song";
import type { ReactNode } from "react";
import { formatSeconds } from "@/utils/format-duration";
import { AlbumArt } from "./shared/album-art";
import { LanguageBadge } from "./shared/language-badge";
import { StatusBadge } from "./shared/status-badge";
import { getSongStatusInfo } from "./shared/song-status";
import { SongActionsMenu } from "./song-actions-menu";

interface SongColumn {
  id: string;
  header: ReactNode;
  thClassName: string;
  tdClassName: string;
  cell: (song: Song, queueStatus?: QueuedStatus) => ReactNode;
}

export const SONG_COLUMNS: SongColumn[] = [
  {
    id: "thumbnail",
    header: <span className="sr-only">Cover</span>,
    thClassName: "song-table__thumbnail pl-6 pr-2 py-2.5 font-medium",
    tdClassName: "song-table__thumbnail py-1.5 pr-2 pl-6",
    cell: (song) => (
      <AlbumArt
        song={song}
        className="size-11 rounded-sm shadow-sm"
        fallbackIconClassName="size-5"
        showVideoBadge
      />
    ),
  },
  {
    id: "song",
    header: "Track",
    thClassName: "song-table__song px-2 py-2.5 font-normal",
    tdClassName: "song-table__song px-2 py-2 align-middle font-medium",
    cell: (song) => (
      <div className="flex h-5 min-w-0 items-center gap-2">
        <span className="min-w-0 truncate leading-5">{song.title}</span>
        <LanguageBadge language={song.language} />
      </div>
    ),
  },
  {
    id: "band",
    header: "Artist",
    thClassName: "song-table__band px-2 py-2.5 font-normal",
    tdClassName: "song-table__band px-2 py-2 text-muted-foreground",
    cell: (song) => <span className="block truncate">{song.artist || "—"}</span>,
  },
  {
    id: "album",
    header: "Album",
    thClassName: "song-table__album px-2 py-2.5 font-normal",
    tdClassName: "song-table__album px-2 py-2 text-muted-foreground",
    cell: (song) => <span className="block truncate">{song.album || "—"}</span>,
  },
  {
    id: "duration",
    header: "Duration",
    thClassName: "song-table__duration px-2 py-2.5 font-normal",
    tdClassName:
      "song-table__duration px-2 py-2 font-variant-numeric tabular-nums text-muted-foreground",
    cell: (song) => formatSeconds(song.duration_secs),
  },
  {
    id: "status",
    header: "Analysis status",
    thClassName: "song-table__status py-2.5 pl-2 pr-6 text-right font-normal",
    tdClassName: "song-table__status py-2 pl-2 pr-6 text-right",
    cell: (song, queueStatus) => <StatusBadge song={song} queueStatus={queueStatus} />,
  },
  {
    id: "actions",
    header: <span className="sr-only">Actions</span>,
    thClassName: "song-table__actions py-2.5 pr-3",
    tdClassName: "song-table__actions py-2 pr-3 text-right",
    cell: (song, queueStatus) => {
      const status = getSongStatusInfo(song, queueStatus);
      const analysisBusy = queueStatus === "Queued" || Boolean(status.isAnalyzing);

      return (
        <div
          className="inline-flex"
          onClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => event.stopPropagation()}
        >
          <SongActionsMenu
            song={song}
            status={status}
            analysisBusy={analysisBusy}
            supportsAnalysisActions={song.is_analyzed && song.transcript_source !== "Usdx"}
            compact
          />
        </div>
      );
    },
  },
];
