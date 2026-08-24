use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::backend_cli::{
    NativeBackendWireV1, RuntimeCliClient, RuntimePolicyWireV1, ValidationStateWireV1,
};

pub const OPENVINO_WORKER_RECIPE_SHA256: &str =
    "bdeac2a4e1299e4bf82cb2d4edf64c7bdbc613fa40f58727c58793cf7f1a4093";

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

/// Compatibility projection of Runtime Manager's CLI-returned model facts.
/// No local catalog or backend policy is retained in Studio.
const NATIVE_RUNTIME_REGISTRY_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct NativeRuntimeRegistryCache {
    value: Option<Vec<NativeModelRuntime>>,
    refreshed_at: Option<Instant>,
}

static NATIVE_RUNTIME_REGISTRY_CACHE: OnceLock<Mutex<NativeRuntimeRegistryCache>> = OnceLock::new();

fn native_runtime_registry_cache() -> &'static Mutex<NativeRuntimeRegistryCache> {
    NATIVE_RUNTIME_REGISTRY_CACHE.get_or_init(|| Mutex::new(NativeRuntimeRegistryCache::default()))
}

pub(crate) fn invalidate_native_runtime_registry_cache() {
    if let Ok(mut cache) = native_runtime_registry_cache().lock() {
        cache.value = None;
        cache.refreshed_at = None;
    }
}

fn load_native_runtime_registry() -> Vec<NativeModelRuntime> {
    let Ok(client) = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Experimental))
    else {
        return Vec::new();
    };
    let Ok(statuses) = client.list() else {
        return Vec::new();
    };
    statuses
        .into_iter()
        .filter_map(|status| {
            let value = status.resource.0.strip_prefix("model:")?;
            let details = client.show(&status.resource).ok()?;
            let component_id = details
                .status
                .runtime_resource
                .as_ref()
                .and_then(|resource| resource.0.strip_prefix("runtime:"))
                .or_else(|| {
                    details
                        .metadata
                        .dependencies
                        .iter()
                        .find_map(|resource| resource.0.strip_prefix("runtime:"))
                })?
                .to_string();
            Some(NativeModelRuntime {
                model_id: value.to_string(),
                component_id,
                backends: details
                    .metadata
                    .backends
                    .into_iter()
                    .map(|backend| BackendCapability {
                        backend: map_backend(backend.backend),
                        validation: map_validation(backend.validation),
                        evidence_id: backend.evidence_id,
                    })
                    .collect(),
                // Runtime Manager already applied the explicit local testing policy.
                // Reuse its selected route instead of recompiling a production-only
                // choice inside Studio.
                pinned_backend: details.status.selected_backend.map(map_backend),
                runtime_recipe_digest: details.metadata.runtime_recipe_digest,
            })
        })
        .collect()
}

pub fn native_runtime_registry() -> Vec<NativeModelRuntime> {
    if let Ok(cache) = native_runtime_registry_cache().lock()
        && cache
            .refreshed_at
            .is_some_and(|refreshed| refreshed.elapsed() < NATIVE_RUNTIME_REGISTRY_CACHE_TTL)
        && let Some(value) = cache.value.as_ref()
    {
        return value.clone();
    }

    let value = load_native_runtime_registry();
    if let Ok(mut cache) = native_runtime_registry_cache().lock() {
        cache.value = Some(value.clone());
        cache.refreshed_at = Some(Instant::now());
    }
    value
}

/// Resolve only explicit packaged/development component variables. Resource
/// lifecycle and policy state are queried through `uta-runtime` elsewhere.
pub fn component_executable(component_id: &str) -> Option<PathBuf> {
    let variable = match component_id {
        "openvino_runtime" | "openvino_2026_3" => "UTA_STUDIO_OPENVINO_RUNTIME_PATH",
        "roformer_runtime" => "UTA_STUDIO_ROFORMER_RUNTIME_PATH",
        "qwen_asr_runtime" => "UTA_STUDIO_QWEN_ASR_RUNTIME_PATH",
        "qwen_align_runtime" => "UTA_STUDIO_QWEN_ALIGN_RUNTIME_PATH",
        "native_analyzer" => "UTA_STUDIO_NATIVE_ANALYZER_PATH",
        _ => return None,
    };
    std::env::var_os(variable)
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

pub(crate) fn map_backend(value: NativeBackendWireV1) -> NativeBackend {
    match value {
        NativeBackendWireV1::OpenVino => NativeBackend::OpenVino,
        NativeBackendWireV1::Vulkan => NativeBackend::Vulkan,
        NativeBackendWireV1::NativeDsp => NativeBackend::NativeDsp,
        NativeBackendWireV1::CpuReference => NativeBackend::CpuReference,
    }
}

pub(crate) fn map_validation(value: ValidationStateWireV1) -> ValidationState {
    match value {
        ValidationStateWireV1::ProductionPinned => ValidationState::ProductionPinned,
        ValidationStateWireV1::BenchmarkCandidate => ValidationState::BenchmarkCandidate,
        ValidationStateWireV1::Experimental => ValidationState::Experimental,
        ValidationStateWireV1::Unsupported => ValidationState::Unsupported,
    }
}
