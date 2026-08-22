use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const OPENVINO_WORKER_RECIPE_SHA256: &str =
    "bd349389e6d0d0b742ae103892c1e5774599dd8733460aec80cb74bcf20ddab6";
pub const RMVPE_IR_RELATIVE_DIR: &str = "pitch/rmvpe/openvino-ir-2026.3.0-bucketed";
pub const RMVPE_IR_MANIFEST_SHA256: &str =
    "cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBackend {
    OpenVino,
    Vulkan,
    NativeDsp,
    CpuReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationState {
    ProductionPinned,
    BenchmarkCandidate,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendCapability {
    pub backend: NativeBackend,
    pub validation: ValidationState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeModelRuntime {
    pub model_id: String,
    pub component_id: String,
    pub backends: Vec<BackendCapability>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned_backend: Option<NativeBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

pub fn native_runtime_registry() -> Vec<NativeModelRuntime> {
    use NativeBackend::*;
    use ValidationState::*;

    let generic = |model_id: &str, component_id: &str, openvino, vulkan| NativeModelRuntime {
        model_id: model_id.to_string(),
        component_id: component_id.to_string(),
        backends: vec![
            BackendCapability {
                backend: OpenVino,
                validation: openvino,
                evidence_id: None,
            },
            BackendCapability {
                backend: Vulkan,
                validation: vulkan,
                evidence_id: None,
            },
        ],
        pinned_backend: None,
        runtime_recipe_digest: (component_id == "openvino_runtime")
            .then(|| OPENVINO_WORKER_RECIPE_SHA256.to_string()),
    };

    vec![
        // Conservative 12-second Vulkan smokes pass, but sustained tests have
        // hard-locked or powered off the Arc host. Keep every exact RoFormer
        // profile a candidate until the full-song/repeat gate passes.
        generic(
            "bs_roformer_vocals_ep317",
            "roformer_runtime",
            Unsupported,
            BenchmarkCandidate,
        ),
        generic(
            "melband_roformer_inst_v2",
            "roformer_runtime",
            Unsupported,
            BenchmarkCandidate,
        ),
        generic(
            "melband_roformer_harmony",
            "roformer_runtime",
            Unsupported,
            BenchmarkCandidate,
        ),
        generic(
            "melband_roformer_denoise_aufr33",
            "roformer_runtime",
            Unsupported,
            BenchmarkCandidate,
        ),
        generic(
            "melband_roformer_dereverb_anvuew",
            "roformer_runtime",
            Unsupported,
            BenchmarkCandidate,
        ),
        generic(
            "firered_asr2_aed",
            "openvino_runtime",
            BenchmarkCandidate,
            Unsupported,
        ),
        generic("rmvpe", "openvino_runtime", ProductionPinned, Unsupported),
        generic("fcpe", "openvino_runtime", BenchmarkCandidate, Unsupported),
        generic("game", "openvino_runtime", BenchmarkCandidate, Unsupported),
        generic(
            "basic_pitch",
            "openvino_runtime",
            BenchmarkCandidate,
            Unsupported,
        ),
        generic("stars", "openvino_runtime", Experimental, Unsupported),
        NativeModelRuntime {
            model_id: "qwen3_asr_1_7b".to_string(),
            component_id: "qwen_asr_runtime".to_string(),
            backends: vec![BackendCapability {
                backend: Vulkan,
                // The runtime recipe is pinned, but full-song singing quality
                // and app integration have not passed the production gate.
                validation: BenchmarkCandidate,
                evidence_id: Some("validation:qwen-runtime-validation".to_string()),
            }],
            pinned_backend: Some(Vulkan),
            runtime_recipe_digest: super::runtime_recipe_digest("qwen3_asr_1_7b").ok(),
        },
        NativeModelRuntime {
            model_id: "qwen3_forced_aligner_0_6b".to_string(),
            component_id: "qwen_align_runtime".to_string(),
            backends: vec![BackendCapability {
                backend: Vulkan,
                // Real Vulkan inference passed, but correct complete-lyrics
                // whole-song quality and packaging remain acceptance gates.
                validation: BenchmarkCandidate,
                evidence_id: Some("validation:qwen-runtime-validation".to_string()),
            }],
            pinned_backend: Some(Vulkan),
            runtime_recipe_digest: super::runtime_recipe_digest("qwen3_forced_aligner_0_6b").ok(),
        },
    ]
}

pub fn component_executable(component_id: &str) -> Option<PathBuf> {
    let variable = match component_id {
        "roformer_runtime" => "UTA_STUDIO_ROFORMER_RUNTIME_PATH",
        "openvino_runtime" => "UTA_STUDIO_OPENVINO_RUNTIME_PATH",
        "qwen_asr_runtime" => "UTA_STUDIO_QWEN_ASR_RUNTIME_PATH",
        "qwen_align_runtime" => "UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH",
        "native_analyzer" => "UTA_STUDIO_NATIVE_ANALYZER_PATH",
        _ => return None,
    };
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}
