import { invoke, listen } from "./runtime";
import type { CachePaths } from "@/types/CachePaths";
import type { SetupProgress } from "@/types/SetupProgress";
import type { ComputeBackend } from "@/types/ComputeBackend";
import type { AnalysisRuntimeStatus } from "@/types/AnalysisRuntimeStatus";
import type { ModelDownloadTarget } from "@/types/ModelDownloadTarget";

export const loadAnalysisRuntimeStatus = async (): Promise<AnalysisRuntimeStatus> => {
  return await invoke<AnalysisRuntimeStatus>("analysis_runtime_status");
};

export const triggerSetup = async (
  dataFolder?: string,
  cachePaths?: CachePaths,
  computeBackend: ComputeBackend = "cpu",
  modelTarget?: ModelDownloadTarget,
): Promise<void> => {
  return await invoke<void>("trigger_setup", {
    dataPath: dataFolder,
    cachePaths,
    computeBackend,
    modelTarget,
  });
};

export const onSetupProgress = async (
  cb: (progress: SetupProgress) => void,
): Promise<() => void> => {
  return await listen<SetupProgress>("setup-progress", ({ payload }) => cb(payload));
};

export const onSetupError = async (cb: (error: string) => void): Promise<() => void> => {
  return await listen<string>("setup-error", ({ payload }) => cb(payload));
};

export const onSetupLog = async (cb: (line: string) => void): Promise<() => void> => {
  return await listen<string>("setup-log", ({ payload }) => cb(payload));
};
