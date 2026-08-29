use serde::{Deserialize, Serialize};

pub const RUNTIME_LOCK_JSON: &str = include_str!("../../native-inference/runtime-lock.json");

pub const OPENVINO_WORKER_RECIPE_SHA256: &str =
    "bdeac2a4e1299e4bf82cb2d4edf64c7bdbc613fa40f58727c58793cf7f1a4093";
pub const RMVPE_IR_RELATIVE_DIR: &str = "pitch/rmvpe/openvino-ir-2026.3.0-bucketed";
pub const RMVPE_SOURCE_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";
pub const RMVPE_IR_MANIFEST_SHA256: &str =
    "cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb";
pub const RMVPE_CONVERSION_RECIPE_SHA256: &str =
    "ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876";
pub const BS_ROFORMER_SOURCE_SHA256: &str =
    "5b84f37e8d444c8cb30c79d77f613a41c05868ff9c9ac6c7049c00aefae115aa";
pub const BS_ROFORMER_CONFIG_SHA256: &str =
    "2bfdd16c656bd9519aba757cc4f8834b7ede675eb1e00ec4772d74ae1c41af7f";
pub const BS_ROFORMER_CONVERSION_RECIPE_SHA256: &str =
    "c64fdf13ca6d38063bbe39f8a44cf2518b7d26f18f394b3897539eff3cc0c69a";
pub const BS_ROFORMER_IR_MANIFEST_SHA256: &str =
    "530fe75a8cab9d3391b42f4945cd57e24db4c4ffca348ccff065f2f3af9b8d98";
pub const ROFORMER_INST_V2_SOURCE_SHA256: &str =
    "bd19766620f7d6f58fdf7aaada7e89907fe41bc64490ce3faa9a6dab15d6e1f2";
pub const ROFORMER_INST_V2_CONFIG_SHA256: &str =
    "4b902a7360a930c178edb4846b30e4e326aa1219d1b2daf660d46a311e0cd50b";
pub const ROFORMER_INST_V2_CONVERSION_RECIPE_SHA256: &str =
    "1dfb93131898bbfb9197f0c0efb87314285aee27d03e3d94c83d1d8f1def5033";
pub const ROFORMER_INST_V2_IR_MANIFEST_SHA256: &str =
    "683c16d852ec16ebc68679656622c2b6bfe75e55dd0201d9e2ccab8fb979d40c";
pub const ROFORMER_HARMONY_SOURCE_SHA256: &str =
    "1de20d459332fe8869aeb01327a31df0032262706e1365114e852dc271779813";
pub const ROFORMER_HARMONY_CONFIG_SHA256: &str =
    "b35077d94861f068097cce1a5e54633c055e7dcc2613eade4e4dc7c7c9c3f48b";
pub const ROFORMER_HARMONY_MONOLITHIC_RECIPE_SHA256: &str =
    "4d428de5af93bb5163e19cbc0aea197b233c254d60d7e318347cdefe402c52ce";
pub const ROFORMER_HARMONY_CONVERSION_RECIPE_SHA256: &str =
    "9eca7db06d98222bd58cbdd19559e1d93772966d073e1b5ebc7214fa9d07d18b";
pub const ROFORMER_HARMONY_IR_MANIFEST_SHA256: &str =
    "c7b3a1fbab8489002ad5449870c54f331849c29190ae77497580b0bd8cec7ab6";
pub const ROFORMER_DENOISE_SOURCE_SHA256: &str =
    "7c1c39191edc34e942ca7f2346ce6b6c0e1208a5f76349ffce6f696bd12910de";
pub const ROFORMER_DENOISE_CONFIG_SHA256: &str =
    "5d7d83b2e9d232da60941b717b0abdc345155d45cff3f79715cdb2790ba18c36";
pub const ROFORMER_DENOISE_CONVERSION_RECIPE_SHA256: &str =
    "b00225b4e1fb69866990c2dded35ea5c9cfd6b4a6d14f76e67db940cdf923e0d";
pub const ROFORMER_DENOISE_IR_MANIFEST_SHA256: &str =
    "bfeb9b9f327332b5e062aaa0583ab69ece7e4be6a7af2468e6895f675807c047";
pub const ROFORMER_DENOISE_XML_SHA256: &str =
    "925a1ee18c25bc063da80a3fc259ea98b8f08520b9da8fbbb56062f5741cc47c";
pub const ROFORMER_DENOISE_BIN_SHA256: &str =
    "1f48a656ac181f894e3597d2f00cd8a95466eccbe6ff194975984620eda9869f";
