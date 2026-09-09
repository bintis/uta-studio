use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const RUNTIME_LOCK_JSON: &str = include_str!("../../native-inference/runtime-lock.json");

pub const GGML_RUNTIME_RECIPE_SHA256: &str =
    "1d1ece1e929b70b2d95c5ea3694940ebd385595b9dd99f5c3d6623a06e0e81ef";
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
        _ => Err(format!("unknown GGML runtime-lock component: {component}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_lock_is_ggml_only() {
        let lock = native_runtime_lock().unwrap();
        assert_eq!(lock.policy.execution_backend, "ggml");
        assert_eq!(lock.components.len(), 1);
        assert!(lock.components.contains_key("ggml_vulkan"));
    }
}
