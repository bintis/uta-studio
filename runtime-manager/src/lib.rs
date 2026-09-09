pub mod acquire;
pub mod catalog;
pub mod cli;
pub mod doctor;
pub mod error;
pub mod external_tool;
pub mod fusion_provider;
pub mod install;
pub mod lease;
pub mod manifest;
pub mod platform;
pub mod requirements;
pub mod resolver;
pub mod resource;
pub mod runtime_lock;
pub mod state;
pub mod store;
pub mod verify;

pub mod convert {}
pub mod repair {}
pub mod smoke;

pub use acquire::{AcquisitionTransport, HttpAcquisitionTransport};
pub use catalog::{
    AcquisitionMethod, AcquisitionSpec, AlgorithmIdentity, BackendCapability, BundleCatalogEntry,
    ConvertedArtifactIdentity, LicenseInfo, ModelArtifactSpec, ModelCatalogEntry, NativeBackend,
    NativeDeviceClass, NativeModelRuntime, ResourceCatalog, RuntimeCatalogEntry,
    SourceArtifactIdentity, SourceIdentity, ToolCatalogEntry,
};
pub use doctor::{DiagnosticCheck, DiagnosticSeverity, DoctorReport};
pub use error::{RuntimeManagerError, RuntimeManagerResult};
pub use external_tool::{
    FUSION_AGENT_ADAPTER_ID, FUSION_AGENT_ADAPTER_MANIFEST_CONTRACT, FUSION_AGENT_PROTOCOL_VERSION,
    FusionAgentAdapterManifestV1, fusion_adapter_manifest_path,
};
pub use fusion_provider::{
    FUSION_PROVIDER_NETWORK_DISCLOSURE, FusionProviderReport, FusionProviderStatus,
};
pub use install::{MutationOptions, MutationResult, PlannedResource, ResourcePlan};
pub use lease::ResourceLease;
pub use manifest::{InstallManifest, InstalledFile};
pub use requirements::{RequirementResource, RequirementSet};
pub use resolver::{
    ResolvedModel, ResolvedTool, ResourceDetails, ResourceMetadata, RuntimeManager,
};
pub use resource::{ModelId, ResourceKind, ResourceRef};
pub use runtime_lock::{
    GGML_RUNTIME_RECIPE_SHA256, NativeRuntimeLock, RMVPE_GGUF_SHA256, RMVPE_GGUF_SIZE_BYTES,
    RUNTIME_LOCK_JSON, RuntimePolicyLock, native_runtime_lock, runtime_recipe_digest,
};
pub use smoke::SmokeReport;
pub use state::{
    InstallState, ReadinessReason, ResourceOrigin, ResourceStatus, RuntimePolicy, ValidationState,
};
pub use store::StorePaths;
pub use verify::VerifyReport;
