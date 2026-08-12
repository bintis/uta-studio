import type { SongStatusInfo } from "@/components/menu/song-list/shared/song-status";
import { useAnalysis } from "@/hooks/use-analysis";
import type { Song } from "@/types/Song";
import { useCallback } from "react";
import { useNavigate } from "react-router";
import { useAnalysisRuntimeStatus } from "@/queries/use-analysis-runtime-status";

export const useSongEditorAction = (
  song: Song | null | undefined,
  status: SongStatusInfo | null | undefined,
  analysisBusy: boolean,
) => {
  const navigate = useNavigate();
  const analysis = useAnalysis();
  const runtime = useAnalysisRuntimeStatus();
  const analyzed = song?.is_analyzed === true;
  const runtimeChecking = !analyzed && runtime.isLoading;
  const needsSetup = !analyzed && !runtime.isLoading && runtime.data?.ready !== true;

  const trigger = useCallback(() => {
    if (!song || analysisBusy || runtimeChecking || needsSetup) return;
    if (!analyzed) {
      void analysis.enqueueOne(song.file_hash);
      return;
    }
    navigate("/editor", { state: { song } });
  }, [analysis, analysisBusy, analyzed, navigate, needsSetup, runtimeChecking, song]);

  const openAnalysisSettings = useCallback(() => {
    navigate("/settings?tab=models");
  }, [navigate]);

  return {
    disabled: !song || analysisBusy || runtimeChecking || needsSetup,
    trigger,
    openAnalysisSettings,
    needsSetup,
    label: !song
      ? "Select a song"
      : analysisBusy
        ? status?.label || "Analyzing…"
        : runtimeChecking
          ? "Checking analysis…"
          : needsSetup
            ? "Analysis unavailable"
            : !analyzed
              ? "Analyze first"
              : song.editor_ready
                ? "Edit chart"
                : "Prepare & edit",
    description: analysisBusy
      ? "The editor unlocks when analysis finishes"
      : runtimeChecking
        ? "Checking the local runtime and model files"
        : needsSetup
          ? `Install or repair ${runtime.data?.missing.join(", ") || "analysis components"} in Settings`
          : !analyzed
            ? "Generate lyrics, timing, pitch, and stems before editing"
            : song.editor_ready
              ? "Open the full timing and pitch workspace"
              : "Complete the remaining pitch assets, then open the editor",
    analyzed,
  };
};
