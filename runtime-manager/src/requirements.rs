use serde::{Deserialize, Serialize};

use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resource::ResourceRef;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementSet {
    pub schema: String,
    pub schema_version: u32,
    pub producer: String,
    pub resources: Vec<RequirementResource>,
}

impl RequirementSet {
    pub fn validate(&self) -> RuntimeManagerResult<()> {
        if self.schema != "uta.runtime.requirements" || self.schema_version != 1 {
            return Err(RuntimeManagerError::new(
                "invalid_requirements",
                "unsupported requirements schema",
            ));
        }
        if self.producer.trim().is_empty() {
            return Err(RuntimeManagerError::new(
                "invalid_requirements",
                "requirements producer must not be empty",
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for requirement in &self.resources {
            crate::resource::ResourceRef::new(
                requirement.resource.kind,
                requirement.resource.id.clone(),
            )
            .map_err(|error| RuntimeManagerError::new("invalid_requirements", error.message))?;
            if requirement.reason.trim().is_empty() || !seen.insert(requirement.resource.clone()) {
                return Err(RuntimeManagerError::new(
                    "invalid_requirements",
                    "requirement reasons must be non-empty and resources unique",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequirementResource {
    pub resource: ResourceRef,
    pub required: bool,
    pub reason: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resource::ResourceKind;

    #[test]
    fn validation_rejects_deserialized_path_escape_and_duplicates() {
        let requirement = RequirementResource {
            resource: ResourceRef {
                kind: ResourceKind::Model,
                id: "../rmvpe".to_string(),
            },
            required: true,
            reason: "pitch.track".to_string(),
        };
        let set = RequirementSet {
            schema: "uta.runtime.requirements".to_string(),
            schema_version: 1,
            producer: "fixture".to_string(),
            resources: vec![requirement],
        };
        assert_eq!(set.validate().unwrap_err().code, "invalid_requirements");
    }
}