pub const ROFORMER_DEREVERB_SOURCE_SHA256: &str =
    "9262877b87e9ebb0fb808a456b0a411fa677f5df31c8383c1254af531c078970";
pub const ROFORMER_DEREVERB_CONFIG_SHA256: &str =
    "66963a9d60756076506a230b4e503c553a3beb7b4e9a10e6bcc73dee9dbd4866";
pub const ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256: &str =
    "959c81f61936a3280baa277890ebe9bd3003944968a7a4b229342d6b155170ee";
pub const ROFORMER_DEREVERB_IR_MANIFEST_SHA256: &str =
    "7cf591f56a76e513a92c394bed9c32cf32f833b60c06d43ab01a390e3d3d75ae";
pub const ROFORMER_DEREVERB_XML_SHA256: &str =
    "b9d680b42cfd8573f55eba5bf1721cf44c54e34e463a35908b5c1a0431d46ce9";
pub const ROFORMER_DEREVERB_BIN_SHA256: &str =
    "e113d853ceeb125f734df18a26c2c9f2d43f109cf1a361f5bc5f1ce707158171";
pub const FIRERED_IR_MANIFEST_SHA256: &str =
    "093335b6a113e5eead88bb011a7870d61f18319e8d0204523c3ce9d82e6c8c35";
pub const FCPE_SOURCE_SHA256: &str =
    "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0";
pub const FCPE_IR_MANIFEST_SHA256: &str =
    "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6";
pub const BASIC_PITCH_SOURCE_SHA256: &str =
    "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";
pub const STARS_IR_MANIFEST_SHA256: &str =
    "37036e2273ca633f95263b45ca8f2f60652858b8a5db0d03bf85c87a593bef9e";
pub const STARS_CONVERSION_RECIPE_SHA256: &str =
    "b2d2c9918704c545a9d0ea86524c02f1c790c4ca9f995f8c32b5d71ea6596e1f";
pub const ROSVOT_IR_MANIFEST_SHA256: &str =
    "a84ef89fba4863a49198f83c232b3a8d14c1ec3b44ad58ef6407a7528e82e9e5";
pub const ROSVOT_CONVERSION_RECIPE_SHA256: &str =
    "6b279054e9cb38c5f92277bf6e218cd0262aa3e0a60dcb0814f91957b9c2150e";
pub const BASIC_PITCH_IR_MANIFEST_SHA256: &str =
    "01b35925daaeb40995f4e49b495e6f1ce9db47c7f41987b19fdc1b5c35f2c1b7";
pub const GAME_SOURCE_ASSET_SHA256: &str =
    "5b7a21e64c6310efac399f5d12838fffa70565be162436b5a4a65f290721e7d8";
pub const GAME_IR_MANIFEST_SHA256: &str =
    "aa9f3a4c2d107527913ef3947f337b41bff7b6de39de6c91ce46b82ced15ac87";
pub const GAME_CONVERSION_RECIPE_SHA256: &str =
    "a4fdeda6c061aa4d43649bc13b86cf46828c233cf3add2a84c531c17d9457426";

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
    pub ggml_vulkan_v1: GgmlVulkanLock,
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
pub struct GgmlVulkanLock {
    pub runtime_repository: String,
    pub runtime_commit: String,
    pub build_recipe: String,
    pub build_recipe_sha256: String,
    pub backend: String,
    pub model_format: String,
    pub validation: String,
    pub cpu_fallback: bool,
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
    pub source_file: String,
    pub source_sha256: String,
    pub source_size_bytes: u64,
    pub source_format: String,
    pub source_license: String,
    pub gguf_file: String,
    pub gguf_sha256: String,
    pub gguf_size_bytes: u64,
    pub gguf_format: String,
    pub gguf_origin: String,
    pub conversion_recipe_digest: String,
    pub text_normalization_profile: String,
    pub language_normalization_profile: String,
    pub supported_language_codes: Vec<String>,
    pub unsupported_language_behavior: String,
    pub alignment_semantics_profile: String,
    pub local_import_receipt: String,
    pub notices: String,
    pub historical_evidence_id: String,
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
    pub gguf_repository_revision: String,
    pub gguf_file: String,
    pub gguf_sha256: String,
    pub status: String,
}

pub fn native_runtime_lock() -> Result<NativeRuntimeLock, String> {
    serde_json::from_str(RUNTIME_LOCK_JSON).map_err(|error| error.to_string())
}

