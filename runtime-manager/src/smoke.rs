use serde::{Deserialize, Serialize};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::{ResourceKind, ResourceRef};
use crate::state::RuntimePolicy;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SmokeReport {
    pub schema: String,
    pub schema_version: u32,
    pub resource: ResourceRef,
    pub executed: bool,
    pub message: String,
}

impl RuntimeManager {
    /// Smoke is deliberately offline. A resource without a registered native
    /// deterministic fixture fails rather than pretending that status/resolve
    /// is inference evidence.
    pub fn smoke(
        &self,
        resource: &ResourceRef,
        policy: RuntimePolicy,
    ) -> RuntimeManagerResult<SmokeReport> {
        if resource.kind != ResourceKind::Model {
            return Err(RuntimeManagerError::new(
                "smoke_failed",
                "smoke currently accepts model resources only",
            )
            .with_resource(resource));
        }
        let status = self.status(resource, policy)?;
        if !status.usable {
            return Err(RuntimeManagerError::resource_missing(resource));
        }
        Err(RuntimeManagerError::new(
            "smoke_failed",
            "no deterministic native smoke fixture is registered for this resource",
        )
        .with_resource(resource))
    }
}
