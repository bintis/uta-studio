use serde::{Deserialize, Serialize};

use crate::error::RuntimeManagerResult;
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::state::{InstallState, ResourceOrigin, ResourceStatus, RuntimePolicy};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyReport {
    pub schema: String,
    pub schema_version: u32,
    pub checked: Vec<ResourceStatus>,
    pub corrupt: Vec<ResourceRef>,
    pub incomplete: Vec<ResourceRef>,
}

pub fn verify_status(
    manager: &RuntimeManager,
    resources: &[ResourceRef],
    policy: RuntimePolicy,
) -> RuntimeManagerResult<Vec<ResourceStatus>> {
    let targets = if resources.is_empty() {
        manager
            .list(policy)?
            .into_iter()
            .filter(|status| status.origin == ResourceOrigin::Managed)
            .map(|status| status.resource)
            .collect::<Vec<_>>()
    } else {
        resources.to_vec()
    };
    targets
        .iter()
        .map(|resource| manager.verified_status(resource, policy))
        .collect()
}

pub fn verify_report(
    manager: &RuntimeManager,
    resources: &[ResourceRef],
    policy: RuntimePolicy,
) -> RuntimeManagerResult<VerifyReport> {
    let checked = verify_status(manager, resources, policy)?;
    let corrupt = checked
        .iter()
        .filter(|status| status.install_state == InstallState::Corrupt)
        .map(|status| status.resource.clone())
        .collect();
    let incomplete = checked
        .iter()
        .filter(|status| status.install_state == InstallState::Incomplete)
        .map(|status| status.resource.clone())
        .collect();
    Ok(VerifyReport {
        schema: "uta.runtime.verify".to_string(),
        schema_version: 1,
        checked,
        corrupt,
        incomplete,
    })
}
