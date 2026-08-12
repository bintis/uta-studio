import { SegmentedProgress } from "@/components/ui/segmented-progress";
import { useCacheStats } from "@/queries/use-cache-stats";
import { formatBytes, segmentPercent, totalUsedBytes } from "@/utils/stats";

const rows = [
  { label: "Songs", color: "bg-blue-500", key: "songs_bytes" as const },
  { label: "Models", color: "bg-yellow-500", key: "models_bytes" as const },
  { label: "Other", color: "bg-gray-500", key: "other_bytes" as const },
];

export const Stats = () => {
  const { data: stats, isLoading, isError } = useCacheStats();

  const total = stats ? totalUsedBytes(stats) : 0n;

  const noStatsLabel = isError ? "Cache stats unavailable" : "…";

  const label = !stats ? noStatsLabel : `${formatBytes(total)} used`;

  const renderHeader = () => <span className="text-[9px] text-muted-foreground">{label}</span>;

  if (isLoading || !stats) {
    return (
      <div className="flex flex-col gap-1.5 px-2">
        {renderHeader()}
        {!isError && <div className="h-2 w-full rounded-full bg-muted" />}
      </div>
    );
  }

  const segments = rows.map((row) => ({
    value: segmentPercent(stats[row.key], total),
    color: row.color,
  }));

  return (
    <div className="flex flex-col gap-1.5 px-2">
      {renderHeader()}
      <SegmentedProgress segments={segments} />
      <div className="hidden flex-col gap-0.5 2xl:flex">
        {rows.map((row) => (
          <div key={row.label} className="flex flex-col gap-0 py-0.5 text-xs">
            <div className="flex items-center gap-2">
              <div className={`size-2 shrink-0 rounded-full ${row.color}`} />
              <span className="flex-1">{row.label}</span>
              <span className="text-muted-foreground">{formatBytes(stats[row.key])}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
};
