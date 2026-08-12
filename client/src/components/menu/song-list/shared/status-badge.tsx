import { Badge } from "@/components/ui/badge";
import { cn } from "@/lib/utils";
import type { QueuedStatus } from "@/types/QueuedStatus";
import type { Song } from "@/types/Song";
import { LoaderCircleIcon } from "lucide-react";
import { formatTranscriptSource, getSongStatusInfo } from "./song-status";

export function StatusBadge({ song, queueStatus }: { song: Song; queueStatus?: QueuedStatus }) {
  const status = getSongStatusInfo(song, queueStatus);
  const source = status.isReady ? ` (${formatTranscriptSource(song.transcript_source)})` : "";

  return (
    <Badge variant={status.variant} className={cn("border-foreground/15", status.className)}>
      {status.isAnalyzing ? <LoaderCircleIcon className="animate-spin" /> : null}
      {status.label}
      {source}
    </Badge>
  );
}
