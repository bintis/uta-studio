import { onSetupError, onSetupLog, onSetupProgress, triggerSetup } from "@/bridge/setup";
import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavInput } from "@/hooks/navigation/use-nav-input";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Progress } from "@/components/ui/progress";
import type { SetupProgress } from "@/types/SetupProgress";
import type { SetupStep } from "@/types/SetupStep";
import logoSrc from "../../../../../client/src-tauri/icons/icon.png";
import { useShouldRunSetup } from "@/hooks/use-should-run-setup";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { useConfig } from "@/queries/use-config";
import {
  ANALYSIS_QUEUE,
  ANALYSIS_RUNTIME_STATUS,
  CONFIG,
  MENU,
  SONGS,
  SONGS_META,
} from "@/queries/keys";
import { useQueryClient } from "@tanstack/react-query";
import type { ComputeBackend } from "@/types/ComputeBackend";
import type { SetupTask } from "@/types/SetupTask";
import { COMPUTE_BACKENDS } from "@/components/menu/settings/constants";
import { Check, ChevronDown, LoaderCircle } from "lucide-react";
import { formatBytes } from "@/utils/stats";

interface ExtendedSetupProgress extends Omit<SetupProgress, "step"> {
  step: SetupStep | "init" | "backend" | "error";
}

type InitialStepProps = {
  toNextStep: () => void;
  onCancel: () => void;
  modelLabel?: string;
};

const InitialStep = ({ toNextStep, onCancel, modelLabel }: InitialStepProps) => {
  return (
    <>
      <AlertDialogHeader>
        <AlertDialogTitle>
          {modelLabel ? `Download ${modelLabel}` : "Set up analysis"}
        </AlertDialogTitle>
        <AlertDialogDescription>
          {modelLabel
            ? `Uta Studio will prepare only ${modelLabel}. If the shared analysis runtime is missing, it will be installed first.`
            : "Uta Studio will reuse compatible system tools and existing model files, then install only the missing analysis runtime or models."}
        </AlertDialogDescription>
        <AlertDialogDescription>
          Nothing is downloaded until you continue. You can leave analysis unavailable and keep
          using the library and editor for charts that already exist.
        </AlertDialogDescription>
      </AlertDialogHeader>
      <AlertDialogFooter>
        <AlertDialogCancel onClick={onCancel}>Not now</AlertDialogCancel>
        <Button onClick={toNextStep}>Continue</Button>
      </AlertDialogFooter>
    </>
  );
};

const ComputeBackendStep = ({
  value,
  onChange,
  onContinue,
  onCancel,
}: {
  value: ComputeBackend;
  onChange: (value: ComputeBackend) => void;
  onContinue: () => void;
  onCancel: () => void;
}) => (
  <>
    <AlertDialogHeader className="w-full place-items-stretch text-left">
      <AlertDialogTitle>AI acceleration</AlertDialogTitle>
      <AlertDialogDescription>
        Choose the runtime Uta Studio should install. This controls the model packages, so changing
        it later requires re-running setup.
      </AlertDialogDescription>
      <div className="mt-2 grid gap-2">
        {COMPUTE_BACKENDS.map((backend) => {
          const selected = backend.value === value;
          return (
            <Button
              key={backend.value}
              type="button"
              variant={selected ? "default" : "outline"}
              className="h-auto items-start justify-start whitespace-normal px-3 py-2 text-left"
              onClick={() => onChange(backend.value as ComputeBackend)}
            >
              <span className="flex flex-col gap-0.5">
                <span>{backend.label}</span>
                <span className="text-[0.7rem] font-normal opacity-80">{backend.description}</span>
              </span>
            </Button>
          );
        })}
      </div>
      {value === "intel" && (
        <AlertDialogDescription className="mt-1 text-xs">
          Intel Arc is experimental for some larger models. Unsupported operations fall back to CPU.
        </AlertDialogDescription>
      )}
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogCancel onClick={onCancel}>Cancel</AlertDialogCancel>
      <Button onClick={onContinue}>Continue</Button>
    </AlertDialogFooter>
  </>
);

interface LoadStepProps {
  action: string;
  percent: number;
  tasks: SetupTask[];
  logs: string[];
  detailsOpen: boolean;
  onDetailsOpenChange: (open: boolean) => void;
}

