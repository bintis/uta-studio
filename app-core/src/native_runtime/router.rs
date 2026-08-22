use serde::{Deserialize, Serialize};

use super::{
    NativeBackend, NativeModelRuntime, ValidationState, component_executable,
    native_runtime_registry,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedNativeRuntime {
    pub model_id: String,
    pub component_id: String,
    pub backend: NativeBackend,
    pub executable: std::path::PathBuf,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_recipe_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuntimeRouteError {
    UnknownModel(String),
    NoValidatedBackend(String),
    ComponentUnavailable(String),
    CpuProductionForbidden,
}

impl std::fmt::Display for RuntimeRouteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownModel(model) => write!(formatter, "unknown native model: {model}"),
            Self::NoValidatedBackend(model) => write!(
                formatter,
                "no validated native backend is available for {model}; install it in Settings > Models & runtime"
            ),
            Self::ComponentUnavailable(component) => write!(
                formatter,
                "native component {component} is unavailable; install it in Settings > Models & runtime"
            ),
            Self::CpuProductionForbidden => {
                formatter.write_str("CPU is available only for explicit reference diagnostics")
            }
        }
    }
}

fn production_backend(model: &NativeModelRuntime) -> Option<NativeBackend> {
    if let Some(pinned) = model.pinned_backend {
        return model
            .backends
            .iter()
            .any(|capability| {
                capability.backend == pinned
                    && capability.validation == ValidationState::ProductionPinned
            })
            .then_some(pinned);
    }
    [NativeBackend::OpenVino, NativeBackend::Vulkan]
        .into_iter()
        .find(|backend| {
            model.backends.iter().any(|capability| {
                capability.backend == *backend
                    && capability.validation == ValidationState::ProductionPinned
            })
        })
}

pub fn resolve_native_runtime(model_id: &str) -> Result<ResolvedNativeRuntime, RuntimeRouteError> {
    let model = native_runtime_registry()
        .into_iter()
        .find(|model| model.model_id == model_id)
        .ok_or_else(|| RuntimeRouteError::UnknownModel(model_id.to_string()))?;
    let backend = production_backend(&model)
        .ok_or_else(|| RuntimeRouteError::NoValidatedBackend(model_id.to_string()))?;
    if backend == NativeBackend::CpuReference {
        return Err(RuntimeRouteError::CpuProductionForbidden);
    }
    let executable = component_executable(&model.component_id)
        .ok_or_else(|| RuntimeRouteError::ComponentUnavailable(model.component_id.clone()))?;
    Ok(ResolvedNativeRuntime {
        model_id: model.model_id,
        component_id: model.component_id,
        backend,
        executable,
        runtime_recipe_digest: model.runtime_recipe_digest,
    })
}
