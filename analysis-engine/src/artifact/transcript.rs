use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};

const MAX_TRANSCRIPT_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAuthority {
    CallerCanonical,
    #[default]
    Generated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptArtifact {
    pub contract: String,
    pub version: u32,
    #[serde(default)]
    pub authority: TranscriptAuthority,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub text: String,
    #[serde(default)]
    pub tokens: Vec<TranscriptToken>,
    /// Coarse ASR audio scopes, indexed in non-whitespace transcript characters.
    /// They locate local alignment requests; they are not word boundaries.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub audio_segments: Vec<TranscriptAudioSegment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub source_experts: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_manifest_sha256: Option<String>,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptToken {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptAudioSegment {
    pub start: u64,
    pub duration: u64,
    pub text_start: usize,
    pub text_end: usize,
}

impl TranscriptArtifact {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != "uta.analysis-engine.transcript"
            || self.version != 1
            || self.text.trim().is_empty()
            || self.source_experts.is_empty()
            || self
                .source_experts
                .iter()
                .any(|source| source.trim().is_empty())
            || self.backend.trim().is_empty()
            || self
                .confidence
                .is_some_and(|value| !valid_confidence(value))
            || self.tokens.iter().any(|token| {
                token.id.trim().is_empty()
                    || token.text.trim().is_empty()
                    || token
                        .confidence
                        .is_some_and(|value| !valid_confidence(value))
            })
        {
            return Err(invalid("transcript artifact is invalid"));
        }
        match self.authority {
            TranscriptAuthority::CallerCanonical => {
                if self.confidence.is_some()
                    || self.model_sha256.is_some()
                    || self.runtime_manifest_sha256.is_some()
                    || self.backend != "caller"
                {
                    return Err(invalid(
                        "caller-canonical transcript must not claim model confidence or provenance",
                    ));
                }
            }
            TranscriptAuthority::Generated => {
                if self.model_sha256.is_none() || self.runtime_manifest_sha256.is_none() {
                    return Err(invalid("generated transcript provenance is incomplete"));
                }
            }
        }
        let characters = self
            .text
            .chars()
            .filter(|character| !character.is_whitespace())
            .count();
        let mut text_end = 0;
        for segment in &self.audio_segments {
            if segment.duration == 0
                || segment.start.checked_add(segment.duration).is_none()
                || segment.text_start != text_end
                || segment.text_end <= text_end
                || segment.text_end > characters
            {
                return Err(invalid("transcript audio/text anchor is invalid"));
            }
            text_end = segment.text_end;
        }
        if !self.audio_segments.is_empty() && text_end != characters {
            return Err(invalid("transcript audio anchors do not cover its text"));
        }
        Ok(())
    }
}

pub fn parse_transcript_artifact(path: &Path) -> EngineResult<TranscriptArtifact> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("transcript evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_TRANSCRIPT_BYTES {
        return Err(invalid("transcript evidence size is invalid"));
    }
    let artifact: TranscriptArtifact = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read transcript evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("transcript evidence JSON is invalid: {error}")))?;
    artifact.validate()?;
    Ok(artifact)
}

fn valid_confidence(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}
