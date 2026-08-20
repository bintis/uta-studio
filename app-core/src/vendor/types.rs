use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use tracing::info;
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum SetupStep {
    PrepareFolders,
    ClearVendor,
    Ffmpeg,
    Uv,
    Python,
    Venv,
    Dependencies,
    ExtractScripts,
    OpenVinoWhisper,
    PitchModel,
    SelectedModels,
    Finish,
}

/// The compute runtime selected before installing the Python environment.
/// It is deliberately explicit: silently picking CUDA on a mixed-GPU system
/// can download several GB of the wrong runtime.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum ComputeBackend {
    #[default]
    Cpu,
    Cuda,
    Intel,
}

/// A concrete, user-visible model family that can be prepared independently.
/// Configurable families read their selected variant from `AppConfig` when the
/// job starts. Explicit optional families use their own variant so their model
/// row can remain available without silently changing the active analysis setup.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Hash)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadTarget {
    Whisper,
    WhisperLanguageDetection,
    Parakeet,
    Separator,
    Alignment,
    MmsKaraokeAlignment,
    Pitch,
    OpenVinoWhisper,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallStatus {
    pub target: ModelDownloadTarget,
    pub label: String,
    pub description: String,
    pub available: bool,
}

impl ComputeBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Cpu => "cpu",
            Self::Cuda => "cuda",
            Self::Intel => "intel",
        }
    }
}

/// Return the normalized distribution name separately from the versioned
/// requirement. `uv --reinstall-package` accepts only the former, while the
/// positional install argument should retain the latter.
pub(crate) fn onnx_runtime_package(backend: ComputeBackend) -> (&'static str, &'static str) {
    match backend {
        ComputeBackend::Cpu => ("onnxruntime", "onnxruntime>=1.17"),
        ComputeBackend::Cuda => ("onnxruntime-gpu", "onnxruntime-gpu>=1.17"),
        ComputeBackend::Intel => ("onnxruntime-openvino", "onnxruntime-openvino>=1.17"),
    }
}

pub(crate) fn inference_runtime_reinstall_args(backend: ComputeBackend, python: &str) -> Vec<&str> {
    let (package_name, package_requirement) = onnx_runtime_package(backend);
    vec![
        "pip",
        "install",
        "--reinstall-package",
        package_name,
        package_requirement,
        "--python",
        python,
    ]
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "lowercase")]
pub enum SetupTaskState {
    Pending,
    Running,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupTask {
    pub step: SetupStep,
    pub label: String,
    pub state: SetupTaskState,
    /// Bytes received for direct downloads, or bytes currently installed in
    /// the virtualenv while a package job is running. `None` means that the
    /// upstream tool did not report a meaningful figure.
    pub downloaded_bytes: Option<u64>,
    /// The server's Content-Length where available. Package resolvers usually
    /// do not expose a reliable total, so this is intentionally optional.
    pub total_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct SetupProgress {
    pub step: SetupStep,
    pub percent: usize,
    pub action: String,
    pub tasks: Vec<SetupTask>,
}

/// A read-only snapshot used by the UI to explain why analysis is available
/// or unavailable. Merely loading this value never downloads or changes
/// anything on disk.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRuntimeStatus {
    pub ready: bool,
    /// True when the on-disk runtime contract matches the current app version.
    /// A stale but still-usable v4 environment stays `ready` and sets this false.
    #[serde(default)]
    pub runtime_contract_current: bool,
    pub ffmpeg_available: bool,
    pub uv_available: bool,
    pub system_python_available: bool,
    pub managed_runtime_available: bool,
    pub analyzer_available: bool,
    pub pitch_model_available: bool,
    pub backend_models_available: bool,
    pub selected_models_available: bool,
    pub selected_models: Vec<String>,
    pub models: Vec<ModelInstallStatus>,
    pub compute_backend: String,
    pub ffmpeg_path: Option<String>,
    pub uv_path: Option<String>,
    pub system_python_path: Option<String>,
    pub missing: Vec<String>,
}
