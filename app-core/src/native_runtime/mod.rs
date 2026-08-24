mod protocol;
mod registry;
mod router;
mod runtime_lock;
mod supervisor;
mod worker;

pub use protocol::*;
pub use registry::*;
pub use router::*;
pub use runtime_lock::*;
pub use supervisor::*;
pub use worker::*;

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::*;

    #[test]
    fn runtime_lock_digest_matches_checked_in_document() {
        assert_eq!(
            format!("{:x}", Sha256::digest(RUNTIME_LOCK_JSON.as_bytes())),
            RUNTIME_LOCK_SHA256
        );
    }

    #[test]
    fn runtime_lock_matches_final_qwen_recipes() {
        let lock = native_runtime_lock().unwrap();
        assert_eq!(
            lock.components.qwen3_asr_1_7b.runtime_commit,
            "ea077b87590bcfb090d7c38c03ab36cd1c7005d3"
        );
        assert_eq!(
            lock.components.qwen3_forced_aligner_0_6b.runtime_commit,
            "6dcc586e5073fd6e85ee5728e75f0903d6c70c6c"
        );
        assert_eq!(
            lock.components
                .qwen3_forced_aligner_0_6b
                .vulkan_ggml_override_commit,
            "8c63e70982c95ceb862e3a1073a2c1beef75d60a"
        );
        assert_eq!(
            lock.components.openvino_2026_3.runtime_commit,
            "8a17657b995fd3b4a52f8484acfcf2bb61214623"
        );
        assert_eq!(
            lock.components.openvino_2026_3.build_recipe_sha256,
            OPENVINO_WORKER_RECIPE_SHA256
        );
        assert!(lock.components.openvino_2026_3.cpu_plugin);
        assert!(!lock.components.openvino_2026_3.script_bindings);
    }

    #[test]
    fn generic_router_never_selects_unvalidated_or_cpu_backends() {
        let registry = native_runtime_registry();
        let rmvpe = registry
            .iter()
            .find(|model| model.model_id == "rmvpe")
            .unwrap();
        assert!(rmvpe.backends.iter().any(|capability| {
            capability.backend == NativeBackend::OpenVino
                && capability.validation == ValidationState::ProductionPinned
        }));
        assert!(!rmvpe.backends.iter().any(|capability| {
            capability.backend == NativeBackend::CpuReference
                && capability.validation == ValidationState::ProductionPinned
        }));
        for model in registry.iter().filter(|model| {
            model.model_id != "qwen3_asr_1_7b" && model.model_id != "qwen3_forced_aligner_0_6b"
        }) {
            assert!(!model.backends.iter().any(|capability| {
                capability.backend == NativeBackend::Vulkan
                    && capability.validation == ValidationState::ProductionPinned
            }));
        }
        for roformer in registry
            .iter()
            .filter(|model| model.model_id.contains("roformer"))
        {
            assert!(
                roformer.backends.iter().all(|capability| {
                    capability.validation != ValidationState::ProductionPinned
                })
            );
        }
    }
}
