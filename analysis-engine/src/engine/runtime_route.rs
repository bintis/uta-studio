use crate::contract::{EngineError, EngineErrorCode, EngineResult, ResolvedResourceProvenanceV1};

pub(super) fn normalized_transcript(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(super) fn roformer_backend(
    model: &uta_runtime_manager::ResolvedModel,
) -> EngineResult<&'static str> {
    match model.backend {
        uta_runtime_manager::NativeBackend::OpenVino => Ok("openvino_gpu"),
        uta_runtime_manager::NativeBackend::CpuReference => Ok("openvino_cpu"),
        uta_runtime_manager::NativeBackend::Vulkan => Ok("ggml_vulkan"),
        _ => Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved to a backend unsupported by the RoFormer route",
                model.model_id
            ),
        )),
    }
}

pub(super) fn roformer_component(backend: &str) -> &'static str {
    match backend {
        "ggml_vulkan" => "uta-ggml-worker",
        _ => "uta-openvino-worker",
    }
}

pub(super) fn openvino_backend(
    model: &uta_runtime_manager::ResolvedModel,
) -> EngineResult<&'static str> {
    match model.backend {
        uta_runtime_manager::NativeBackend::OpenVino => Ok("openvino_gpu"),
        uta_runtime_manager::NativeBackend::CpuReference => Ok("openvino_cpu"),
        _ => Err(EngineError::new(
            EngineErrorCode::RuntimeResolutionFailed,
            format!(
                "model {} resolved to a backend that the OpenVINO worker cannot execute",
                model.model_id
            ),
        )),
    }
}

pub(super) fn execution_device(backend: uta_runtime_manager::NativeBackend) -> &'static str {
    match backend {
        uta_runtime_manager::NativeBackend::OpenVino
        | uta_runtime_manager::NativeBackend::Vulkan => "device:0",
        uta_runtime_manager::NativeBackend::NativeDsp => "native",
        uta_runtime_manager::NativeBackend::CpuReference => "diagnostic_cpu",
    }
}

pub(super) fn resource_provenance(
    resource: &uta_runtime_manager::ResolvedModel,
) -> ResolvedResourceProvenanceV1 {
    ResolvedResourceProvenanceV1 {
        resource: format!("model:{}", resource.model_id),
        generation: resource.generation.clone(),
        content_digest: resource.model_content_digest.clone(),
        runtime: resource.runtime_id.clone(),
        runtime_generation: resource.runtime_generation.clone(),
        runtime_recipe_digest: resource.runtime_recipe_digest.clone(),
        backend: match resource.backend {
            uta_runtime_manager::NativeBackend::OpenVino => "openvino",
            uta_runtime_manager::NativeBackend::Vulkan => "vulkan",
            uta_runtime_manager::NativeBackend::NativeDsp => "native_dsp",
            uta_runtime_manager::NativeBackend::CpuReference => "cpu_reference",
        }
        .to_string(),
        device: execution_device(resource.backend).to_string(),
    }
}
