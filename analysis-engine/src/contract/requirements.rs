use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use uta_runtime_manager::{RequirementResource, RequirementSet, ResourceRef};

use super::{EngineError, EngineErrorCode, EngineResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRequirements {
    pub schema: String,
    pub schema_version: u32,
    pub producer: String,
    pub resources: Vec<EngineRequirementResource>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineRequirementResource {
    pub resource: String,
    pub required: bool,
    pub reason: String,
}

impl EngineRequirements {
    pub fn new(resources: Vec<EngineRequirementResource>) -> Self {
        Self {
            schema: "uta.runtime.requirements".to_string(),
            schema_version: 1,
            producer: "uta-analysis-engine".to_string(),
            resources,
        }
    }

    pub fn validate(&self) -> EngineResult<()> {
        if self.schema != "uta.runtime.requirements"
            || self.schema_version != 1
            || self.producer != "uta-analysis-engine"
        {
            return Err(EngineError::new(
                EngineErrorCode::InvalidContract,
                "invalid Engine requirements identity",
            ));
        }
        let mut seen = BTreeSet::new();
        for requirement in &self.resources {
            let _: ResourceRef = requirement.resource.parse().map_err(|error| {
                EngineError::new(
                    EngineErrorCode::InvalidContract,
                    format!("invalid requirement resource: {error}"),
                )
            })?;
            if requirement.reason.is_empty() || !seen.insert(&requirement.resource) {
                return Err(EngineError::new(
                    EngineErrorCode::InvalidContract,
                    "requirements contain an empty reason or duplicate resource",
                ));
            }
        }
        Ok(())
    }

    pub fn runtime_manager_set(&self) -> EngineResult<RequirementSet> {
        self.validate()?;
        Ok(RequirementSet {
            schema: self.schema.clone(),
            schema_version: self.schema_version,
            producer: self.producer.clone(),
            resources: self
                .resources
                .iter()
                .map(|requirement| {
                    Ok(RequirementResource {
                        resource: requirement.resource.parse().map_err(|error| {
                            EngineError::new(
                                EngineErrorCode::InvalidContract,
                                format!("invalid requirement resource: {error}"),
                            )
                        })?,
                        required: requirement.required,
                        reason: requirement.reason.clone(),
                    })
                })
                .collect::<EngineResult<Vec<_>>>()?,
        })
    }
}