pub fn runtime_recipe_digest(component: &str) -> Result<String, String> {
    let lock = native_runtime_lock()?;
    let mut value = match component {
        "openvino_2026_3" => serde_json::to_value(lock.components.openvino_2026_3),
        "ggml_vulkan_v1" => serde_json::to_value(lock.components.ggml_vulkan_v1),
        "qwen3_asr_1_7b" => serde_json::to_value(lock.components.qwen3_asr_1_7b),
        "qwen3_forced_aligner_0_6b" => {
            serde_json::to_value(lock.components.qwen3_forced_aligner_0_6b)
        }
        _ => return Err(format!("unknown runtime-lock component: {component}")),
    }
    .map_err(|error| error.to_string())?;
    // Acquisition, conversion, and worker I/O provenance do not alter the
    // already-pinned native engine ABI represented by historical manifests.
    if component == "qwen3_asr_1_7b" {
        value
            .as_object_mut()
            .ok_or_else(|| "Qwen ASR runtime lock component is not an object".to_string())?
            .remove("gguf_repository_revision");
    } else if component == "qwen3_forced_aligner_0_6b" {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "Qwen aligner runtime lock component is not an object".to_string())?;
        for field in [
            "source_file",
            "source_sha256",
            "source_size_bytes",
            "source_format",
            "source_license",
            "gguf_size_bytes",
            "gguf_format",
            "gguf_origin",
            "conversion_recipe_digest",
            "text_normalization_profile",
            "language_normalization_profile",
            "supported_language_codes",
            "unsupported_language_behavior",
            "alignment_semantics_profile",
            "local_import_receipt",
            "notices",
            "historical_evidence_id",
        ] {
            object.remove(field);
        }
        // Preserve compatibility with already-installed engine manifests.
        object.insert(
            "notes".to_string(),
            serde_json::Value::String("Vulkan validation used the predict-woo model graph with the transcribe.cpp-compatible GGML Vulkan revision. Any compatibility patch must be vendored and hashed.".to_string()),
        );
        object.insert(
            "status".to_string(),
            serde_json::Value::String("pinned_runtime_recipe".to_string()),
        );
    }
    let bytes = serde_json::to_vec(&value).map_err(|error| error.to_string())?;
    Ok(blake3::hash(&bytes).to_hex()[..32].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_lock_parses_and_preserves_pinned_recipe_identities() {
        let lock = native_runtime_lock().unwrap();
        assert_eq!(
            lock.components.qwen3_asr_1_7b.runtime_commit,
            "ea077b87590bcfb090d7c38c03ab36cd1c7005d3"
        );
        assert_eq!(
            lock.components.qwen3_asr_1_7b.gguf_repository_revision,
            "92282af1610a2db19d66f2bef1e260f5deca782d"
        );
        assert_eq!(
            lock.components.qwen3_forced_aligner_0_6b.runtime_commit,
            "6dcc586e5073fd6e85ee5728e75f0903d6c70c6c"
        );
        assert_eq!(
            lock.components.qwen3_forced_aligner_0_6b.source_sha256,
            "00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86"
        );
        assert_eq!(
            lock.components
                .qwen3_forced_aligner_0_6b
                .conversion_recipe_digest,
            "ffd8a575238c81823509e2a7bf645bf9bb5d38db2903bc3306648afd619b42d6"
        );
        assert_eq!(
            lock.components.openvino_2026_3.build_recipe_sha256,
            OPENVINO_WORKER_RECIPE_SHA256
        );
        assert_eq!(
            lock.components.openvino_2026_3.status,
            "production_ir_runtime"
        );
        assert_eq!(
            lock.policy.generic_native_models.preferred,
            "model_pinned_native_backend"
        );
        assert_eq!(lock.policy.generic_native_models.fallback, "none");
        assert_eq!(
            lock.components.ggml_vulkan_v1.validation,
            "production_pinned"
        );
        assert_eq!(lock.components.ggml_vulkan_v1.status, "production_admitted");
        assert_eq!(
            lock.components.qwen3_forced_aligner_0_6b.status,
            "pinned_runtime_recipe"
        );
        assert_eq!(
            runtime_recipe_digest("qwen3_asr_1_7b").unwrap(),
            "53083b7b39dd2a805f441453ae07c797"
        );
        assert_eq!(
            runtime_recipe_digest("qwen3_forced_aligner_0_6b").unwrap(),
            "3ec367aaf3f723079851e2fbdbd375f8"
        );
    }
}
