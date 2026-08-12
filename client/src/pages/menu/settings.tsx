import { setFullScreen, isFullScreen as tauriIsFullScreen } from "@/bridge/fullScreen";
import { getRecentLogs } from "@/bridge/opener";
import { selectExportFolderPath } from "@/bridge/source";
import { runFeatureDiagnostics, type DiagnosticReport } from "@/bridge/diagnostics";
import {
  ALIGN_BACKENDS,
  ASR_ENGINES,
  COMPUTE_BACKENDS,
  DEFAULTS,
  MODELS,
  NAV,
  PITCH_MODELS,
  SEPARATORS,
  SETTINGS_TABS,
  VOCAL_THRESHOLD_MAX,
  getModelsNav,
  type SettingsTab,
} from "@/components/menu/settings/constants";
import {
  Hint,
  NumberButtonGroup,
  SettingsSelect,
} from "@/components/menu/settings/settings-controls";
import { modelSettingsVisibility } from "@/components/menu/settings/settings-logic";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Field, FieldGroup } from "@/components/ui/field";
import { Label } from "@/components/ui/label";
import { Slider } from "@/components/ui/slider";
import { Switch } from "@/components/ui/switch";
import { SegmentedProgress } from "@/components/ui/segmented-progress";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { useSettingsNavigation } from "@/hooks/navigation/use-settings-navigation";
import { useLibrarySourceActions } from "@/hooks/use-library-source-actions";
import { useDialog } from "@/hooks/use-dialog";
import { useShouldRunSetup } from "@/hooks/use-should-run-setup";
import { useConfigMutation } from "@/mutations/use-config-mutation";
import { useConfig } from "@/queries/use-config";
import { useCacheStats } from "@/queries/use-cache-stats";
import { useAnalysisRuntimeStatus } from "@/queries/use-analysis-runtime-status";
import { formatBytes, segmentPercent, totalUsedBytes } from "@/utils/stats";
import {
  Database,
  Box,
  FileText,
  FolderOpen,
  Monitor,
  RefreshCw,
  RotateCcw,
  Sparkles,
  Trash2,
  CheckCircle2,
  AlertCircle,
  LoaderCircle,
  ShieldCheck,
} from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import { useNavigate, useSearchParams } from "react-router";
import { toast } from "sonner";

const INTEL_OPENVINO_SEPARATORS = new Set(["openvino_demucs"]);

