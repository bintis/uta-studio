use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::catalog::NativeBackend;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resource::ResourceRef;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallState {
    Absent,
    Installed,
    Incomplete,
    Corrupt,
    Legacy,
}

impl InstallState {
    pub fn locally_present(self) -> bool {
        matches!(self, Self::Installed | Self::Legacy)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOrigin {
    Missing,
    Managed,
    Legacy,
    EnvironmentOverride,
    ExternalConfiguration,
    Derived,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationState {
    ProductionPinned,
    BenchmarkCandidate,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePolicy {
    #[default]
    Production,
    Benchmark,
    Experimental,
}

impl FromStr for RuntimePolicy {
    type Err = RuntimeManagerError;

    fn from_str(value: &str) -> RuntimeManagerResult<Self> {
        match value {
            "production" => Ok(Self::Production),
            "benchmark" => Ok(Self::Benchmark),
            "experimental" => Ok(Self::Experimental),
            other => Err(RuntimeManagerError::new(
                "invalid_policy",
                format!("unknown runtime policy: {other}"),
            )),
        }
    }
}

impl RuntimePolicy {
    pub fn allows(self, validation: ValidationState) -> bool {
        match (self, validation) {
            (Self::Production, ValidationState::ProductionPinned) => true,
            (Self::Production, _) => false,
            (
                Self::Benchmark,
                ValidationState::ProductionPinned | ValidationState::BenchmarkCandidate,
            ) => true,
            (Self::Benchmark, _) => false,
            // Experimental admits explicitly experimental routes while retaining
            // the invariant that Unsupported can never resolve.
            (Self::Experimental, ValidationState::Unsupported) => false,
            (Self::Experimental, _) => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessReason {
    UnknownResource,
    Absent,
    Incomplete,
    Corrupt,
    Legacy,
    DependencyMissing,
    RuntimeMissing,
    ExecutableMissing,
    WorkerCapabilityMissing,
    ProtocolMismatch,
    BackendUnvalidated,
    CpuProductionForbidden,
    UnsupportedPlatform,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceStatus {
    pub resource: ResourceRef,
    pub install_state: InstallState,
    pub origin: ResourceOrigin,
    /// True only when a managed generation passed manifest and content verification.
    #[serde(default)]
    pub integrity_verified: bool,
    /// True when local files, dependencies, and worker capability can execute,
    /// independently of the selected validation policy.
    #[serde(default)]
    pub runnable: bool,
    pub validation_state: ValidationState,
    pub dependencies_ready: bool,
    pub executable_ready: bool,
    pub usable: bool,
    pub reasons: Vec<ReadinessReason>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub selected_backend: Option<NativeBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_resource: Option<ResourceRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_identity: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_protocol_version: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_policy_matches_contract() {
        assert!(RuntimePolicy::Production.allows(ValidationState::ProductionPinned));
        assert!(!RuntimePolicy::Production.allows(ValidationState::BenchmarkCandidate));
        assert!(RuntimePolicy::Benchmark.allows(ValidationState::BenchmarkCandidate));
        assert!(!RuntimePolicy::Benchmark.allows(ValidationState::Experimental));
        assert!(RuntimePolicy::Experimental.allows(ValidationState::Experimental));
        assert!(!RuntimePolicy::Production.allows(ValidationState::Unsupported));
        assert!(!RuntimePolicy::Benchmark.allows(ValidationState::Unsupported));
        assert!(!RuntimePolicy::Experimental.allows(ValidationState::Unsupported));
    }
}
