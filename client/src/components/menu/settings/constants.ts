import type { AppConfig } from "@/types/AppConfig";

export type SettingsTab = "general" | "storage" | "models" | "analysis";
export type SettingsOption = { value: string; label: string; description?: string };

export const SETTINGS_TABS: { value: SettingsTab; label: string }[] = [
  { value: "general", label: "General" },
  { value: "storage", label: "Storage" },
  { value: "models", label: "Models & runtime" },
  { value: "analysis", label: "Analysis" },
];

export const SEPARATORS: SettingsOption[] = [
  {
    value: "karaoke",
    label: "UVR Karaoke",
    description:
      "Usually separates more cleanly, but can occasionally slip on tricky parts. NVIDIA CUDA is accelerated; Intel Arc on Linux currently runs this model on CPU.",
  },
  {
    value: "demucs",
    label: "Demucs",
    description:
      "Smoother and more consistent with fewer abrupt artifacts, though slightly less crisp overall.",
  },
  {
    value: "openvino_demucs",
    label: "OpenVINO Demucs v4 (Intel GPU)",
    description:
      "Intel's GPU-ported Demucs v4. Produces reliable vocal and instrumental stems on Intel Arc.",
  },
];

export const ASR_ENGINES: SettingsOption[] = [
  {
    value: "whisper",
    label: "Whisper",
    description: "Works in any language and lets you pick a model size below.",
  },
  {
    value: "parakeet",
    label: "Parakeet v3 (Experimental)",
    description:
      "Much faster and produces its own word timings (skipping alignment), but only covers 25 European languages. Whisper takes over for anything else.",
  },
];

export const ALIGN_BACKENDS: SettingsOption[] = [
  {
    value: "whisperx",
    label: "WhisperX",
    description: "The reliable default, timing words with a proven decoder.",
  },
  {
    value: "ctc",
    label: "CTC Forced Alignment (Experimental)",
    description:
      "Calculates word start/end points with a different algorithm, and runs much faster on GPU and Apple Silicon. Falls back to WhisperX if a line trips it up.",
  },
  {
    value: "qwen",
    label: "Qwen Forced Alignment (Experimental)",
    description:
      "A fast AI model covering 11 languages. Timing quality varies song to song, but it can do better on Chinese, Japanese, and Korean. Falls back to WhisperX otherwise.",
  },
];

export const PITCH_MODELS: SettingsOption[] = [
  {
    value: "rmvpe",
    label: "RMVPE",
    description:
      "Detects the sung fundamental frequency and turns it into the editable pitch guide.",
  },
];

export const MODELS = ["large-v3", "large-v3-turbo", "medium", "small", "base", "tiny"];

export const COMPUTE_BACKENDS: SettingsOption[] = [
  {
    value: "cpu",
    label: "CPU",
    description: "Most compatible; no graphics runtime required.",
  },
  {
    value: "cuda",
    label: "NVIDIA CUDA",
    description: "For NVIDIA GPUs with a working CUDA driver.",
  },
  {
    value: "intel",
    label: "Intel Arc",
    description: "Uses Intel XPU/OpenVINO where the installed model supports it.",
  },
];

export const DEFAULTS = {
  separator: "karaoke",
  asr_engine: "whisper",
  align_backend: "whisperx",
  pitch_model: "rmvpe",
  vocal_detection_threshold_pct: 0.15,
  whisper_model: "large-v3",
  beam_size: 8,
  batch_size: 8,
  compute_backend: "cpu",
  auto_analyze: false,
} satisfies Pick<
  AppConfig,
  | "separator"
  | "asr_engine"
  | "align_backend"
  | "pitch_model"
  | "vocal_detection_threshold_pct"
  | "whisper_model"
  | "beam_size"
  | "batch_size"
  | "compute_backend"
  | "auto_analyze"
>;

// Vocal-detection threshold is stored as a fraction of peak RMS (0-1) but shown
// as a percentage. Capped at 60% since anything higher trims almost everything.
export const VOCAL_THRESHOLD_STEP = 0.01;
export const VOCAL_THRESHOLD_MIN = 0;
export const VOCAL_THRESHOLD_MAX = 0.6;
export const NUMBER_PICKER_SIZE = 3;

export const NAV = {
  tabSegment: 0,
  general: {
    window: 1,
    diagnostics: 2,
    api: 3,
  },
  models: {
    runtime: 1,
    computeBackend: 2,
    setup: 3,
    downloadStart: 4,
  },
  analysis: {
    separator: 1,
    asrEngine: 2,
    whisperModel: 3,
    beamSize: 4,
    batchSize: 5,
    alignBackend: 6,
    pitchModel: 7,
    autoAnalyze: 8,
    vocalThreshold: 9,
    restore: 10,
  },
} as const;

export function getModelsNav(_isParakeet: boolean) {
  return NAV.models;
}

export function getSettingsStops(
  tab: SettingsTab,
  _isParakeet: boolean,
  folderCount = 0,
  modelCount = 0,
) {
  if (tab === "general") {
    return [4, 1, 1, 1];
  }

  if (tab === "storage") {
    return [4, folderCount + 2, 2, 2];
  }

  if (tab === "models") {
    return [4, 1, 1, 1, ...Array.from({ length: modelCount }, () => 1)];
  }

  return [4, 1, 1, 1, NUMBER_PICKER_SIZE, NUMBER_PICKER_SIZE, 1, 1, 1, 1, 1];
}
