use serde::{Deserialize, Serialize};

use super::NativeBackend;
use crate::backend_cli::{BackendCliError, RuntimeCliClient, RuntimePolicyWireV1};

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

/// Compatibility execution adapter over the Runtime Manager CLI resolver.
pub fn resolve_native_runtime(model_id: &str) -> Result<ResolvedNativeRuntime, RuntimeRouteError> {
    let client = RuntimeCliClient::discover()
        .map(|client| client.with_policy(RuntimePolicyWireV1::Experimental))
        .map_err(|_| RuntimeRouteError::NoValidatedBackend(model_id.to_string()))?;
    let resolved = client
        .resolve(model_id)
        .map_err(|error| map_error(error, model_id))?;
    let backend = super::registry::map_backend(resolved.backend);
    if backend == NativeBackend::CpuReference && resolved.policy == RuntimePolicyWireV1::Production
    {
        return Err(RuntimeRouteError::CpuProductionForbidden);
    }
    Ok(ResolvedNativeRuntime {
        model_id: resolved.resource.id().to_string(),
        component_id: resolved.runtime,
        backend,
        executable: resolved.runtime_executable,
        runtime_recipe_digest: resolved.runtime_recipe_digest,
    })
}

fn map_error(error: BackendCliError, model_id: &str) -> RuntimeRouteError {
    match error {
        BackendCliError::Domain { code, .. }
            if matches!(code.as_str(), "unknown_resource" | "invalid_resource") =>
        {
            RuntimeRouteError::UnknownModel(model_id.to_string())
        }
        BackendCliError::Domain { code, message, .. } if code == "runtime_missing" => {
            RuntimeRouteError::ComponentUnavailable(message)
        }
        _ => RuntimeRouteError::NoValidatedBackend(model_id.to_string()),
    }
}
