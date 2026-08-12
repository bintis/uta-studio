import { getRecentLogs } from "@/bridge/opener";
import { loadAnalysisTasks } from "@/bridge/songs";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Progress as ProgressBar } from "@/components/ui/progress";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useAnalysisQueue } from "@/queries/use-songs";
import type { QueuedStatus } from "@/types/QueuedStatus";
import type { AnalysisTask } from "@/types/AnalysisTask";
import { Activity, CircleAlert, LoaderCircle, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

const describeStatus = (status: QueuedStatus) => {
  if (status === "Queued") return { label: "Queued", progress: null, failed: false };
  if ("Analyzing" in status)
    return { label: `Analyzing · ${status.Analyzing}%`, progress: status.Analyzing, failed: false };
  return { label: status.Failed || "Failed", progress: null, failed: true };
};

export const ActivityCenter = () => {
  const { data: queue } = useAnalysisQueue();
  const [open, setOpen] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [taskDetails, setTaskDetails] = useState<AnalysisTask[]>([]);
  const logEndRef = useRef<HTMLDivElement>(null);
  const tasks = useMemo(() => Object.entries(queue?.entries ?? {}), [queue?.entries]);
  const displayTasks = useMemo(() => {
    const byHash = new Map(taskDetails.map((task) => [task.file_hash, task]));
    return tasks.map(
      ([fileHash, status]) =>
        byHash.get(fileHash) ?? {
          file_hash: fileHash,
          title: "Song analysis",
          artist: "Loading details…",
          status,
        },
    );
  }, [taskDetails, tasks]);
  const activeCount = tasks.filter(
    ([, status]) => status === "Queued" || (typeof status === "object" && "Analyzing" in status),
  ).length;
  const failedCount = tasks.filter(
    ([, status]) => typeof status === "object" && "Failed" in status,
  ).length;
  const activityLogs = useMemo(
    () => logs.filter((line) => /\[(analyzer|export)\]/i.test(line)).slice(-180),
    [logs],
  );

  const refreshLogs = async () => {
    const next = await getRecentLogs().catch(() => []);
    setLogs(next);
  };

  useEffect(() => {
    void loadAnalysisTasks()
      .then(setTaskDetails)
      .catch(() => setTaskDetails([]));
  }, [queue?.entries]);

  useEffect(() => {
    if (!open && activeCount === 0) return;
    void refreshLogs();
    const interval = window.setInterval(() => void refreshLogs(), 850);
    return () => window.clearInterval(interval);
  }, [activeCount, open]);

  useEffect(() => {
    if (open) logEndRef.current?.scrollIntoView({ block: "end" });
  }, [activityLogs.length, open]);

  return (
    <Sheet open={open} onOpenChange={setOpen}>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        className="relative rounded-full"
        onClick={() => setOpen(true)}
        aria-label={`Activity${activeCount ? `, ${activeCount} active` : ""}`}
        title="Activity"
      >
        {activeCount > 0 ? (
          <LoaderCircle className="animate-spin text-primary" />
        ) : failedCount > 0 ? (
          <CircleAlert className="text-destructive" />
        ) : (
          <Activity />
        )}
        {activeCount + failedCount > 0 ? (
          <span className="absolute -right-0.5 -top-0.5 grid min-w-4 place-items-center rounded-full bg-primary px-1 text-[8px] font-bold text-primary-foreground">
            {activeCount + failedCount}
          </span>
        ) : null}
      </Button>
      <SheetContent className="glass-panel w-[min(94vw,32rem)] sm:max-w-[32rem]" side="right">
        <SheetHeader className="border-b border-border/55 pb-4">
          <div className="flex items-center gap-2">
            <Activity className="size-4 text-primary" />
            <SheetTitle>Activity</SheetTitle>
          </div>
          <SheetDescription>
            Live analysis and export work. This panel updates without interrupting your workspace.
          </SheetDescription>
        </SheetHeader>

        <div className="min-h-0 flex-1 overflow-y-auto px-5 py-4">
          <section aria-labelledby="jobs-title">
            <div className="mb-3 flex items-center justify-between">
              <h2
                id="jobs-title"
                className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground"
              >
                Jobs
              </h2>
              <Badge variant="outline">{tasks.length}</Badge>
            </div>
            {tasks.length === 0 ? (
              <div className="rounded-md border border-dashed border-border/55 p-5 text-center text-xs text-muted-foreground">
                Nothing is running. New analysis and exports will appear here.
              </div>
            ) : (
              <div className="space-y-2">
                {displayTasks.map((task) => {
                  const status = describeStatus(task.status);
                  return (
                    <article
                      key={task.file_hash}
                      className="rounded-md border border-border/55 bg-card/28 p-3"
                    >
                      <div className="flex items-start justify-between gap-3">
                        <div className="min-w-0">
                          <p className="truncate text-xs font-medium">{task.title}</p>
                          <p
                            className="mt-0.5 truncate text-[9px] text-muted-foreground"
                            title={task.file_hash}
                          >
                            {task.artist}
                          </p>
                        </div>
                        <Badge variant={status.failed ? "destructive" : "secondary"}>
                          {status.label}
                        </Badge>
                      </div>
                      {status.progress !== null ? (
                        <ProgressBar value={status.progress} className="mt-3 h-1.5" />
                      ) : null}
                    </article>
                  );
                })}
              </div>
            )}
          </section>

          <section className="mt-6" aria-labelledby="live-log-title">
            <div className="mb-3 flex items-center justify-between gap-2">
              <div>
                <h2
                  id="live-log-title"
                  className="text-[10px] font-semibold uppercase tracking-[0.18em] text-muted-foreground"
                >
                  Live log
                </h2>
                <p className="mt-1 text-[10px] text-muted-foreground">
                  Newest analyzer and exporter messages
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => void refreshLogs()}
                aria-label="Refresh activity log"
              >
                <RefreshCw />
              </Button>
            </div>
            <div
              className="max-h-[46vh] min-h-40 overflow-auto rounded-md border border-border/55 bg-card/35 p-3 font-mono text-[10px] leading-relaxed text-foreground/78"
              role="log"
              aria-live="polite"
            >
              {activityLogs.length > 0
                ? activityLogs.map((line, index) => <div key={`${index}:${line}`}>{line}</div>)
                : "Waiting for activity…"}
              <div ref={logEndRef} />
            </div>
          </section>
        </div>
      </SheetContent>
    </Sheet>
  );
};
