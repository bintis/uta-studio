import { convertFileSrc } from "@/bridge/media";
import { cn } from "@/lib/utils";
import type { Song } from "@/types/Song";
import { MusicIcon, VideoIcon } from "lucide-react";

interface AlbumArtProps {
  song: Song;
  className?: string;
  fallbackIconClassName?: string;
  showVideoBadge?: boolean;
  lazy?: boolean;
}

export function AlbumArt({
  song,
  className,
  fallbackIconClassName,
  showVideoBadge,
  lazy = true,
}: AlbumArtProps) {
  return (
    <div
      className={cn("relative shrink-0 overflow-hidden bg-muted text-muted-foreground", className)}
    >
      {song.album_art_path ? (
        <img
          src={convertFileSrc(song.album_art_path)}
          alt=""
          loading={lazy ? "lazy" : undefined}
          decoding={lazy ? "async" : undefined}
          className="size-full object-cover"
        />
      ) : (
        <MusicIcon
          className={cn("absolute inset-0 m-auto", fallbackIconClassName)}
          aria-hidden="true"
        />
      )}
      {showVideoBadge && song.is_video ? (
        <VideoIcon className="absolute right-1 bottom-1 size-3 rounded-sm bg-background/85 p-0.5" />
      ) : null}
    </div>
  );
}
