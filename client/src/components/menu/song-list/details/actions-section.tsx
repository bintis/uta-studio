import { Separator } from "@/components/ui/separator";
import { useAnalysis } from "@/hooks/use-analysis";
import { useDialog } from "@/hooks/use-dialog";
import { exportSongPackage, exportUltraStar } from "@/bridge/utz";
import type { Song } from "@/types/Song";
import { Fragment, useRef, useState } from "react";
import { toast } from "sonner";
import type { SongStatusInfo } from "../shared/song-status";
import { ActionItem } from "./action-item";
import { buildActionGroups } from "./song-actions";
import { useAnalysisRuntimeStatus } from "@/queries/use-analysis-runtime-status";
import { useNavigate } from "react-router";
import { useConfig } from "@/queries/use-config";

interface ActionsSectionProps {
  song: Song;
  status: SongStatusInfo;
  analysisBusy: boolean;
  supportsAnalysisActions: boolean;
  menuMode?: boolean;
}

export const ActionsSection = ({
  song,
  status,
  analysisBusy,
  supportsAnalysisActions,
  menuMode = false,
}: ActionsSectionProps) => {
  const { setMode } = useDialog();
  const analysis = useAnalysis();
  const navigate = useNavigate();
  const { data: runtimeStatus } = useAnalysisRuntimeStatus();
  const { data: config } = useConfig();
  const [exporting, setExporting] = useState<"utz" | "ultrastar" | null>(null);
  const exportingRef = useRef(false);

  const run = (message: string, action: () => void | Promise<void>) => async () => {
    await action();
    toast.info(message);
  };

  const groups = buildActionGroups({
    song,
    status,
    analysisBusy: analysisBusy || exporting !== null,
    supportsAnalysisActions,
    analysisReady: runtimeStatus?.ready === true,
    analysisNeedsSetup: runtimeStatus?.ready === false,
    analysis,
    onEditLyrics: () => setMode({ mode: "edit-lyrics", song }),
    onChangeLanguage: () => setMode({ mode: "language", song }),
    onSetupAnalysis: () => navigate("/settings?tab=models"),
    onExportUtz: async () => {
      if (exportingRef.current) return;
      exportingRef.current = true;
      setExporting("utz");
      const toastId = toast.loading(`Exporting “${song.title}”…`, {
        description: "Validating assets and writing the package safely.",
      });
      try {
        const output = await exportSongPackage(
          song,
          (progress) => {
            toast.loading(`Exporting “${song.title}” · ${progress.percent}%`, {
              id: toastId,
              description: progress.asset
                ? `${progress.phase} · ${progress.asset}`
                : progress.phase,
            });
          },
          config,
        );
        toast.dismiss(toastId);
        if (output) toast.success(`Exported “${song.title}”`, { description: output });
      } catch (error) {
        toast.dismiss(toastId);
        toast.error("Could not export .utz", { description: String(error) });
      } finally {
        exportingRef.current = false;
        setExporting(null);
      }
    },
    onExportUltraStar: async () => {
      if (exportingRef.current) return;
      exportingRef.current = true;
      setExporting("ultrastar");
      const toastId = toast.loading(`Exporting “${song.title}”…`, {
        description: "Writing the chart and linked media files.",
      });
      try {
        const output = await exportUltraStar(song, config);
        toast.dismiss(toastId);
        if (output)
          toast.success(`Exported UltraStar chart for “${song.title}”`, { description: output });
      } catch (error) {
        toast.dismiss(toastId);
        toast.error("Could not export UltraStar chart", { description: String(error) });
      } finally {
        exportingRef.current = false;
        setExporting(null);
      }
    },
    run,
  });

  return (
    <section
      className={menuMode ? "px-2 py-2" : "px-3 py-4"}
      aria-labelledby="song-actions-heading"
    >
      <h3
        id="song-actions-heading"
        className={
          menuMode
            ? "sr-only"
            : "mb-2 px-2 text-[9px] font-semibold uppercase tracking-[0.16em] text-muted-foreground"
        }
      >
        Production actions
      </h3>
      <div className="flex flex-col gap-1">
        {groups.map((group, groupIndex) => (
          <Fragment key={groupIndex}>
            {groupIndex > 0 ? (
              <Separator className={menuMode ? "my-1 bg-border/55" : "my-1"} />
            ) : null}
            {group.map((item) => (
              <ActionItem key={item.title} {...item} menuMode={menuMode} />
            ))}
          </Fragment>
        ))}
      </div>
    </section>
  );
};