const LoadStep = ({
  action,
  percent,
  tasks,
  logs,
  detailsOpen,
  onDetailsOpenChange,
}: LoadStepProps) => (
  <>
    <AlertDialogHeader>
      <AlertDialogTitle>Setting up Uta Studio</AlertDialogTitle>
      <div className="flex flex-col gap-2 w-full">
        <AlertDialogDescription className="w-full">{action}</AlertDialogDescription>
        <Progress value={percent} />
        <Collapsible className="pt-1" open={detailsOpen} onOpenChange={onDetailsOpenChange}>
          <CollapsibleTrigger asChild>
            <Button variant="ghost" size="sm" className="w-fit px-0 text-muted-foreground">
              <ChevronDown />
              Details · {tasks.length} tasks
            </Button>
          </CollapsibleTrigger>
          <CollapsibleContent className="max-h-72 overflow-y-auto rounded-md border border-border/60 bg-muted/20 p-2">
            <div className="flex flex-col gap-1.5 text-xs">
              {tasks.map((task) => {
                const transferred =
                  task.downloaded_bytes == null
                    ? null
                    : task.total_bytes == null
                      ? formatBytes(task.downloaded_bytes)
                      : `${formatBytes(task.downloaded_bytes)} / ${formatBytes(task.total_bytes)}`;
                return (
                  <div key={task.step} className="flex items-center gap-2">
                    {task.state === "done" ? (
                      <Check className="size-3.5 text-emerald-500" />
                    ) : task.state === "running" ? (
                      <LoaderCircle className="size-3.5 animate-spin text-primary" />
                    ) : (
                      <span className="size-3.5 rounded-full border border-muted-foreground/50" />
                    )}
                    <span className="min-w-0 flex-1 truncate">{task.label}</span>
                    {transferred && (
                      <span className="shrink-0 font-variant-numeric tabular-nums text-muted-foreground">
                        {task.step === "dependencies" ? `${transferred} installed` : transferred}
                      </span>
                    )}
                  </div>
                );
              })}
            </div>
            <div className="mt-2 border-t border-border/60 pt-2">
              <div className="mb-1 text-[0.65rem] font-medium uppercase tracking-wide text-muted-foreground">
                Installer output
              </div>
              <pre className="max-h-36 overflow-auto whitespace-pre-wrap break-all font-mono text-[0.65rem]/relaxed text-muted-foreground">
                {logs.length > 0 ? logs.join("\n") : "Waiting for installer output…"}
              </pre>
            </div>
          </CollapsibleContent>
        </Collapsible>
      </div>
    </AlertDialogHeader>
  </>
);

interface ErrorStepProps {
  error: string;
  onClose: () => void;
}

const ErrorStep = ({ error, onClose }: ErrorStepProps) => (
  <>
    <AlertDialogHeader>
      <AlertDialogTitle>Something went wrong</AlertDialogTitle>
      <AlertDialogDescription>
        <code>{error}</code>
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogAction onClick={onClose}>Close</AlertDialogAction>
    </AlertDialogFooter>
  </>
);

interface FinalStepProps {
  onFinish: () => void;
  modelLabel?: string;
}

const FinalStep = ({ onFinish, modelLabel }: FinalStepProps) => (
  <>
    <AlertDialogHeader>
      <AlertDialogTitle>{modelLabel ? "Model ready" : "You're all set!"}</AlertDialogTitle>
      <AlertDialogDescription>
        {modelLabel
          ? `${modelLabel} is available locally. Other missing models can be downloaded independently from Settings.`
          : "Analysis is ready. Compatible system tools and existing model files were reused wherever possible."}
      </AlertDialogDescription>
    </AlertDialogHeader>
    <AlertDialogFooter>
      <AlertDialogAction onClick={onFinish}>Get Started</AlertDialogAction>
    </AlertDialogFooter>
  </>
);

const defaultProgress = {
  step: "init" as const,
  percent: 0,
  action: "",
  tasks: [],
};

