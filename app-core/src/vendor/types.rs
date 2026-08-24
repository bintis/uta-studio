use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    PrepareFolders,
    Ffmpeg,
    NativeComponents,
    RuntimeLock,
    SelectedModels,
    Finish,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackend {
    #[default]
    Auto,
    OpenVino,
    Vulkan,
    DiagnosticCpu,
}

impl ComputeBackend {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::OpenVino => "openvino",
            Self::Vulkan => "vulkan",
            Self::DiagnosticCpu => "diagnostic_cpu",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Hash)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadTarget {
    SharedRuntime,
    RoFormer,
    FireRed,
    QwenAsr,
    QwenAlign,
    Pitch,
    Fcpe,
    Game,
    Stars,
    BasicPitch,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct ModelInstallStatus {
    pub target: ModelDownloadTarget,
    pub label: String,
    pub description: String,
    pub available: bool,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub validation: String,
}

/// Exact Runtime Manager fact for one Analysis strategy row. Unlike
/// `ModelDownloadTarget::RoFormer`, this never projects bundle health onto an
/// individual provider/capability.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisStrategyResourceStatus {
    pub strategy_id: String,
    pub label: String,
    pub model_id: String,
    pub capability: String,
    pub available: bool,
    pub backend: String,
    pub validation: String,
    #[serde(default)]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
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
    pub downloaded_bytes: Option<u64>,
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

#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisRuntimeStatus {
    pub ready: bool,
    pub runtime_contract_current: bool,
    pub ffmpeg_available: bool,
    pub native_analyzer_available: bool,
    pub openvino_runtime_available: bool,
    pub ggml_vulkan_runtime_available: bool,
    pub qwen_asr_runtime_available: bool,
    pub qwen_align_runtime_available: bool,
    pub runtime_lock_valid: bool,
    pub pitch_model_available: bool,
    pub selected_models_available: bool,
    pub selected_models: Vec<String>,
    pub models: Vec<ModelInstallStatus>,
    pub compute_backend: String,
    pub ffmpeg_path: Option<String>,
    pub runtime_lock_sha256: String,
    pub missing: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, TS)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetupFolders {
    pub data_path: Option<String>,
    pub cache_paths: Option<crate::cache::CachePaths>,
    #[serde(default)]
    pub compute_backend: ComputeBackend,
    #[serde(default)]
    pub model_target: Option<ModelDownloadTarget>,
}
