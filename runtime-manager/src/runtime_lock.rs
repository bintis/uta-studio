use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const RUNTIME_LOCK_JSON: &str = include_str!("../../native-inference/runtime-lock.json");

pub const GGML_RUNTIME_RECIPE_SHA256: &str =
    "fd238ace2d64c95054c2e281c2e2790d8bb7f7c13b19c49fa02acf20a71fa3d3";
/// Identity of `native-inference/libtorch-runtime/runtime-recipe.json`. It is
/// provenance metadata carried in resolved routes, not a verification gate.
pub const LIBTORCH_XPU_RUNTIME_RECIPE_SHA256: &str =
    "9a7ab279294916c2944f2161edde9ff4a47ab55e74007e22fe57aa2978bd4707";
pub const RMVPE_GGUF_SHA256: &str =
    "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2";
pub const RMVPE_GGUF_SIZE_BYTES: u64 = 361_625_344;
pub const FCPE_GGUF_SHA256: &str =
    "6f6dcf86133608191d798897adda6fb1d9d480f7990e2c11c9e255e1d8c65673";
pub const FCPE_GGUF_SIZE_BYTES: u64 = 43_309_760;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeRuntimeLock {
    pub policy: RuntimePolicyLock,
    pub components: BTreeMap<String, serde_json::Value>,
    pub status: String,
    pub repository: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimePolicyLock {
    pub execution_backend: String,
    pub fallback: String,
    pub model_format: String,
}

pub fn native_runtime_lock() -> Result<NativeRuntimeLock, String> {
    serde_json::from_str(RUNTIME_LOCK_JSON).map_err(|error| error.to_string())
}

pub fn runtime_recipe_digest(component: &str) -> Result<String, String> {
    match component {
        "ggml_vulkan" => Ok(GGML_RUNTIME_RECIPE_SHA256.to_string()),
        "libtorch_xpu" => Ok(LIBTORCH_XPU_RUNTIME_RECIPE_SHA256.to_string()),
        _ => Err(format!(
            "unknown native runtime-lock component: {component}"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_lock_names_both_selectable_native_runtimes() {
        let lock = native_runtime_lock().unwrap();
        assert_eq!(lock.policy.execution_backend, "ggml");
        assert_eq!(lock.policy.fallback, "none");
        assert_eq!(lock.components.len(), 2);
        assert!(lock.components.contains_key("ggml_vulkan"));
        assert!(lock.components.contains_key("libtorch_xpu"));
        assert_eq!(
            runtime_recipe_digest("libtorch_xpu").unwrap(),
            LIBTORCH_XPU_RUNTIME_RECIPE_SHA256
        );
        assert!(runtime_recipe_digest("libtorch_rocm").is_err());
    }
}
