use super::AnalysisEngine;
use crate::contract::{EngineError, EngineErrorCode, EngineResult, ExportRequest};

impl AnalysisEngine {
    pub fn export(&self, request: &ExportRequest) -> EngineResult<()> {
        request.validate()?;
        Err(EngineError::new(
            EngineErrorCode::ExportFailed,
            "standalone representation export is not implemented in this build",
        )
        .for_request(&request.request_id))
    }
}
