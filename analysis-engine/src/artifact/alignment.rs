use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{
    BoundaryAuthority, BoundaryLevel, EngineError, EngineErrorCode, EngineResult,
};

const MAX_ALIGNMENT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentArtifactV1 {
    pub contract: String,
    pub version: u32,
    pub transcript: String,
    pub language: Option<String>,
    pub items: Vec<AlignmentItemV1>,
    pub source_expert: String,
    pub model_sha256: String,
    pub runtime_manifest_sha256: String,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentItemV1 {
    pub id: String,
    pub text: String,
    pub level: BoundaryLevel,
    pub start: u64,
    pub duration: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub authority: BoundaryAuthority,
}

impl AlignmentArtifactV1 {
    pub fn validate(&self, source_start: u64, source_duration: u64) -> EngineResult<()> {
        let source_end = source_start
            .checked_add(source_duration)
            .ok_or_else(|| invalid("alignment source timeline overflows"))?;
        if self.contract != "uta.analysis-engine.alignment"
            || self.version != 1
            || self.transcript.trim().is_empty()
            || self.source_expert.trim().is_empty()
            || self.model_sha256.trim().is_empty()
            || self.runtime_manifest_sha256.trim().is_empty()
            || !matches!(self.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
            || self.items.is_empty()
        {
            return Err(invalid("alignment evidence identity is invalid"));
        }
        let mut ids = BTreeSet::new();
        let mut previous_end = source_start;
        for item in &self.items {
            let end = item
                .start
                .checked_add(item.duration)
                .ok_or_else(|| invalid("alignment item overflows"))?;
            if item.id.trim().is_empty()
                || item.text.trim().is_empty()
                || !ids.insert(item.id.as_str())
                || item.level != BoundaryLevel::Word
                || item.authority != BoundaryAuthority::Soft
                || item.duration == 0
                || item.start < previous_end
                || item.start < source_start
                || end > source_end
                || item
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                return Err(invalid("alignment item is invalid"));
            }
            previous_end = end;
        }
        Ok(())
    }
}

pub fn parse_alignment_artifact(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<AlignmentArtifactV1> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("alignment evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ALIGNMENT_BYTES {
        return Err(invalid("alignment evidence size is invalid"));
    }
    let artifact: AlignmentArtifactV1 = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read alignment evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("alignment evidence JSON is invalid: {error}")))?;
    artifact.validate(source_start, source_duration)?;
    Ok(artifact)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}
