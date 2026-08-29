use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePolicyWireV1 {
    #[default]
    Production,
    Benchmark,
    Experimental,
}

impl RuntimePolicyWireV1 {
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
pub enum RuntimeResourceKindWireV1 {
    Model,
    Runtime,
    Tool,
    Bundle,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RuntimeResourceRefWireV1(pub String);

impl RuntimeResourceRefWireV1 {
    pub fn new(kind: RuntimeResourceKindWireV1, id: &str) -> Result<Self, String> {
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
            RuntimeResourceKindWireV1::Model => "model",
            RuntimeResourceKindWireV1::Runtime => "runtime",
            RuntimeResourceKindWireV1::Tool => "tool",
            RuntimeResourceKindWireV1::Bundle => "bundle",
        };
        Ok(Self(format!("{kind}:{id}")))
    }

    pub fn model(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWireV1::Model, id)
    }
    pub fn runtime(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWireV1::Runtime, id)
    }
    pub fn tool(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWireV1::Tool, id)
    }
    pub fn bundle(id: &str) -> Result<Self, String> {
        Self::new(RuntimeResourceKindWireV1::Bundle, id)
    }
    pub fn id(&self) -> &str {
        self.0.split_once(':').map_or(self.0.as_str(), |(_, id)| id)
    }
}

impl fmt::Display for RuntimeResourceRefWireV1 {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeBackendWireV1 {
    OpenVino,
    Vulkan,
    NativeDsp,
    CpuReference,
}

/// Device-class preference, orthogonal to `NativeBackendWireV1`. Hand-mirrors
/// the packaged runtime protocol's native-device-class field while preserving
/// this crate's convention of never importing the backend crate directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceClassWireV1 {
    Cpu,
    Gpu,
    IntegratedGpu,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationStateWireV1 {
    ProductionPinned,
    BenchmarkCandidate,
    Experimental,
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallStateWireV1 {
    Absent,
    Installed,
    Incomplete,
    Corrupt,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceOriginWireV1 {
    Missing,
    Managed,
    Legacy,
    EnvironmentOverride,
    ExternalConfiguration,
    Derived,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReadinessReasonWireV1 {
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
pub struct RuntimeResourceStatusWireV1 {
    pub resource: RuntimeResourceRefWireV1,
    pub install_state: InstallStateWireV1,
    pub origin: ResourceOriginWireV1,
    #[serde(default)]
    pub integrity_verified: bool,
    #[serde(default)]
    pub runnable: bool,
    pub validation_state: ValidationStateWireV1,
    pub dependencies_ready: bool,
    pub executable_ready: bool,
    pub usable: bool,
    #[serde(default)]
    pub reasons: Vec<ReadinessReasonWireV1>,
    #[serde(default)]
    pub selected_backend: Option<NativeBackendWireV1>,
    #[serde(default)]
    pub runtime_resource: Option<RuntimeResourceRefWireV1>,
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
pub struct RuntimeBackendCapabilityWireV1 {
    pub backend: NativeBackendWireV1,
    pub validation: ValidationStateWireV1,
    #[serde(default)]
    pub evidence_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeLicenseWireV1 {
    pub status: String,
    pub source_attribution: String,
    #[serde(default)]
    pub source_page: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResourceMetadataWireV1 {
    pub display_name: String,
    pub purpose: String,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub dependencies: Vec<RuntimeResourceRefWireV1>,
    #[serde(default)]
    pub backends: Vec<RuntimeBackendCapabilityWireV1>,
    #[serde(default)]
    pub license: Option<RuntimeLicenseWireV1>,
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
pub struct RuntimeResourceDetailsWireV1 {
    pub resource: RuntimeResourceRefWireV1,
    pub metadata: RuntimeResourceMetadataWireV1,
    pub status: RuntimeResourceStatusWireV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResolvedIdentityWireV1 {
    pub resource: RuntimeResourceRefWireV1,
    pub generation: String,
    pub content_digest: String,
    pub model_recipe_digest: String,
    pub runtime: String,
    pub runtime_generation: String,
    pub runtime_content_digest: String,
    #[serde(default)]
    pub runtime_recipe_digest: Option<String>,
    pub runtime_executable: PathBuf,
    pub backend: NativeBackendWireV1,
    pub policy: RuntimePolicyWireV1,
    pub validation_state: ValidationStateWireV1,
    #[serde(default)]
    pub readiness_reasons: Vec<ReadinessReasonWireV1>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeResolvedToolWireV1 {
    pub resource: RuntimeResourceRefWireV1,
    pub executable: PathBuf,
    pub identity: String,
    pub version: String,
    pub protocol_version: u32,
    pub origin: ResourceOriginWireV1,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeMutationResultWireV1 {
    #[serde(default)]
    pub changed: Vec<RuntimeResourceRefWireV1>,
    #[serde(default)]
    pub unchanged: Vec<RuntimeResourceRefWireV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeResultEnvelopeV1<T> {
    pub schema: String,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub command: String,
    pub status: String,
    pub data: T,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeErrorEnvelopeV1 {
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
pub struct RuntimeEventEnvelopeV1 {
    pub schema: String,
    pub schema_version: u32,
    #[serde(rename = "type")]
    pub frame_type: String,
    pub operation_id: String,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub resource: Option<RuntimeResourceRefWireV1>,
}
