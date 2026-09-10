use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePolicyWire {
    #[default]
    Production,
    Benchmark,
    Experimental,
}

impl RuntimePolicyWire {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Benchmark => "benchmark",
            Self::Experimental => "experimental",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeResourceKindWire {
    Model,
    Runtime,
    Tool,
    Bundle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeResourceRefWire(pub String);

impl RuntimeResourceRefWire {
    pub fn new(kind: RuntimeResourceKindWire, id: &str) -> Result<Self, String> {
        if id.is_empty()
            || id.contains("..")
            || id.contains(['/', '\\', ':'])
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
        {
            return Err(format!("invalid runtime resource id: {id}"));
        }
        let kind = match kind {
            RuntimeResourceKindWire::Model => "model",
            RuntimeResourceKindWire::Runtime => "runtime",
            RuntimeResourceKindWire::Tool => "tool",
            RuntimeResourceKindWire::Bundle => "bundle",
        };
        Ok(Self(format!("{kind}:{id}")))
    }

    pub fn model(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWire::Model, id)
    }
    pub fn runtime(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWire::Runtime, id)
    }
    pub fn tool(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWire::Tool, id)
    }
    pub fn bundle(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWire::Bundle, id)
    }
    pub fn id(&self) -> &str {
        self.0.split_once(':').map_or(self.0.as_str(), |(_, id)| id)
    }
}

impl fmt::Display for RuntimeResourceRefWire {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBackendWire {
    Ggml,
}

/// Device-class preference, orthogonal to `NativeBackendWire`. Hand-mirrors
/// the packaged runtime protocol's native-device-class field while preserving
/// this crate's convention of never importing the backend crate directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClassWire {
    Cpu,
    Gpu,
    IntegratedGpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStateWire {
    ProductionPinned,
    BenchmarkCandidate,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStateWire {
    Absent,
    Installed,
    Incomplete,
    Corrupt,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOriginWire {
    Missing,
    Managed,
    Legacy,
    EnvironmentOverride,
    ExternalConfiguration,
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessReasonWire {
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
pub struct RuntimeResourceStatusWire {
    pub resource: RuntimeResourceRefWire,
    pub install_state: InstallStateWire,
    pub origin: ResourceOriginWire,
    #[serde(default)]
    pub integrity_verified: bool,
    #[serde(default)]
    pub runnable: bool,
    pub validation_state: ValidationStateWire,
    pub dependencies_ready: bool,
    pub executable_ready: bool,
    pub usable: bool,
    #[serde(default)]
    pub reasons: Vec<ReadinessReasonWire>,
    #[serde(default)]
    pub selected_backend: Option<NativeBackendWire>,
    #[serde(default)]
    pub runtime_resource: Option<RuntimeResourceRefWire>,
    #[serde(default)]
    pub generation: Option<String>,
    #[serde(default)]
    pub tool_identity: Option<String>,
    #[serde(default)]
    pub tool_version: Option<String>,
    #[serde(default)]
    pub tool_protocol_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeBackendCapabilityWire {
    pub backend: NativeBackendWire,
    pub validation: ValidationStateWire,
    #[serde(default)]
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLicenseWire {
    pub status: String,
    pub source_attribution: String,
    #[serde(default)]
    pub source_page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResourceMetadataWire {
    pub display_name: String,
    pub purpose: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<RuntimeResourceRefWire>,
    #[serde(default)]
    pub backends: Vec<RuntimeBackendCapabilityWire>,
    #[serde(default)]
    pub license: Option<RuntimeLicenseWire>,
    #[serde(default)]
    pub estimated_download_bytes: Option<u64>,
    #[serde(default)]
    pub estimated_installed_bytes: Option<u64>,
    #[serde(default)]
    pub recipe_digest: Option<String>,
    #[serde(default)]
    pub runtime_recipe_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResourceDetailsWire {
    pub resource: RuntimeResourceRefWire,
    pub metadata: RuntimeResourceMetadataWire,
    pub status: RuntimeResourceStatusWire,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResolvedIdentityWire {
    pub resource: RuntimeResourceRefWire,
    pub generation: String,
    pub content_digest: String,
    pub model_recipe_digest: String,
    pub runtime: String,
    pub runtime_generation: String,
    pub runtime_content_digest: String,
    #[serde(default)]
    pub runtime_recipe_digest: Option<String>,
    pub runtime_executable: PathBuf,
    pub backend: NativeBackendWire,
    pub policy: RuntimePolicyWire,
    pub validation_state: ValidationStateWire,
    #[serde(default)]
    pub readiness_reasons: Vec<ReadinessReasonWire>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResolvedToolWire {
    pub resource: RuntimeResourceRefWire,
    pub executable: PathBuf,
    pub identity: String,
    pub version: String,
    pub protocol_version: u32,
    pub origin: ResourceOriginWire,
}

/// Runtime Manager's provider integration projection intentionally contains
/// no executable path or credential/authentication claim. A provider is
/// selectable only when its PATH CLI and sibling manifest-verified native
/// adapter are both present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFusionProviderStatusWire {
    pub provider: String,
    pub display_name: String,
    pub executable_name: String,
    pub available: bool,
    pub adapter_available: bool,
    pub usable: bool,
    pub selected: bool,
    #[serde(default)]
    pub reasons: Vec<String>,
    #[serde(default)]
    pub adapter_identity: Option<String>,
    #[serde(default)]
    pub adapter_version: Option<String>,
    #[serde(default)]
    pub protocol_version: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeFusionProviderReportWire {
    pub adapter_resource: String,
    pub selected_provider: Option<String>,
    pub providers: Vec<RuntimeFusionProviderStatusWire>,
    pub network_disclosure: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMutationResultWire {
    #[serde(default)]
    pub changed: Vec<RuntimeResourceRefWire>,
    #[serde(default)]
    pub unchanged: Vec<RuntimeResourceRefWire>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeResultEnvelope<T> {
    pub schema: String,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub command: String,
    pub status: String,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeErrorEnvelope {
    pub schema: String,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeEventEnvelope {
    pub schema: String,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub operation_id: String,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub resource: Option<RuntimeResourceRefWire>,
}
