pub mod acquire;
pub mod catalog;
pub mod cli;
pub mod doctor;
pub mod error;
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
    ConvertedArtifactIdentity, LicenseInfo, ModelCatalogEntry, NativeBackend, NativeModelRuntime,
    ResourceCatalog, RuntimeCatalogEntry, SourceArtifactIdentity, SourceIdentity, ToolCatalogEntry,
};
pub use doctor::{DiagnosticCheck, DiagnosticSeverity, DoctorReport};
pub use error::{RuntimeManagerError, RuntimeManagerResult};
pub use install::{MutationOptions, MutationResult, PlannedResource, ResourcePlan};
pub use lease::ResourceLease;
pub use manifest::{InstallManifest, InstalledFile};
pub use requirements::{RequirementResource, RequirementSet};
pub use resolver::{ResolvedModel, ResourceDetails, ResourceMetadata, RuntimeManager};
pub use resource::{ModelId, ResourceKind, ResourceRef};
pub use runtime_lock::OPENVINO_WORKER_RECIPE_SHA256;
pub use runtime_lock::{
    GenericRuntimePolicyLock, NativeRuntimeLock, OpenVinoLock, QwenAlignLock, QwenAsrLock,
    RMVPE_CONVERSION_RECIPE_SHA256, RMVPE_IR_MANIFEST_SHA256, RMVPE_IR_RELATIVE_DIR,
    RMVPE_SOURCE_SHA256, RUNTIME_LOCK_JSON, RUNTIME_LOCK_SHA256, RuntimeComponents,
    RuntimePolicyLock, native_runtime_lock, runtime_recipe_digest,
};
pub use smoke::SmokeReport;
pub use state::{
    InstallState, ReadinessReason, ResourceOrigin, ResourceStatus, RuntimePolicy, ValidationState,
};
pub use store::StorePaths;
pub use verify::VerifyReport;
