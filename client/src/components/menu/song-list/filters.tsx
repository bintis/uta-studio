import { Button } from "@/components/ui/button";
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { useMenuFocus } from "@/contexts/menu-focus-context";
import { useAnalysis } from "@/hooks/use-analysis";
import { useLibraryFilter } from "@/hooks/use-library-filter";
import { cn } from "@/lib/utils";
import { AudioLinesIcon, Grid2X2Icon, ListIcon, SettingsIcon } from "lucide-react";
import { useEffect } from "react";
import { useAnalysisRuntimeStatus } from "@/queries/use-analysis-runtime-status";
import { useNavigate } from "react-router";

export type SongListView = "table" | "grid";

interface FiltersProps {
  view: SongListView;
  onViewChange: (view: SongListView) => void;
  isSavingView?: boolean;
}

export const Filters = ({ view, onViewChange, isSavingView }: FiltersProps) => {
  const { status, transcript_source, setLibraryFilter } = useLibraryFilter();
  const { enqueueAll } = useAnalysis();
  const navigate = useNavigate();
  const { data: runtimeStatus, isLoading: runtimeStatusLoading } = useAnalysisRuntimeStatus();
  const { focus, actionsRef } = useMenuFocus();

  useEffect(() => {
    actionsRef.current.onConfirmAnalyzeAll =
      runtimeStatus?.ready === true ? enqueueAll : () => navigate("/settings?tab=models");
    return () => {
      actionsRef.current.onConfirmAnalyzeAll = null;
    };
  }, [actionsRef, enqueueAll, navigate, runtimeStatus?.ready]);

  const isAnalyzeAllFocused = focus.active && focus.panel === "songList" && focus.analyzeAllFocused;

  return (
    <div
      className="flex w-full flex-wrap items-center justify-start gap-2 lg:justify-end"
      aria-label="Song library controls"
    >
      <Select
        value={status ?? "all"}
        onValueChange={(value) =>
          setLibraryFilter((current) => ({
            ...current,
            status: value === "all" ? null : value,
          }))
        }
      >
        <SelectTrigger
          aria-label="Filter by analysis status"
          className="w-[calc(50%-0.25rem)] min-w-28 border-0 bg-muted/50 sm:w-32"
        >
          <SelectValue placeholder="Status" />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            <SelectLabel>Status</SelectLabel>
            <SelectItem value="all">All statuses</SelectItem>
            <SelectItem value="not_analyzed">Not analyzed</SelectItem>
            <SelectItem value="queued">Queued</SelectItem>
            <SelectItem value="analyzing">Analyzing</SelectItem>
            <SelectItem value="analyzed">Analyzed</SelectItem>
            <SelectItem value="failed">Failed</SelectItem>
          </SelectGroup>
        </SelectContent>
      </Select>
      <Select
        value={transcript_source ?? "all"}
        onValueChange={(value) =>
          setLibraryFilter((current) => ({
            ...current,
            transcript_source: value === "all" ? null : value,
          }))
        }
      >
        <SelectTrigger
          aria-label="Filter by transcript type"
          className="w-[calc(50%-0.25rem)] min-w-28 border-0 bg-muted/50 sm:w-32"
        >
          <SelectValue placeholder="Source" />
        </SelectTrigger>
        <SelectContent>
          <SelectGroup>
            <SelectLabel>Type</SelectLabel>
            <SelectItem value="all">All types</SelectItem>
            <SelectItem value="generated">Generated</SelectItem>
            <SelectItem value="lyrics">AI Aligned</SelectItem>
            <SelectItem value="lrc">LRC</SelectItem>
          </SelectGroup>
        </SelectContent>
      </Select>
      <div className="flex items-center justify-end gap-2">
        {runtimeStatus?.ready === false ? (
          <Button
            tabIndex={-1}
            variant="ghost"
            size="sm"
            onClick={() => navigate("/settings?tab=models")}
          >
            <SettingsIcon /> Set up analysis
          </Button>
        ) : null}
        <Button
          tabIndex={-1}
          variant="outline"
          disabled={runtimeStatusLoading || runtimeStatus?.ready !== true}
          onClick={enqueueAll}
          title={
            runtimeStatus?.ready
              ? "Analyze every unprocessed song"
              : "Set up the analysis runtime and models in Settings > Models & runtime"
          }
          data-analyze-all-focus="true"
          className={cn(
            "h-8 w-8 border-0 bg-primary px-0 text-primary-foreground focus-visible:border-transparent focus-visible:ring-0 sm:w-auto sm:min-w-28 sm:px-3",
            isAnalyzeAllFocused && "ring-2 ring-primary",
          )}
        >
          <AudioLinesIcon />
          <span className="sr-only sm:not-sr-only">Analyze all</span>
        </Button>
        <div
          className="flex shrink-0 rounded-full bg-muted/55 p-0.5"
          role="group"
          aria-label="Song list view"
        >
          <Button
            variant={view === "table" ? "secondary" : "ghost"}
            size="icon-sm"
            disabled={isSavingView}
            onClick={() => onViewChange("table")}
            aria-label="Table view"
            aria-pressed={view === "table"}
            title="Table view"
          >
            <ListIcon />
          </Button>
          <Button
            variant={view === "grid" ? "secondary" : "ghost"}
            size="icon-sm"
            disabled={isSavingView}
            onClick={() => onViewChange("grid")}
            aria-label="Card grid view"
            aria-pressed={view === "grid"}
            title="Card grid view"
          >
            <Grid2X2Icon />
          </Button>
        </div>
      </div>
    </div>
  );
};