export const SettingsPage = () => {
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const { data: config } = useConfig();
  const { mutate } = useConfigMutation();
  const { openSetup } = useShouldRunSetup();
  const containerRef = useRef<HTMLDivElement>(null);
  const requestedTab = searchParams.get("tab");
  const [tab, setTab] = useState<SettingsTab>(
    requestedTab === "storage" || requestedTab === "models" || requestedTab === "analysis"
      ? requestedTab
      : "general",
  );
  const [isFullScreen, setIsFullScreen] = useState<boolean | null | undefined>(config?.fullscreen);
  const [vocalThresholdPct, setVocalThresholdPct] = useState(
    config?.vocal_detection_threshold_pct ?? DEFAULTS.vocal_detection_threshold_pct,
  );
  const [logOpen, setLogOpen] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [diagnosticOpen, setDiagnosticOpen] = useState(false);
  const [diagnosticRunning, setDiagnosticRunning] = useState(false);
  const [diagnosticReport, setDiagnosticReport] = useState<DiagnosticReport>();
  const source = useLibrarySourceActions();
  const { setMode } = useDialog();
  const { data: cacheStats, isError: cacheStatsError } = useCacheStats();
  const {
    data: runtimeStatus,
    isLoading: runtimeStatusLoading,
    refetch: refreshRuntimeStatus,
  } = useAnalysisRuntimeStatus();
  const cacheTotal = cacheStats ? totalUsedBytes(cacheStats) : 0n;

  const close = () => navigate("/");
  const asrEngine = config?.asr_engine ?? DEFAULTS.asr_engine;
  const isParakeet = asrEngine === "parakeet";
  const isIntelBackend = config?.compute_backend === "intel";
  const modelOptions = useMemo(() => MODELS.map((model) => ({ value: model, label: model })), []);
  const separatorOptions = useMemo(
    () =>
      isIntelBackend
        ? SEPARATORS
        : SEPARATORS.filter(({ value }) => !INTEL_OPENVINO_SEPARATORS.has(value)),
    [isIntelBackend],
  );
  const vocalThresholdDisplayPct = Math.round(vocalThresholdPct * 100);
  const batchSize = config?.batch_size ?? DEFAULTS.batch_size;
  const beamSize = config?.beam_size ?? DEFAULTS.beam_size;
  const modelNav = getModelsNav(isParakeet);
  const modelSettings = modelSettingsVisibility(asrEngine);
  const installableModels = runtimeStatus?.models ?? [];

  useEffect(() => {
    setVocalThresholdPct(
      config?.vocal_detection_threshold_pct ?? DEFAULTS.vocal_detection_threshold_pct,
    );
  }, [config?.vocal_detection_threshold_pct]);

  useEffect(() => {
    if (
      requestedTab === "general" ||
      requestedTab === "storage" ||
      requestedTab === "models" ||
      requestedTab === "analysis"
    ) {
      setTab(requestedTab);
    }
  }, [requestedTab]);

  useEffect(() => {
    void tauriIsFullScreen().then(setIsFullScreen);
  }, []);

  const updateComputeBackend = (compute_backend: string) => {
    const separator = config?.separator ?? DEFAULTS.separator;
    mutate({
      compute_backend,
      ...(compute_backend !== "intel" && INTEL_OPENVINO_SEPARATORS.has(separator)
        ? { separator: DEFAULTS.separator }
        : {}),
    });
  };

  const toggleWindowMode = (fullscreen: boolean) => {
    setIsFullScreen(fullscreen);
    void setFullScreen(fullscreen);
    mutate({ fullscreen });
  };

  const handleOpenLog = async () => {
    try {
      setLogs(await getRecentLogs());
      setLogOpen(true);
    } catch (error) {
      toast.error(`Could not open log: ${error instanceof Error ? error.message : String(error)}`);
    }
  };

  const handleDiagnostics = async () => {
    setDiagnosticOpen(true);
    setDiagnosticRunning(true);
    try {
      const report = await runFeatureDiagnostics({ includeExportSmoke: true });
      setDiagnosticReport(report);
      if (report.ok) toast.success("Feature diagnostics passed");
      else toast.error(`${report.failed} diagnostic check${report.failed === 1 ? "" : "s"} failed`);
    } catch (error) {
      toast.error("Diagnostics could not run", {
        description: error instanceof Error ? error.message : String(error),
      });
    } finally {
      setDiagnosticRunning(false);
    }
  };

  const updateVocalThreshold = (pct: number) => {
    setVocalThresholdPct(pct);
    mutate({ vocal_detection_threshold_pct: pct });
  };

  const resetDefaults = () => {
    mutate(DEFAULTS);
    setVocalThresholdPct(DEFAULTS.vocal_detection_threshold_pct);
    toast.success("Analysis defaults restored");
  };

  const chooseExportFolder = async () => {
    const export_path = await selectExportFolderPath();
    if (export_path) mutate({ export_path });
  };

  const { getFocusClassName, syncFocusFromElement } = useSettingsNavigation({
    containerRef,
    tab,
    isParakeet,
    vocalThresholdPct,
    folderCount: source.paths.length,
    modelCount: installableModels.length,
    onBack: close,
    onTabChange: setTab,
    onVocalThresholdChange: updateVocalThreshold,
  });

  return (
    <main
      ref={containerRef}
      className="roon-settings-page bg-editor-ambient h-full w-full overflow-hidden"
      onFocusCapture={(event) => syncFocusFromElement(event.target)}
    >
      <Tabs
        value={tab}
        orientation="vertical"
        className="flex h-full min-h-0 flex-col gap-0 lg:flex-row"
        onValueChange={(value) => setTab(value as SettingsTab)}
      >
        <TabsList
          variant="line"
          aria-label="Settings sections"
          className="roon-settings-nav scrollbar-hide h-auto w-full shrink-0 justify-start gap-1 overflow-x-auto rounded-none border-b border-border/55 px-3 py-3 lg:w-56 lg:flex-col lg:items-stretch lg:justify-start lg:overflow-y-auto lg:border-b-0 lg:border-r lg:px-6 lg:py-8"
        >
          <div className="hidden px-3 pb-6 lg:block">
            <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
              Uta Studio
            </p>
            <h1 className="mt-1.5 text-xl font-light tracking-tight">Settings</h1>
            <p className="mt-1.5 text-[10px] leading-relaxed text-muted-foreground">
              Workspace, library, and generation.
            </p>
          </div>
          {SETTINGS_TABS.map((settingsTab, slot) => (
            <TabsTrigger
              key={settingsTab.value}
              value={settingsTab.value}
              className={`min-h-9 flex-none justify-start rounded-none border-0 px-3 after:hidden hover:bg-transparent hover:text-foreground/72 focus-visible:border-transparent focus-visible:ring-0 data-active:bg-transparent data-active:text-foreground/48 data-active:before:absolute data-active:before:inset-y-2 data-active:before:left-0 data-active:before:w-px data-active:before:bg-primary lg:w-full ${getFocusClassName(NAV.tabSegment, slot)}`}
              onKeyDown={(event) => {
                if (event.key === "Enter") setTab(settingsTab.value);
              }}
            >
              {settingsTab.value === "general" ? (
                <Monitor />
              ) : settingsTab.value === "storage" ? (
                <Database />
              ) : settingsTab.value === "models" ? (
                <Box />
              ) : (
                <Sparkles />
              )}
              {settingsTab.label}
            </TabsTrigger>
          ))}
        </TabsList>

        <div className="roon-settings-content flex min-h-0 min-w-0 flex-1 flex-col">
          <div className="min-h-0 flex-1 overflow-y-auto">
            <TabsContent
              value="general"
              className="m-0 mx-auto max-w-5xl px-4 py-6 sm:px-7 lg:px-10 lg:py-9"
            >
              <div className="mb-6 border-b border-border/55 pb-5">
                <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                  Workspace
                </p>
                <h2 className="mt-1 text-xl font-light">General</h2>
                <p className="mt-1 text-[10px] text-muted-foreground">
                  Window behavior and diagnostic tools.
                </p>
              </div>
              <FieldGroup className="roon-settings-fields gap-0">
                <Field>
                  <div className="min-w-0">
                    <Label>Fullscreen workspace</Label>
                    <Hint>
                      {isFullScreen
                        ? "The editor fills this display."
                        : "The app uses a standard window."}
                    </Hint>
                  </div>
                  <Switch
                    checked={Boolean(isFullScreen)}
                    onCheckedChange={toggleWindowMode}
                    className={`!w-9 justify-self-end ${getFocusClassName(NAV.general.window)}`}
                    aria-label="Fullscreen"
                  />
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label>Application log</Label>
                    <Hint>
                      Review recent events when analysis, editing, or export needs troubleshooting.
                    </Hint>
                  </div>
                  <Button
                    variant="outline"
                    className={getFocusClassName(NAV.general.diagnostics)}
                    onClick={() => void handleOpenLog()}
                  >
                    <FileText /> View log
                  </Button>
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label>Feature API diagnostics</Label>
                    <Hint>
                      Verify local APIs, editor audio, and real UTZ/UltraStar exports in a temporary
                      folder. Your library and model files are not changed.
                    </Hint>
                  </div>
                  <Button
                    variant="outline"
                    className={getFocusClassName(NAV.general.api)}
                    disabled={diagnosticRunning}
                    onClick={() => void handleDiagnostics()}
                  >
                    {diagnosticRunning ? (
                      <LoaderCircle className="animate-spin" />
                    ) : (
                      <ShieldCheck />
                    )}
                    {diagnosticRunning ? "Testing…" : "Run checks"}
                  </Button>
                </Field>
              </FieldGroup>
            </TabsContent>

            <TabsContent
              value="storage"
              className="m-0 mx-auto max-w-5xl px-4 py-6 sm:px-7 lg:px-10 lg:py-9"
            >
              <div className="mb-6 border-b border-border/55 pb-5">
                <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                  Library
                </p>
                <h2 className="mt-1 text-xl font-light">Storage</h2>
                <p className="mt-1 text-[10px] text-muted-foreground">
                  Manage watched folders and generated data. Your source media is never moved or
                  deleted.
                </p>
              </div>
              <FieldGroup className="roon-settings-fields gap-0">
                <Field className="items-start">
                  <div className="min-w-0 flex-1">
                    <Label>Watched folders</Label>
                    <Hint>
                      Add as many music locations as you need. Folder changes are merged into one
                      library.
                    </Hint>
                    <div className="mt-3 space-y-1">
                      {source.paths.length === 0 ? (
                        <div className="rounded-sm border border-dashed border-border/55 px-3 py-4 text-[10px] text-muted-foreground">
                          No local folders connected.
                        </div>
                      ) : (
                        source.paths.map((path) => (
                          <div
                            key={path}
                            className="group flex min-w-0 items-center gap-2 rounded-sm bg-foreground/[0.025] px-2.5 py-2"
                          >
                            <FolderOpen className="size-4 shrink-0 text-muted-foreground" />
                            <span className="min-w-0 flex-1 truncate text-[10px]" title={path}>
                              {path}
                            </span>
                            <Button
                              variant="ghost"
                              size="icon-sm"
                              className="opacity-45 hover:opacity-100"
                              disabled={source.isPending}
                              aria-label={`Remove ${path}`}
                              onClick={() => {
                                if (
                                  window.confirm(
                                    "Stop watching this folder? Your media will not be deleted.",
                                  )
                                )
                                  source.removeFolder(path);
                              }}
                            >
                              <Trash2 />
                            </Button>
                          </div>
                        ))
                      )}
                    </div>
                  </div>
                  <div className="flex flex-wrap justify-start gap-2 lg:justify-end">
                    <Button disabled={source.isPending} onClick={source.selectFolder}>
                      <FolderOpen /> Add folder…
                    </Button>
                    <Button
                      variant="outline"
                      disabled={source.rescanDisabled}
                      onClick={source.rescan}
                    >
                      <RefreshCw /> Rescan all
                    </Button>
                  </div>
                </Field>

                <Field className="items-start">
                  <div className="min-w-0 flex-1">
                    <Label>Default export folder</Label>
                    <Hint>
                      Every format opens Save As here first. You can still choose another folder for
                      each export.
                    </Hint>
                    <p
                      className="mt-2 max-w-xl truncate text-[9px] text-muted-foreground"
                      title={config?.export_path ?? undefined}
                    >
                      {config?.export_path ?? "Use the last folder chosen by the system dialog"}
                    </p>
                  </div>
                  <div className="flex flex-wrap justify-start gap-2 lg:justify-end">
                    <Button variant="outline" onClick={() => void chooseExportFolder()}>
                      <FolderOpen /> Choose…
                    </Button>
                    <Button
                      variant="ghost"
                      disabled={!config?.export_path}
                      onClick={() => mutate({ export_path: null })}
                    >
                      Use system default
                    </Button>
                  </div>
                </Field>

                <Field className="items-start">
                  <div className="min-w-0 flex-1">
                    <Label>Generated storage</Label>
                    <Hint>
                      Stems, editable charts, playback previews, AI models, and temporary authoring
                      files.
                    </Hint>
                    <div className="mt-4 max-w-xl">
                      <div className="mb-2 flex items-end justify-between gap-3">
                        <span className="text-[10px] text-muted-foreground">Current cache use</span>
                        <span className="text-sm font-light tabular-nums">
                          {cacheStatsError
                            ? "Unavailable"
                            : cacheStats
                              ? formatBytes(cacheTotal)
                              : "…"}
                        </span>
                      </div>
                      <SegmentedProgress
                        className="h-1.5 bg-foreground/[0.06]"
                        segments={
                          cacheStats
                            ? [
                                {
                                  value: segmentPercent(cacheStats.songs_bytes, cacheTotal),
                                  color: "bg-primary/75",
                                },
                                {
                                  value: segmentPercent(cacheStats.models_bytes, cacheTotal),
                                  color: "bg-amber-500/70",
                                },
                                {
                                  value: segmentPercent(cacheStats.other_bytes, cacheTotal),
                                  color: "bg-foreground/30",
                                },
                              ]
                            : []
                        }
                      />
                      {cacheStats ? (
                        <div className="mt-3 grid grid-cols-3 gap-3 text-[9px] text-muted-foreground">
                          <span>
                            <i className="mr-1.5 inline-block size-1.5 rounded-full bg-primary/75" />
                            Songs {formatBytes(cacheStats.songs_bytes)}
                          </span>
                          <span>
                            <i className="mr-1.5 inline-block size-1.5 rounded-full bg-amber-500/70" />
                            Models {formatBytes(cacheStats.models_bytes)}
                          </span>
                          <span>
                            <i className="mr-1.5 inline-block size-1.5 rounded-full bg-foreground/30" />
                            Other {formatBytes(cacheStats.other_bytes)}
                          </span>
                        </div>
                      ) : null}
                    </div>
                  </div>
                  <div className="flex flex-wrap justify-start gap-2 lg:justify-end">
                    <Button
                      variant="outline"
                      onClick={() => setMode({ mode: "clear-cache", target: "all" })}
                    >
                      <Trash2 /> Clear generated cache
                    </Button>
                    <Button
                      variant="ghost"
                      onClick={() => setMode({ mode: "clear-cache", target: "models" })}
                    >
                      <Box /> Clear models
                    </Button>
                  </div>
                </Field>
              </FieldGroup>
            </TabsContent>

            <TabsContent
              value="models"
              className="m-0 mx-auto max-w-5xl px-4 py-6 sm:px-7 lg:px-10 lg:py-9"
            >
              <div className="mb-6 border-b border-border/55 pb-5">
                <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                  Local intelligence
                </p>
                <h2 className="mt-1 text-xl font-light">Models &amp; runtime</h2>
                <p className="mt-1 max-w-3xl text-[10px] leading-relaxed text-muted-foreground">
                  Choose what should run on this machine. Checks are read-only; downloads start only
                  after you confirm Setup.
                </p>
              </div>

              <FieldGroup className="roon-settings-fields gap-0">
                <Field className="items-start">
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      {runtimeStatus?.ready ? (
                        <CheckCircle2 className="size-4 text-emerald-500" />
                      ) : (
                        <AlertCircle className="size-4 text-amber-500" />
                      )}
                      <Label>{runtimeStatus?.ready ? "Ready to analyze" : "Setup required"}</Label>
                    </div>
                    <Hint>
                      {runtimeStatusLoading
                        ? "Checking installed tools and model files…"
                        : runtimeStatus?.ready
                          ? "The selected runtime and every required model are available locally."
                          : `Missing: ${runtimeStatus?.missing.join(" · ") || "runtime status unavailable"}`}
                    </Hint>
                    {runtimeStatus ? (
                      <div className="mt-3 grid max-w-xl grid-cols-2 gap-x-5 gap-y-2 text-[9px] text-muted-foreground sm:grid-cols-3">
                        <span>
                          ffmpeg · {runtimeStatus.ffmpegAvailable ? "available" : "missing"}
                        </span>
                        <span>uv · {runtimeStatus.uvAvailable ? "available" : "missing"}</span>
                        <span>
                          Python · {runtimeStatus.systemPythonAvailable ? "available" : "missing"}
                        </span>
                        <span>
                          Analyzer · {runtimeStatus.analyzerAvailable ? "installed" : "missing"}
                        </span>
                        <span>
                          Pitch · {runtimeStatus.pitchModelAvailable ? "installed" : "missing"}
                        </span>
                        <span>
                          Models · {runtimeStatus.selectedModelsAvailable ? "installed" : "missing"}
                        </span>
                      </div>
                    ) : null}
                  </div>
                  <Button
                    variant="ghost"
                    className={getFocusClassName(modelNav.runtime)}
                    onClick={() => void refreshRuntimeStatus()}
                  >
                    <RefreshCw /> Check again
                  </Button>
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label htmlFor="model-runtime">Acceleration</Label>
                    <Hint>
                      Choose the hardware target before installing the analysis environment.
                    </Hint>
                  </div>
                  <SettingsSelect
                    id="model-runtime"
                    label="Acceleration"
                    placeholder="Select a runtime"
                    value={config?.compute_backend ?? DEFAULTS.compute_backend}
                    options={COMPUTE_BACKENDS}
                    triggerClassName={getFocusClassName(modelNav.computeBackend)}
                    onValueChange={updateComputeBackend}
                  />
                </Field>

                <Field className="items-start">
                  <div className="min-w-0 flex-1">
                    <Label>Shared analysis runtime</Label>
                    <Hint>
                      Setup reuses compatible system ffmpeg, uv, Python, and existing model files.
                      Use this for initial setup or after changing acceleration. Individual model
                      downloads are listed separately below.
                    </Hint>
                    {cacheStats ? (
                      <p className="mt-1 text-[9px] text-muted-foreground">
                        Model storage · {formatBytes(cacheStats.models_bytes)}
                      </p>
                    ) : null}
                  </div>
                  <Button className={getFocusClassName(modelNav.setup)} onClick={() => openSetup()}>
                    <Box /> {runtimeStatus?.managedRuntimeAvailable ? "Reconfigure…" : "Set up…"}
                  </Button>
                </Field>

                <div className="px-5 pb-1 pt-5">
                  <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                    Selected model files
                  </p>
                  <p className="mt-1 text-[9px] leading-relaxed text-muted-foreground">
                    Each button downloads only that selected model. A missing shared runtime is
                    prepared first, after your confirmation.
                  </p>
                </div>

                {installableModels.length > 0 ? (
                  installableModels.map((model, index) => (
                    <Field key={model.target}>
                      <div className="min-w-0">
                        <div className="flex items-center gap-2">
                          {model.available ? (
                            <CheckCircle2 className="size-3.5 text-emerald-500" />
                          ) : (
                            <AlertCircle className="size-3.5 text-amber-500" />
                          )}
                          <Label>{model.label}</Label>
                        </div>
                        <Hint>{model.description}</Hint>
                      </div>
                      <Button
                        variant="outline"
                        className={getFocusClassName(modelNav.downloadStart + index)}
                        disabled={model.available}
                        onClick={() => openSetup({ modelTarget: model.target })}
                      >
                        {model.available ? <CheckCircle2 /> : <Box />}
                        {model.available ? "Installed" : "Download…"}
                      </Button>
                    </Field>
                  ))
                ) : (
                  <Field>
                    <div className="min-w-0">
                      <Label>Model status unavailable</Label>
                      <Hint>
                        Check the runtime status again to refresh the selected model list.
                      </Hint>
                    </div>
                  </Field>
                )}
              </FieldGroup>
            </TabsContent>

            <TabsContent
              value="analysis"
              className="m-0 mx-auto max-w-5xl px-4 py-6 sm:px-7 lg:px-10 lg:py-9"
            >
              <div className="mb-6 border-b border-border/55 pb-5">
                <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                  Generation
                </p>
                <h2 className="mt-1 text-xl font-light">Analysis</h2>
                <p className="mt-1 max-w-3xl text-[10px] leading-relaxed text-muted-foreground">
                  Controls for newly generated stems, lyric alignment, and pitch analysis. Existing
                  charts change only after re-analysis.
                </p>
              </div>

              <FieldGroup className="roon-settings-fields gap-0">
                <Field>
                  <div className="min-w-0">
                    <Label htmlFor="separator-1">Vocal separator</Label>
                    <Hint>How vocals are separated from the instrumental.</Hint>
                  </div>
                  <SettingsSelect
                    id="separator-1"
                    label="Separator"
                    placeholder="Select a separator"
                    value={config?.separator ?? DEFAULTS.separator}
                    options={separatorOptions}
                    triggerClassName={getFocusClassName(NAV.analysis.separator)}
                    onValueChange={(separator) => mutate({ separator })}
                  />
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label htmlFor="analysis-engine">Transcription family</Label>
                    <Hint>
                      Chooses how new lyrics are recognized. Changing it may require downloading its
                      selected model from Models &amp; runtime.
                    </Hint>
                  </div>
                  <SettingsSelect
                    id="analysis-engine"
                    label="Transcription family"
                    placeholder="Select an engine"
                    value={asrEngine}
                    options={ASR_ENGINES}
                    triggerClassName={getFocusClassName(NAV.analysis.asrEngine)}
                    onValueChange={(asr_engine) => mutate({ asr_engine })}
                  />
                </Field>

                <div className="px-5 pb-1 pt-5">
                  <p className="text-[8px] font-medium uppercase tracking-[0.18em] text-primary">
                    {isParakeet ? "Parakeet v3 and compatibility fallback" : "Whisper analysis"}
                  </p>
                  <p className="mt-1 text-[9px] leading-relaxed text-muted-foreground">
                    {isParakeet
                      ? "Parakeet handles supported languages directly. Whisper remains the compatibility fallback for unsupported languages or empty results."
                      : "These parameters shape newly generated transcription and word timing."}
                  </p>
                </div>

                {modelSettings.whisperModel ? (
                  <>
                    <Field>
                      <div className="min-w-0">
                        <Label htmlFor="analysis-model-size">
                          {isParakeet ? "Whisper fallback model" : "Whisper model"}
                        </Label>
                        <Hint>
                          {isParakeet
                            ? "Used only when Parakeet cannot handle the language or returns no words."
                            : "Turbo is the balanced default; larger models trade speed for detail."}
                        </Hint>
                      </div>
                      <SettingsSelect
                        id="analysis-model-size"
                        label="Whisper model"
                        placeholder="Select a model"
                        value={config?.whisper_model ?? DEFAULTS.whisper_model}
                        options={modelOptions}
                        triggerClassName={getFocusClassName(NAV.analysis.whisperModel)}
                        onValueChange={(whisper_model) => mutate({ whisper_model })}
                      />
                    </Field>
                    <Field>
                      <div className="min-w-0">
                        <Label>
                          {isParakeet ? "Whisper fallback precision" : "Recognition precision"}
                        </Label>
                        <Hint>
                          Whisper search breadth. 8 is the balanced starting point; higher values
                          improve difficult phrases at the cost of speed.
                        </Hint>
                      </div>
                      <NumberButtonGroup
                        name="beam_size"
                        value={beamSize}
                        segment={NAV.analysis.beamSize}
                        getFocusClassName={getFocusClassName}
                        onChange={(beam_size) => mutate({ beam_size })}
                      />
                    </Field>
                  </>
                ) : null}

                <Field>
                  <div className="min-w-0">
                    <Label>{isParakeet ? "Parakeet batch size" : "Whisper batch size"}</Label>
                    <Hint>
                      Performance tuning only: lower it if analysis runs out of GPU or system
                      memory.
                    </Hint>
                  </div>
                  <NumberButtonGroup
                    name="batch_size"
                    value={batchSize}
                    segment={NAV.analysis.batchSize}
                    getFocusClassName={getFocusClassName}
                    onChange={(batch_size) => mutate({ batch_size })}
                  />
                </Field>

                {modelSettings.wordAlignment ? (
                  <Field>
                    <div className="min-w-0">
                      <Label htmlFor="analysis-align-backend">
                        {isParakeet ? "Whisper fallback alignment" : "Word alignment"}
                      </Label>
                      <Hint>
                        Refines Whisper output into editable word timings. Parakeet's direct output
                        skips this step.
                      </Hint>
                    </div>
                    <SettingsSelect
                      id="analysis-align-backend"
                      label="Whisper word alignment"
                      placeholder="Select an alignment model"
                      value={config?.align_backend ?? DEFAULTS.align_backend}
                      options={ALIGN_BACKENDS}
                      triggerClassName={getFocusClassName(NAV.analysis.alignBackend)}
                      onValueChange={(align_backend) => mutate({ align_backend })}
                    />
                  </Field>
                ) : null}

                {modelSettings.pitchModel ? (
                  <Field>
                    <div className="min-w-0">
                      <Label htmlFor="analysis-pitch-model">Frequency analysis model</Label>
                      <Hint>
                        Detects the sung fundamental frequency for editable pitch notes. RMVPE is
                        currently the supported model.
                      </Hint>
                    </div>
                    <SettingsSelect
                      id="analysis-pitch-model"
                      label="Frequency analysis model"
                      placeholder="Select a pitch model"
                      value={config?.pitch_model ?? DEFAULTS.pitch_model}
                      options={PITCH_MODELS}
                      triggerClassName={getFocusClassName(NAV.analysis.pitchModel)}
                      onValueChange={(pitch_model) => mutate({ pitch_model })}
                    />
                  </Field>
                ) : null}

                <Field>
                  <div className="min-w-0">
                    <Label>Auto-analyze</Label>
                    <Hint>Automatically queue unanalyzed songs after a library scan.</Hint>
                  </div>
                  <Switch
                    checked={config?.auto_analyze === true}
                    onCheckedChange={(auto_analyze) => mutate({ auto_analyze })}
                    className={`!w-9 justify-self-end ${getFocusClassName(NAV.analysis.autoAnalyze)}`}
                    aria-label="Auto-analyze"
                  />
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label>Vocal detection sensitivity</Label>
                    <Hint>
                      Lower for soft singing; raise to remove more silence (
                      {vocalThresholdDisplayPct}% of peak).
                    </Hint>
                  </div>
                  <Slider
                    min={0}
                    max={Math.round(VOCAL_THRESHOLD_MAX * 100)}
                    step={1}
                    value={[vocalThresholdDisplayPct]}
                    onValueChange={([pct]) => updateVocalThreshold(pct / 100)}
                    className={getFocusClassName(NAV.analysis.vocalThreshold)}
                  />
                </Field>

                <Field>
                  <div className="min-w-0">
                    <Label>Analysis defaults</Label>
                    <Hint>Restore all generation controls to the recommended starting values.</Hint>
                  </div>
                  <Button
                    variant="ghost"
                    className={getFocusClassName(NAV.analysis.restore)}
                    onClick={resetDefaults}
                  >
                    <RotateCcw /> Restore defaults
                  </Button>
                </Field>
              </FieldGroup>
            </TabsContent>
          </div>
        </div>
      </Tabs>

      <Dialog open={logOpen} onOpenChange={setLogOpen}>
        <DialogContent className="glass-panel max-w-4xl">
          <DialogHeader>
            <DialogTitle>Application log</DialogTitle>
            <DialogDescription>Recent entries from this session.</DialogDescription>
          </DialogHeader>
          <pre className="max-h-[65vh] overflow-auto rounded-xl bg-background/60 p-3 text-[11px] leading-relaxed whitespace-pre-wrap">
            {logs.length > 0 ? logs.join("\n") : "No log entries captured yet."}
          </pre>
          <DialogFooter>
            <Button variant="outline" onClick={() => void handleOpenLog()}>
              Refresh
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      <Dialog open={diagnosticOpen} onOpenChange={setDiagnosticOpen}>
        <DialogContent className="glass-panel max-w-3xl">
          <DialogHeader>
            <DialogTitle>Feature API diagnostics</DialogTitle>
            <DialogDescription>
              {diagnosticRunning
                ? "Checking local feature boundaries and temporary exports…"
                : diagnosticReport
                  ? `${diagnosticReport.capabilities} API endpoints · ${diagnosticReport.passed} passed · ${diagnosticReport.failed} failed · ${diagnosticReport.skipped} skipped`
                  : "No diagnostic report has been generated yet."}
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[62vh] space-y-1 overflow-y-auto pr-1">
            {diagnosticRunning && !diagnosticReport ? (
              <div className="flex items-center gap-2 rounded-md bg-foreground/[0.025] px-3 py-6 text-[11px] text-muted-foreground">
                <LoaderCircle className="size-4 animate-spin" /> Running safe checks…
              </div>
            ) : (
              diagnosticReport?.checks.map((check) => (
                <div
                  key={check.id}
                  className="grid grid-cols-[auto_minmax(0,1fr)_auto] items-start gap-2 rounded-sm border-b border-border/35 px-2 py-2.5 last:border-0"
                >
                  {check.status === "passed" ? (
                    <CheckCircle2 className="mt-0.5 size-3.5 text-emerald-500" />
                  ) : check.status === "failed" ? (
                    <AlertCircle className="mt-0.5 size-3.5 text-destructive" />
                  ) : (
                    <span className="mt-1 block size-2 rounded-full bg-muted-foreground/35" />
                  )}
                  <div className="min-w-0">
                    <p className="text-[10px] font-medium">{check.id}</p>
                    <p className="mt-0.5 break-words text-[9px] leading-relaxed text-muted-foreground">
                      {check.detail}
                    </p>
                  </div>
                  <span className="text-[8px] tabular-nums text-muted-foreground/65">
                    {check.elapsedMs > 0 ? `${check.elapsedMs} ms` : "—"}
                  </span>
                </div>
              ))
            )}
          </div>
          <DialogFooter>
            <Button
              variant="outline"
              disabled={diagnosticRunning}
              onClick={() => void handleDiagnostics()}
            >
              {diagnosticRunning ? "Testing…" : "Run again"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </main>
  );
};