export const Setup = () => {
  const { data: config } = useConfig();
  const { shouldRunSetup, setupRequest, setShouldRunSetup } = useShouldRunSetup();
  const queryClient = useQueryClient();

  const [setupProgress, setSetupProgress] = useState<ExtendedSetupProgress>(defaultProgress);
  const [computeBackend, setComputeBackend] = useState<ComputeBackend>("cpu");
  const [setupLogs, setSetupLogs] = useState<string[]>([]);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const modelTarget = setupRequest?.modelTarget;
  const modelLabel = useMemo(() => {
    if (!modelTarget) return undefined;
    const labels = {
      whisper: "the selected Whisper model",
      whisper_language_detection: "Whisper Tiny language detection",
      parakeet: "Parakeet v3",
      separator: "the selected separator model",
      alignment: "the selected alignment model",
      pitch: "RMVPE pitch detection",
      open_vino_whisper: "OpenVINO Whisper",
    } as const;
    return labels[modelTarget];
  }, [modelTarget]);

  useEffect(() => {
    const backend = config?.compute_backend;
    if (backend === "cpu" || backend === "cuda" || backend === "intel") {
      setComputeBackend(backend);
    }
  }, [config?.compute_backend]);

  const startSetup = useCallback(() => {
    setSetupLogs([]);
    setSetupProgress((current) => ({
      ...current,
      step: "selectedmodels",
      percent: 1,
      action: modelLabel ? `Starting ${modelLabel} download…` : "Starting analysis setup…",
    }));
    return triggerSetup(
      undefined,
      config?.cache_paths ?? undefined,
      computeBackend,
      modelTarget,
    ).catch((error) => {
      setSetupProgress((current) => ({
        ...current,
        step: "error",
        percent: 0,
        action: error instanceof Error ? error.message : String(error),
      }));
    });
  }, [computeBackend, config?.cache_paths, modelLabel, modelTarget]);

  const invalidatePostSetupState = useCallback(async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: CONFIG }),
      queryClient.invalidateQueries({ queryKey: SONGS_META }),
      queryClient.invalidateQueries({ queryKey: SONGS }),
      queryClient.invalidateQueries({ queryKey: MENU }),
      queryClient.invalidateQueries({ queryKey: ANALYSIS_QUEUE }),
      queryClient.invalidateQueries({ queryKey: ANALYSIS_RUNTIME_STATUS }),
    ]);
  }, [queryClient]);

  useEffect(() => {
    let unlistenProgress: (() => void) | undefined;
    let unlistenError: (() => void) | undefined;
    let unlistenLog: (() => void) | undefined;

    onSetupProgress((progress) => {
      setSetupProgress(progress);
      if (progress.step === "finish") {
        void invalidatePostSetupState();
      }
    }).then((fn) => {
      unlistenProgress = fn;
    });

    onSetupError((error) => {
      setSetupProgress((current) => ({ ...current, step: "error", percent: 0, action: error }));
    }).then((fn) => {
      unlistenError = fn;
    });

    onSetupLog((line) => {
      setSetupLogs((current) => [...current, line].slice(-200));
    }).then((fn) => {
      unlistenLog = fn;
    });

    return () => {
      unlistenProgress?.();
      unlistenError?.();
      unlistenLog?.();
    };
  }, [invalidatePostSetupState]);

  const { step, percent, action } = setupProgress;
  const closeSetup = useCallback(() => {
    setSetupProgress(defaultProgress);
    setSetupLogs([]);
    setShouldRunSetup(false);
  }, [setShouldRunSetup]);

  useNavInput(
    useCallback(
      (navAction) => {
        if (!shouldRunSetup) {
          return;
        }

        if (navAction.back) {
          if (["init", "backend", "finish", "error"].includes(step)) {
            closeSetup();
          }

          return;
        }

        if (navAction.confirm) {
          if (step === "init") {
            if (modelTarget) void startSetup();
            else setSetupProgress((current) => ({ ...current, step: "backend" }));
          } else if (step === "backend") {
            startSetup();
          } else if (step === "finish") {
            void invalidatePostSetupState();
            setShouldRunSetup(false);
          } else if (step === "error") {
            closeSetup();
          }
        }
      },
      [
        closeSetup,
        invalidatePostSetupState,
        modelTarget,
        setShouldRunSetup,
        shouldRunSetup,
        startSetup,
        step,
      ],
    ),
  );

  const Step = useMemo(() => {
    switch (step) {
      case "init":
        return () => (
          <InitialStep
            onCancel={closeSetup}
            modelLabel={modelLabel}
            toNextStep={() =>
              modelTarget
                ? void startSetup()
                : setSetupProgress({ ...setupProgress, step: "backend" })
            }
          />
        );
      case "backend":
        return () => (
          <ComputeBackendStep
            value={computeBackend}
            onChange={setComputeBackend}
            onCancel={closeSetup}
            onContinue={startSetup}
          />
        );
      case "clearvendor":
      case "ffmpeg":
      case "preparefolders":
      case "uv":
      case "python":
      case "venv":
      case "dependencies":
      case "extractscripts":
      case "openvinowhisper":
      case "pitchmodel":
      case "selectedmodels":
        return () => (
          <LoadStep
            action={action}
            percent={percent}
            tasks={setupProgress.tasks}
            logs={setupLogs}
            detailsOpen={detailsOpen}
            onDetailsOpenChange={setDetailsOpen}
          />
        );
      case "finish":
        return () => (
          <FinalStep
            modelLabel={modelLabel}
            onFinish={() => {
              void invalidatePostSetupState();
              setSetupProgress(defaultProgress);
              setSetupLogs([]);
              setShouldRunSetup(false);
            }}
          />
        );
      case "error":
        return () => <ErrorStep error={action} onClose={closeSetup} />;
    }
  }, [
    step,
    action,
    percent,
    computeBackend,
    setupLogs,
    detailsOpen,
    startSetup,
    invalidatePostSetupState,
    setShouldRunSetup,
    closeSetup,
    modelLabel,
    modelTarget,
  ]);

  return (
    <AlertDialog
      open={shouldRunSetup}
      onOpenChange={(open) => {
        if (!open && ["init", "backend", "finish", "error"].includes(step)) {
          closeSetup();
        }
      }}
    >
      <AlertDialogContent
        className="glass-panel border-white/15 sm:max-w-xl"
        data-nav-passthrough
        onEscapeKeyDown={(event) => {
          if (!["init", "backend", "finish", "error"].includes(step)) {
            event.preventDefault();
          }
        }}
      >
        <img
          src={logoSrc}
          width={144}
          height={96}
          alt="Uta Studio"
          className="rounded-3xl object-contain shadow-2xl shadow-primary/20"
        />
        <Step />
      </AlertDialogContent>
    </AlertDialog>
  );
};
