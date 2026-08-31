use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::backend_cli::{
    NativeBackendWireV1, RuntimeCliClient, RuntimeFusionProviderReportWireV1, RuntimePolicyWireV1,
    RuntimeResourceRefWireV1, RuntimeResourceStatusWireV1, ValidationStateWireV1,
};

pub const FUSION_AGENT_ADAPTER_RESOURCE_ID: &str = "fusion_agent_adapter";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeBackendPresentation {
    OpenVino,
    Vulkan,
    NativeDsp,
    CpuReference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeValidationPresentation {
    ProductionPinned,
    BenchmarkCandidate,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBackendCapabilityPresentation {
    pub backend: RuntimeBackendPresentation,
    pub validation: RuntimeValidationPresentation,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeModelPresentation {
    pub model_id: String,
    pub component_id: String,
    pub backends: Vec<RuntimeBackendCapabilityPresentation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_backend: Option<RuntimeBackendPresentation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

/// Read-only presentation projection of Runtime Manager CLI facts.
/// Studio never resolves or executes a runtime from this representation.
const RUNTIME_PRESENTATION_CACHE_TTL: Duration = Duration::from_secs(60);

#[derive(Default)]
struct RuntimePresentationCache {
    value: Option<Vec<RuntimeModelPresentation>>,
    refreshed_at: Option<Instant>,
}

static RUNTIME_PRESENTATION_CACHE: OnceLock<Mutex<RuntimePresentationCache>> = OnceLock::new();

fn runtime_presentation_cache() -> &'static Mutex<RuntimePresentationCache> {
    RUNTIME_PRESENTATION_CACHE.get_or_init(|| Mutex::new(RuntimePresentationCache::default()))
}

pub(crate) fn invalidate_runtime_presentation_cache() {
    if let Ok(mut cache) = runtime_presentation_cache().lock() {
        cache.value = None;
        cache.refreshed_at = None;
    }
}

fn load_runtime_presentations() -> Vec<RuntimeModelPresentation> {
    let Ok(client) = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
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
            Some(RuntimeModelPresentation {
                model_id: value.to_string(),
                component_id,
                backends: details
                    .metadata
                    .backends
                    .into_iter()
                    .map(|backend| RuntimeBackendCapabilityPresentation {
                        backend: map_backend(backend.backend),
                        validation: map_validation(backend.validation),
                        evidence_id: backend.evidence_id,
                    })
                    .collect(),
                // Runtime Manager already applied the Production policy. Reuse its
                // selected route instead of recompiling policy inside Studio.
                selected_backend: details.status.selected_backend.map(map_backend),
                runtime_recipe_digest: details.metadata.runtime_recipe_digest,
            })
        })
        .collect()
}

pub fn fusion_agent_adapter_status() -> Result<RuntimeResourceStatusWireV1, String> {
    let client = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?;
    let resource = RuntimeResourceRefWireV1::tool(FUSION_AGENT_ADAPTER_RESOURCE_ID)?;
    client
        .status(&[resource])
        .map_err(|error| error.to_string())?
        .into_iter()
        .next()
        .ok_or_else(|| "Runtime Manager omitted Fusion Agent Adapter status".to_string())
}

pub fn configure_fusion_agent_adapter(
    executable: &std::path::Path,
) -> Result<RuntimeResourceStatusWireV1, String> {
    let status = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?
        .configure_tool(FUSION_AGENT_ADAPTER_RESOURCE_ID, executable)
        .map_err(|error| error.to_string())?;
    invalidate_runtime_presentation_cache();
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

pub fn clear_fusion_agent_adapter() -> Result<RuntimeResourceStatusWireV1, String> {
    let status = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?
        .clear_tool(FUSION_AGENT_ADAPTER_RESOURCE_ID)
        .map_err(|error| error.to_string())?;
    invalidate_runtime_presentation_cache();
    crate::invalidate_analysis_runtime_status_cache();
    Ok(status)
}

/// Discover provider CLIs and native adapters through Runtime Manager. This
/// projection contains no raw executable path and performs no provider call.
pub fn fusion_provider_status() -> Result<RuntimeFusionProviderReportWireV1, String> {
    RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?
        .fusion_providers()
        .map_err(|error| error.to_string())
}

/// Persist a provider identity (`pi`, `codex`, or `claude`) in Runtime Manager.
/// The provider CLI remains responsible for credentials and external network
/// policy; Studio never receives or stores its executable path.
pub fn configure_fusion_provider(
    provider: &str,
) -> Result<RuntimeFusionProviderReportWireV1, String> {
    let report = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?
        .configure_fusion_provider(provider)
        .map_err(|error| error.to_string())?;
    invalidate_runtime_presentation_cache();
    crate::invalidate_analysis_runtime_status_cache();
    Ok(report)
}

pub fn clear_fusion_provider() -> Result<RuntimeFusionProviderReportWireV1, String> {
    let report = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Production))
        .map_err(|error| error.to_string())?
        .clear_fusion_provider()
        .map_err(|error| error.to_string())?;
    invalidate_runtime_presentation_cache();
    crate::invalidate_analysis_runtime_status_cache();
    Ok(report)
}

pub fn runtime_model_presentations() -> Vec<RuntimeModelPresentation> {
    if let Ok(cache) = runtime_presentation_cache().lock()
        && cache
            .refreshed_at
            .is_some_and(|refreshed| refreshed.elapsed() < RUNTIME_PRESENTATION_CACHE_TTL)
        && let Some(value) = cache.value.as_ref()
    {
        return value.clone();
    }

    let value = load_runtime_presentations();
    if let Ok(mut cache) = runtime_presentation_cache().lock() {
        cache.value = Some(value.clone());
        cache.refreshed_at = Some(Instant::now());
    }
    value
}

pub(crate) fn map_backend(value: NativeBackendWireV1) -> RuntimeBackendPresentation {
    match value {
        NativeBackendWireV1::OpenVino => RuntimeBackendPresentation::OpenVino,
        NativeBackendWireV1::Vulkan => RuntimeBackendPresentation::Vulkan,
        NativeBackendWireV1::NativeDsp => RuntimeBackendPresentation::NativeDsp,
        NativeBackendWireV1::CpuReference => RuntimeBackendPresentation::CpuReference,
    }
}

pub(crate) fn map_validation(value: ValidationStateWireV1) -> RuntimeValidationPresentation {
    match value {
        ValidationStateWireV1::ProductionPinned => RuntimeValidationPresentation::ProductionPinned,
        ValidationStateWireV1::BenchmarkCandidate => {
            RuntimeValidationPresentation::BenchmarkCandidate
        }
        ValidationStateWireV1::Experimental => RuntimeValidationPresentation::Experimental,
        ValidationStateWireV1::Unsupported => RuntimeValidationPresentation::Unsupported,
    }
}
