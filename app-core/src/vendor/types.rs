use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SetupStep {
    PrepareFolders,
    Ffmpeg,
    BackendProtocols,
    SelectedModels,
    Finish,
}

/// Global runtime routing. `Ggml` and `LibtorchXpu` route every model to that
/// backend and leave saved per-model choices inactive; `Custom` routes each
/// model by its own saved backend and device choice.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ComputeBackend {
    #[default]
    Ggml,
    LibtorchXpu,
    Custom,
}

impl ComputeBackend {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ggml => "ggml",
            Self::LibtorchXpu => "libtorch_xpu",
            Self::Custom => "custom",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        if value == "custom" {
            return Some(Self::Custom);
        }
        match crate::backend_cli::NativeBackendWire::parse_setting(value)? {
            crate::backend_cli::NativeBackendWire::Ggml => Some(Self::Ggml),
            crate::backend_cli::NativeBackendWire::LibtorchXpu => Some(Self::LibtorchXpu),
        }
    }

    /// Maps the persisted settings spelling; an absent or unknown value is
    /// the pinned GGML Vulkan route.
    pub fn from_setting(value: Option<&str>) -> Self {
        value.and_then(Self::parse).unwrap_or_default()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Hash)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum ModelDownloadTarget {
    RoFormer,
    Pitch,
}

/// Download level the setup guide offers. `Standard` and `Maximum` are the
/// model sets the default workflow needs for Balanced and Maximum analysis;
/// `Complete` adds every other catalog model.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, TS, PartialEq, Eq, Hash)]
#[ts(export)]
#[serde(rename_all = "snake_case")]
pub enum SetupTier {
    Standard,
    Maximum,
    Complete,
}

/// One setup-guide level with its models and the download still needed.
#[derive(Debug, Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[ts(export)]
#[serde(rename_all = "camelCase")]
pub struct SetupTierOption {
    pub tier: SetupTier,
    pub model_ids: Vec<String>,
    pub missing_model_ids: Vec<String>,
    /// Catalog download estimate for the missing models.
    pub download_bytes: Option<u64>,
    /// Catalog installed-size estimate for every model in the level.
    pub installed_bytes: Option<u64>,
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
    pub ggml_runtime_available: bool,
    pub pitch_model_available: bool,
    pub selected_models_available: bool,
    pub selected_models: Vec<String>,
    pub models: Vec<ModelInstallStatus>,
    pub compute_backend: String,
    pub ffmpeg_path: Option<String>,
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
    #[serde(default)]
    pub model_tier: Option<SetupTier>,
}
