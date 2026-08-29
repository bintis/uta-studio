use std::fmt;

use serde::{Deserialize, Serialize};

pub type EngineResult<T> = Result<T, EngineError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineErrorCode {
    UnsupportedContractVersion,
    InvalidContract,
    InvalidAudioRole,
    MultiplePrimarySources,
    MissingPrimarySource,
    MissingRequiredInput,
    DecodeFailed,
    TimelineInvalid,
    InvalidConstraints,
    MissingCapability,
    ModelUnavailable,
    RuntimeUnvalidated,
    RuntimeResolutionFailed,
    WorkerUnavailable,
    WorkerProtocolMismatch,
    WorkerTimeout,
    WorkerFailed,
    InferenceFailed,
    OutputValidationFailed,
    ExportFailed,
    Cancelled,
    InternalError,
}

impl EngineErrorCode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedContractVersion => "unsupported_contract_version",
            Self::InvalidContract => "invalid_contract",
            Self::InvalidAudioRole => "invalid_audio_role",
            Self::MultiplePrimarySources => "multiple_primary_sources",
            Self::MissingPrimarySource => "missing_primary_source",
            Self::MissingRequiredInput => "missing_required_input",
            Self::DecodeFailed => "decode_failed",
            Self::TimelineInvalid => "timeline_invalid",
            Self::InvalidConstraints => "invalid_constraints",
            Self::MissingCapability => "missing_capability",
            Self::ModelUnavailable => "model_unavailable",
            Self::RuntimeUnvalidated => "runtime_unvalidated",
            Self::RuntimeResolutionFailed => "runtime_resolution_failed",
            Self::WorkerUnavailable => "worker_unavailable",
            Self::WorkerProtocolMismatch => "worker_protocol_mismatch",
            Self::WorkerTimeout => "worker_timeout",
            Self::WorkerFailed => "worker_failed",
            Self::InferenceFailed => "inference_failed",
            Self::OutputValidationFailed => "output_validation_failed",
            Self::ExportFailed => "export_failed",
            Self::Cancelled => "cancelled",
            Self::InternalError => "internal_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineError {
    pub code: EngineErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capability: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    #[serde(default)]
    pub retryable: bool,
}

impl EngineError {
    pub fn new(code: EngineErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            request_id: None,
            capability: None,
            resource: None,
            retryable: false,
        }
    }

    pub fn for_request(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capability = Some(capability.into());
        self
    }

    pub fn with_resource(mut self, resource: impl ToString) -> Self {
        self.resource = Some(resource.to_string());
        self
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

impl From<uta_runtime_manager::RuntimeManagerError> for EngineError {
    fn from(error: uta_runtime_manager::RuntimeManagerError) -> Self {
        let tool_resource = error
            .resource
            .as_deref()
            .is_some_and(|resource| resource.starts_with("tool:"));
        let code = match error.code.as_str() {
            "resource_missing" if tool_resource => EngineErrorCode::WorkerUnavailable,
            "resource_missing" => EngineErrorCode::ModelUnavailable,
            "tool_unusable" => EngineErrorCode::WorkerUnavailable,
            "tool_protocol_mismatch" => EngineErrorCode::WorkerProtocolMismatch,
            "no_validated_backend" => EngineErrorCode::RuntimeUnvalidated,
            "worker_capability_missing" => EngineErrorCode::WorkerUnavailable,
            "runtime_missing" => EngineErrorCode::RuntimeResolutionFailed,
            _ => EngineErrorCode::RuntimeResolutionFailed,
        };
        let mut result = Self::new(code, error.message);
        result.resource = error.resource;
        result.retryable = error.retryable;
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_tool_failures_keep_resource_and_map_to_worker_codes() {
        let resource = uta_runtime_manager::ResourceRef::tool("fusion_agent_adapter").unwrap();
        for (runtime_code, expected) in [
            ("resource_missing", EngineErrorCode::WorkerUnavailable),
            ("tool_unusable", EngineErrorCode::WorkerUnavailable),
            (
                "tool_protocol_mismatch",
                EngineErrorCode::WorkerProtocolMismatch,
            ),
        ] {
            let runtime_error =
                uta_runtime_manager::RuntimeManagerError::new(runtime_code, "adapter is not ready")
                    .with_resource(&resource);
            let error = EngineError::from(runtime_error);
            assert_eq!(error.code, expected);
            assert_eq!(error.resource.as_deref(), Some("tool:fusion_agent_adapter"));
        }
    }
}
