import { Button } from "@/components/ui/button";
import { Separator } from "@/components/ui/separator";
import type { QueuedStatus } from "@/types/QueuedStatus";
import type { Song } from "@/types/Song";
import { SONGS } from "@/queries/keys";
import { useQueryClient } from "@tanstack/react-query";
import { PencilRuler } from "lucide-react";
import { useEffect, useState } from "react";
import { ActionsSection } from "./details/actions-section";
import { KeyTempoSection } from "./details/key-tempo-section";
import { SongDetailsHeader } from "./details/song-details-header";
import { useSongDetailsNav } from "./details/use-song-details-nav";
import { getSongStatusInfo } from "./shared/song-status";
import type { ShiftType } from "./shifts";
import { useSongEditorAction } from "@/hooks/use-song-editor-action";

interface SongDetailsSidebarProps {
  song: Song;
  queueStatus?: QueuedStatus;
  onClose: () => void;
}

export const SongDetailsSidebar = ({ song, queueStatus, onClose }: SongDetailsSidebarProps) => {
  const queryClient = useQueryClient();
  const { detailsRef, closeDetails } = useSongDetailsNav(onClose);
  const [shifting, setShifting] = useState<Record<ShiftType, boolean>>({
    tempo: false,
    key: false,
  });

  const status = getSongStatusInfo(song, queueStatus);
  const analysisBusy = queueStatus === "Queued" || Boolean(status.isAnalyzing);
  // LRC songs based on the original mix become editable immediately while their
  // key is still being detected off-queue. Until the key lands, treat the
  // key/tempo section as pending rather than showing controls.
  const keyPending =
    song.is_analyzed && song.transcript_source === "Lrc" && song.no_stems && song.key === null;
  const supportsShifts = song.is_analyzed && song.transcript_source !== "Usdx" && !keyPending;
  const supportsAnalysisActions = song.is_analyzed && song.transcript_source !== "Usdx";
  const editorAction = useSongEditorAction(song, status, analysisBusy);

  // Off-queue key detection doesn't invalidate any query, so poll the song list
  // while the key is pending to pick it up and unlock the shift controls.
  useEffect(() => {
    if (!keyPending) return;
    const interval = setInterval(() => {
      queryClient.invalidateQueries({ queryKey: SONGS });
    }, 2000);
    return () => clearInterval(interval);
  }, [keyPending, queryClient]);

  return (
    <aside
      ref={detailsRef}
      className="roon-inspector flex min-h-0 min-w-0 flex-1 flex-col overflow-hidden border-l border-border/55 backdrop-blur-2xl [&_[data-song-details-focused=true]]:z-10 [&_[data-song-details-focused=true]]:bg-foreground/[0.035] xl:w-[18rem] xl:flex-none 2xl:w-[20rem]"
      aria-label="Song details"
    >
      <SongDetailsHeader song={song} queueStatus={queueStatus} onClose={closeDetails} />

      <div className="no-scrollbar min-h-0 flex-1 overflow-y-auto">
        <KeyTempoSection
          song={song}
          supportsShifts={supportsShifts}
          shifting={shifting}
          setShifting={setShifting}
        />

        <Separator />

        <ActionsSection
          song={song}
          status={status}
          analysisBusy={analysisBusy}
          supportsAnalysisActions={supportsAnalysisActions}
        />
      </div>

      <footer className="border-t border-border/55 bg-card/28 p-3">
        <Button
          size="lg"
          className="h-10 w-full text-sm"
          disabled={editorAction.disabled || shifting.key || shifting.tempo}
          onClick={editorAction.trigger}
          title={editorAction.description}
        >
          <PencilRuler /> {editorAction.label}
        </Button>
      </footer>
    </aside>
  );
};
