use std::fmt;

use serde::{Deserialize, Serialize};

pub type RuntimeManagerResult<T> = Result<T, RuntimeManagerError>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RuntimeManagerError {
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub retryable: bool,
}

impl RuntimeManagerError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            resource: None,
            retryable: false,
        }
    }

    pub fn with_resource(mut self, resource: impl ToString) -> Self {
        self.resource = Some(resource.to_string());
        self
    }

    pub fn retryable(mut self) -> Self {
        self.retryable = true;
        self
    }

    pub fn invalid_resource_ref(message: impl Into<String>) -> Self {
        Self::new("invalid_resource", message)
    }

    pub fn unknown_resource(resource: impl ToString) -> Self {
        Self::new(
            "unknown_resource",
            format!("unknown resource: {}", resource.to_string()),
        )
        .with_resource(resource)
    }

    pub fn resource_missing(resource: impl ToString) -> Self {
        Self::new(
            "resource_missing",
            format!("resource is not ready: {}", resource.to_string()),
        )
        .with_resource(resource)
    }

    pub fn resource_corrupt(resource: impl ToString) -> Self {
        Self::new(
            "resource_corrupt",
            format!("resource integrity failed for {}", resource.to_string()),
        )
        .with_resource(resource)
    }

    pub fn no_validated_backend(resource: impl ToString) -> Self {
        Self::new(
            "no_validated_backend",
            format!(
                "no validated backend is available for {}",
                resource.to_string()
            ),
        )
        .with_resource(resource)
    }

    pub fn runtime_missing(resource: impl ToString) -> Self {
        Self::new(
            "runtime_missing",
            format!("runtime dependency is missing for {}", resource.to_string()),
        )
        .with_resource(resource)
    }

    pub fn worker_capability_missing(resource: impl ToString) -> Self {
        Self::new(
            "worker_capability_missing",
            format!("worker capability is missing for {}", resource.to_string()),
        )
        .with_resource(resource)
    }

    pub fn invalid_catalog(message: impl Into<String>) -> Self {
        Self::new("invalid_catalog", message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new("internal_error", message)
    }
}

impl fmt::Display for RuntimeManagerError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for RuntimeManagerError {}
