use std::collections::BTreeSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{
    BoundaryAuthority, BoundaryLevel, EngineError, EngineErrorCode, EngineResult,
};

const MAX_ALIGNMENT_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentArtifact {
    pub contract: String,
    pub version: u32,
    pub transcript: String,
    pub language: Option<String>,
    pub items: Vec<AlignmentItem>,
    pub source_expert: String,
    pub model_sha256: String,
    pub runtime_manifest_sha256: String,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentItem {
    pub id: String,
    pub text: String,
    pub level: BoundaryLevel,
    pub start: u64,
    pub duration: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub authority: BoundaryAuthority,
    /// Unresolved items keep their text and audition scope. Their start/duration
    /// MUST NOT be interpreted as measured word boundaries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timing_issue: Option<String>,
}

impl AlignmentArtifact {
    pub fn measured_items(&self) -> impl Iterator<Item = &AlignmentItem> {
        self.items.iter().filter(|item| item.timing_issue.is_none())
    }
    /// Search scopes describe playable audio, not measured word boundaries.
    /// Resampling can leave their final edge beyond the original source by a
    /// fraction of a sample; intersect them with the actual source timeline.
    fn intersect_unresolved_scopes(
        &mut self,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<()> {
        let source_end = source_start
            .checked_add(source_duration)
            .ok_or_else(|| invalid("alignment source timeline overflows"))?;
        for item in self
            .items
            .iter_mut()
            .filter(|item| item.timing_issue.is_some())
        {
            let end = item
                .start
                .checked_add(item.duration)
                .ok_or_else(|| invalid("alignment item overflows"))?;
            let start = item.start.max(source_start);
            let end = end.min(source_end);
            if end <= start {
                return Err(invalid(
                    "alignment search scope does not overlap the source",
                ));
            }
            item.start = start;
            item.duration = end - start;
        }
        Ok(())
    }

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
            || !matches!(
                self.backend.as_str(),
                "ggml_cpu" | "ggml_vulkan" | "libtorch_xpu"
            )
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
                || (item.timing_issue.is_none() && item.start < previous_end)
                || item
                    .timing_issue
                    .as_ref()
                    .is_some_and(|issue| issue.trim().is_empty())
                || item.start < source_start
                || end > source_end
                || item
                    .confidence
                    .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            {
                return Err(invalid("alignment item is invalid"));
            }
            if item.timing_issue.is_none() {
                previous_end = end;
            }
        }
        Ok(())
    }
}

pub fn parse_alignment_artifact(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<AlignmentArtifact> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("alignment evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_ALIGNMENT_BYTES {
        return Err(invalid("alignment evidence size is invalid"));
    }
    let mut artifact: AlignmentArtifact = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read alignment evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("alignment evidence JSON is invalid: {error}")))?;
    artifact.intersect_unresolved_scopes(source_start, source_duration)?;
    artifact.validate(source_start, source_duration)?;
    Ok(artifact)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tail_alignment(source_start: u64) -> AlignmentArtifact {
        AlignmentArtifact {
            contract: "uta.analysis-engine.alignment".to_string(),
            version: 1,
            transcript: "pa ako".to_string(),
            language: Some("tl".to_string()),
            items: vec![
                AlignmentItem {
                    id: "measured".to_string(),
                    text: "pa".to_string(),
                    level: BoundaryLevel::Word,
                    start: source_start + 29_600_000,
                    duration: 720_000,
                    confidence: None,
                    authority: BoundaryAuthority::Soft,
                    timing_issue: None,
                },
                AlignmentItem {
                    id: "pending".to_string(),
                    text: "ako".to_string(),
                    level: BoundaryLevel::Word,
                    start: source_start + 30_000_000,
                    duration: 3_212_250,
                    confidence: None,
                    authority: BoundaryAuthority::Soft,
                    timing_issue: Some("window_edge_pending".to_string()),
                },
            ],
            source_expert: "qwen3_forced_aligner_0_6b".to_string(),
            model_sha256: "fixture".to_string(),
            runtime_manifest_sha256: "fixture".to_string(),
            backend: "libtorch_xpu".to_string(),
        }
    }

    #[test]
    fn resampled_tail_scope_keeps_lyrics_inside_original_audio() {
        // Vocadito: 1,464,660 source frames at 44.1 kHz end at 33,212,245 us.
        // Its 531,396 resampled frames at 16 kHz end at 33,212,250 us.
        for source_start in [0, 5_000_000] {
            let mut artifact = tail_alignment(source_start);
            let original = artifact.clone();
            assert!(artifact.validate(source_start, 33_212_245).is_err());
            artifact
                .intersect_unresolved_scopes(source_start, 33_212_245)
                .unwrap();
            artifact.validate(source_start, 33_212_245).unwrap();
            assert_eq!(artifact.items[0], original.items[0]);
            let mut expected = original;
            expected.items[1].duration = 3_212_245;
            assert_eq!(artifact, expected);
            assert_eq!(artifact.measured_items().count(), 1);
        }
    }

    #[test]
    fn scopes_intersect_source_start_without_inventing_measured_timing() {
        let source_start = 5_000_000;
        let mut artifact = tail_alignment(source_start);
        artifact.items[1].start = source_start - 5;
        artifact.items[1].duration = 33_212_255;
        artifact
            .intersect_unresolved_scopes(source_start, 33_212_245)
            .unwrap();
        artifact.validate(source_start, 33_212_245).unwrap();
        assert_eq!(artifact.items[1].start, source_start);
        assert_eq!(artifact.items[1].duration, 33_212_245);
        assert_eq!(
            artifact.items[1].timing_issue.as_deref(),
            Some("window_edge_pending")
        );
    }

    #[test]
    fn measured_out_of_source_word_is_still_rejected_without_clipping() {
        let mut artifact = tail_alignment(0);
        artifact.items[1].start = 31_000_000;
        artifact.items[1].duration = 2_212_250;
        artifact.items[1].timing_issue = None;
        let original = artifact.clone();
        artifact.intersect_unresolved_scopes(0, 33_212_245).unwrap();
        assert_eq!(artifact, original);
        assert!(artifact.validate(0, 33_212_245).is_err());
    }

    #[test]
    fn empty_disjoint_and_overflowing_search_scopes_still_fail() {
        for (start, duration) in [(33_212_245, 5), (30_000_000, 0), (u64::MAX, 1)] {
            let mut artifact = tail_alignment(0);
            artifact.items[1].start = start;
            artifact.items[1].duration = duration;
            assert!(artifact.intersect_unresolved_scopes(0, 33_212_245).is_err());
        }
        assert!(
            tail_alignment(0)
                .intersect_unresolved_scopes(u64::MAX, 1)
                .is_err()
        );
    }
}
