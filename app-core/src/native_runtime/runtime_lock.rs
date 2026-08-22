use serde::{Deserialize, Serialize};

pub const RUNTIME_LOCK_JSON: &str = include_str!("../../../native-inference/runtime-lock.json");
pub const RUNTIME_LOCK_SHA256: &str =
    "c6f2228718c832323e053cf62815d7ba7ff01309c899bcafe8adc68db4fc200d";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRuntimeLock {
    pub schema_version: u32,
    pub policy: RuntimePolicyLock,
    pub components: RuntimeComponents,
    pub document_version: String,
    pub status: String,
    pub repository: String,
    pub branch: String,
    pub baseline_commit: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePolicyLock {
    pub generic_native_models: GenericRuntimePolicyLock,
    pub pinned_exceptions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenericRuntimePolicyLock {
    pub preferred: String,
    pub fallback: String,
    pub on_no_validated_backend: String,
    pub cpu: String,
    #[serde(rename = "python")]
    pub forbidden_script_fallback: String,
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeComponents {
    pub openvino_2026_3: OpenVinoLock,
    pub qwen3_forced_aligner_0_6b: QwenAlignLock,
    pub qwen3_asr_1_7b: QwenAsrLock,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenVinoLock {
    pub runtime_repository: String,
    pub runtime_tag: String,
    pub runtime_commit: String,
    pub build_recipe: String,
    pub build_recipe_sha256: String,
    pub production_backend: String,
    pub cpu_plugin: bool,
    pub script_bindings: bool,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QwenAlignLock {
    pub runtime_repository: String,
    pub runtime_commit: String,
    pub cpu_reference_ggml_commit: String,
    pub vulkan_ggml_override_commit: String,
    pub integration_patch: String,
    pub integration_patch_sha256: String,
    pub production_backend: String,
    pub model_repository: String,
    pub model_revision: String,
    pub gguf_file: String,
    pub gguf_sha256: String,
    pub notes: String,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QwenAsrLock {
    pub runtime_repository: String,
    pub runtime_commit: String,
    pub ggml_commit: String,
    pub production_backend: String,
    pub source_model_repository: String,
    pub source_model_revision: String,
    pub gguf_repository: String,
    pub gguf_file: String,
    pub gguf_sha256: String,
    pub status: String,
}

pub fn native_runtime_lock() -> Result<NativeRuntimeLock, String> {
    serde_json::from_str(RUNTIME_LOCK_JSON).map_err(|error| error.to_string())
}

pub fn runtime_recipe_digest(component: &str) -> Result<String, String> {
    let lock = native_runtime_lock()?;
    let value = match component {
        "openvino_2026_3" => serde_json::to_value(lock.components.openvino_2026_3),
        "qwen3_asr_1_7b" => serde_json::to_value(lock.components.qwen3_asr_1_7b),
        "qwen3_forced_aligner_0_6b" => {
            serde_json::to_value(lock.components.qwen3_forced_aligner_0_6b)
        }
        _ => return Err(format!("unknown runtime-lock component: {component}")),
    }
    .map_err(|error| error.to_string())?;
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(blake3::hash(&bytes).to_hex()[..32].to_string())
}
