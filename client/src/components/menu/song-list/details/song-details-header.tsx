import { Button } from "@/components/ui/button";
import type { QueuedStatus } from "@/types/QueuedStatus";
import type { Song } from "@/types/Song";
import { XIcon } from "lucide-react";
import { formatSeconds } from "@/utils/format-duration";
import { AlbumArt } from "../shared/album-art";
import { LanguageBadge, isDisplayableLanguage } from "../shared/language-badge";
import { StatusBadge } from "../shared/status-badge";

interface SongDetailsHeaderProps {
  song: Song;
  queueStatus?: QueuedStatus;
  onClose: () => void;
}

export const SongDetailsHeader = ({ song, queueStatus, onClose }: SongDetailsHeaderProps) => (
  <header className="relative border-b border-border/55 bg-gradient-to-b from-primary/[0.055] to-transparent px-5 pb-5 pt-5">
    <Button
      variant="ghost"
      size="icon-sm"
      className="absolute right-3 top-3 z-10 bg-background/45 backdrop-blur-md"
      onClick={onClose}
      aria-label="Close song details"
    >
      <XIcon />
    </Button>

    <div className="mx-auto flex max-w-[14rem] flex-col items-center text-center">
      <AlbumArt
        song={song}
        className="aspect-square w-full rounded-sm shadow-[0_28px_55px_-25px_rgba(0,0,0,.78)] ring-1 ring-border/55"
        fallbackIconClassName="size-10"
        lazy={false}
      />

      <div className="mt-4 min-w-0 w-full">
        <p className="text-[8px] font-medium uppercase tracking-[0.2em] text-primary">
          Selected song
        </p>
        <h2 className="mt-1 line-clamp-2 text-base font-light leading-snug text-balance">
          {song.title}
        </h2>
        <p className="mt-1.5 truncate text-[11px] text-muted-foreground">
          {song.artist || "Unknown band"}
        </p>
        <p className="mt-0.5 truncate text-[10px] text-muted-foreground/70">
          {song.album || "Unknown album"}
        </p>
      </div>
    </div>

    <div className="mt-4 flex flex-wrap items-center justify-center gap-2 text-[10px] text-muted-foreground">
      <StatusBadge song={song} queueStatus={queueStatus} />
      {isDisplayableLanguage(song.language) ? (
        <>
          <span aria-hidden="true">·</span>
          <LanguageBadge language={song.language} />
        </>
      ) : null}
      <span aria-hidden="true">·</span>
      <span className="tabular-nums">{formatSeconds(song.duration_secs)}</span>
    </div>
  </header>
);
