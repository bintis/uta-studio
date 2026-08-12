import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { LrclibCandidate } from "@/types/LrclibCandidate";
import { formatSeconds } from "@/utils/edit-lyrics";
import { Loader2Icon } from "lucide-react";
import { ringFor } from "./parts";

interface LrclibMatchesProps {
  candidates: LrclibCandidate[];
  isLoading: boolean;
  isError: boolean;
  errorMessage: string | null;
  index: number;
  onSelect: (candidate: LrclibCandidate) => void;
  onUseLrc: (candidate: LrclibCandidate) => void;
  isFocused: (slot: number) => boolean;
}

export const LrclibMatches = ({
  candidates,
  isLoading,
  isError,
  errorMessage,
  index,
  onSelect,
  onUseLrc,
  isFocused,
}: LrclibMatchesProps) => {
  if (isLoading) {
    return (
      <div className="flex flex-1 items-center justify-center gap-2 text-muted-foreground">
        <Loader2Icon className="size-4 animate-spin" />
        <span>Searching LRCLIB…</span>
      </div>
    );
  }

  if (isError) {
    return <p className="text-destructive">{errorMessage ?? "Failed to load LRCLIB matches"}</p>;
  }

  if (candidates.length === 0) {
    return <p className="text-muted-foreground">No LRCLIB matches found for this song.</p>;
  }

  const safeIndex = Math.min(index, candidates.length - 1);
  const candidate = candidates[safeIndex];
  const hasLrc = candidate.synced_lyrics != null;

  return (
    <div className="flex min-h-0 flex-1 flex-col gap-2">
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="truncate text-sm font-medium">{candidate.track_name}</span>
        <span className="truncate text-xs text-muted-foreground">
          {candidate.artist_name}
          {candidate.album_name ? ` • ${candidate.album_name}` : ""} •{" "}
          {formatSeconds(candidate.duration_secs)}
          {" • "}
          {candidate.lines.length} {candidate.lines.length === 1 ? "line" : "lines"}
          {hasLrc && <span className="text-primary"> • LRC available</span>}
        </span>
        <div className="mt-1.5 flex items-center gap-2">
          {hasLrc && (
            <Button
              size="xs"
              variant="default"
              onClick={() => onUseLrc(candidate)}
              className={ringFor(isFocused(0))}
            >
              Use LRC
            </Button>
          )}
          <Button
            size="xs"
            variant={hasLrc ? "outline" : "default"}
            onClick={() => onSelect(candidate)}
            className={cn(ringFor(isFocused(hasLrc ? 1 : 0)))}
          >
            Use as plain text
          </Button>
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto rounded-md border border-border">
        <pre className="px-3 py-2 font-mono text-[11px] leading-relaxed whitespace-pre-wrap">
          {hasLrc ? candidate.synced_lyrics : candidate.lines.join("\n")}
        </pre>
      </div>
    </div>
  );
};
