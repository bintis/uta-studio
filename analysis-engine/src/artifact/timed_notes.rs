use serde::{Deserialize, Serialize};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{
    BoundaryAlternative, BoundaryEvidenceKind, EvidenceProvenance, ExpertTask, TimeRange,
};

pub const TIMED_NOTE_EVIDENCE_CONTRACT: &str = "uta.analysis-engine.timed-note-evidence";
pub const TIMED_NOTE_EVIDENCE_VERSION: u32 = 1;

/// Model-independent physical note hypothesis on the canonical timeline.
///
/// MIDI is optional because a physical-boundary expert can contribute useful
/// onset/offset evidence without claiming an authoritative pitch. Source-local
/// scores are never compared across model families as global probabilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedNoteHypothesisV1 {
    pub source_id: String,
    pub range: TimeRange,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub midi: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local_boundary_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_local_pitch_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_boundary_confidence: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calibrated_pitch_confidence: Option<f32>,
}

/// Normalized timed-note evidence consumed by Candidate Pool construction.
/// Raw worker contracts remain model-specific and must convert to this type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TimedNoteExpertEvidenceV1 {
    pub contract: String,
    pub version: u32,
    pub expert_id: String,
    pub model_generation: String,
    pub backend: String,
    pub notes: Vec<TimedNoteHypothesisV1>,
    pub provenance: EvidenceProvenance,
}

impl TimedNoteExpertEvidenceV1 {
    pub fn validate(&self, source_start: u64, source_duration: u64) -> EngineResult<()> {
        let source_end = source_start
            .checked_add(source_duration)
            .ok_or_else(|| invalid("timed-note source timeline overflows"))?;
        if source_duration == 0
            || self.contract != TIMED_NOTE_EVIDENCE_CONTRACT
            || self.version != TIMED_NOTE_EVIDENCE_VERSION
            || !identity(&self.expert_id)
            || !identity(&self.model_generation)
            || !identity(&self.backend)
            || self.provenance.expert_id != self.expert_id
            || self.provenance.task != ExpertTask::NoteBoundary
            || [
                self.provenance.model_hash.as_deref(),
                self.provenance.runtime_identity.as_deref(),
                self.provenance.calibration_version.as_deref(),
                self.provenance.correlation_group.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| !identity(value))
            || self
                .provenance
                .depends_on
                .iter()
                .any(|dependency| !identity(dependency))
        {
            return Err(invalid("timed-note evidence identity is invalid"));
        }
        let calibrated = self.notes.iter().any(|note| {
            note.calibrated_boundary_confidence.is_some()
                || note.calibrated_pitch_confidence.is_some()
        });
        if calibrated
            && self
                .provenance
                .calibration_version
                .as_deref()
                .is_none_or(|version| !identity(version))
        {
            return Err(invalid(
                "calibrated timed-note confidence requires a calibration identity",
            ));
        }
        for note in &self.notes {
            if note.source_id != self.expert_id
                || note.range.start < source_start
                || note.range.end > source_end
                || note.range.start >= note.range.end
                || note.midi.is_some_and(|midi| midi > 127)
                || [
                    note.source_local_boundary_score,
                    note.source_local_pitch_score,
                    note.calibrated_boundary_confidence,
                    note.calibrated_pitch_confidence,
                ]
                .into_iter()
                .flatten()
                .any(|score| !score.is_finite() || !(0.0..=1.0).contains(&score))
            {
                return Err(invalid("timed-note hypothesis is invalid"));
            }
        }
        if self.notes.windows(2).any(|pair| {
            (
                pair[0].range.start,
                pair[0].range.end,
                pair[0].midi,
                pair[0].source_id.as_str(),
            ) >= (
                pair[1].range.start,
                pair[1].range.end,
                pair[1].midi,
                pair[1].source_id.as_str(),
            )
        }) {
            return Err(invalid(
                "timed-note hypotheses must be strictly ordered and unique",
            ));
        }
        Ok(())
    }

    pub fn boundary_alternatives(
        &self,
        source_start: u64,
        source_duration: u64,
    ) -> EngineResult<Vec<BoundaryAlternative>> {
        self.validate(source_start, source_duration)?;
        Ok(self
            .notes
            .iter()
            .map(|note| BoundaryAlternative {
                source_expert: note.source_id.clone(),
                range: note.range,
                kind: BoundaryEvidenceKind::AdvancedNote,
                fractional_midi: note.midi.map(f32::from),
                source_local_score: note.source_local_boundary_score,
                source_local_pitch_score: note.source_local_pitch_score,
                calibrated_boundary_confidence: note.calibrated_boundary_confidence,
                calibrated_pitch_confidence: note.calibrated_pitch_confidence,
                hard: false,
            })
            .collect())
    }
}

fn identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> TimedNoteExpertEvidenceV1 {
        TimedNoteExpertEvidenceV1 {
            contract: TIMED_NOTE_EVIDENCE_CONTRACT.to_string(),
            version: TIMED_NOTE_EVIDENCE_VERSION,
            expert_id: "jbm555_cectc_80".to_string(),
            model_generation: "model-generation".to_string(),
            backend: "openvino_cpu".to_string(),
            notes: vec![
                TimedNoteHypothesisV1 {
                    source_id: "jbm555_cectc_80".to_string(),
                    range: TimeRange::new(100_000, 200_000).unwrap(),
                    midi: Some(69),
                    source_local_boundary_score: Some(0.8),
                    source_local_pitch_score: Some(0.7),
                    calibrated_boundary_confidence: None,
                    calibrated_pitch_confidence: None,
                },
                TimedNoteHypothesisV1 {
                    source_id: "jbm555_cectc_80".to_string(),
                    range: TimeRange::new(200_000, 260_000).unwrap(),
                    midi: Some(69),
                    source_local_boundary_score: Some(0.9),
                    source_local_pitch_score: Some(0.75),
                    calibrated_boundary_confidence: None,
                    calibrated_pitch_confidence: None,
                },
            ],
            provenance: EvidenceProvenance {
                expert_id: "jbm555_cectc_80".to_string(),
                task: ExpertTask::NoteBoundary,
                model_hash: Some("checkpoint".to_string()),
                runtime_identity: Some("runtime".to_string()),
                calibration_version: None,
                correlation_group: Some("separator-conditioned".to_string()),
                depends_on: vec!["mix-generation".to_string(), "vocal-generation".to_string()],
            },
        }
    }

    #[test]
    fn normalized_evidence_keeps_repeated_attacks_and_source_local_scores() {
        let evidence = evidence();
        let alternatives = evidence.boundary_alternatives(0, 1_000_000).unwrap();
        assert_eq!(alternatives.len(), 2);
        assert_eq!(alternatives[0].fractional_midi, Some(69.0));
        assert_eq!(alternatives[0].source_local_score, Some(0.8));
        assert_eq!(alternatives[0].source_local_pitch_score, Some(0.7));
        assert_eq!(alternatives[0].calibrated_boundary_confidence, None);
    }

    #[test]
    fn empty_observation_is_valid_negative_evidence() {
        let mut evidence = evidence();
        evidence.notes.clear();
        assert!(evidence.validate(0, 1_000_000).is_ok());
        assert!(evidence.validate(0, 0).is_err());
        assert!(
            evidence
                .boundary_alternatives(0, 1_000_000)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn mixed_source_attribution_fails_closed() {
        let mut evidence = evidence();
        evidence.notes[1].source_id = "another-expert".to_string();
        assert!(evidence.validate(0, 1_000_000).is_err());
    }

    #[test]
    fn calibrated_claims_require_identity_and_timeline_escape_fails_closed() {
        let mut evidence = evidence();
        evidence.notes[0].calibrated_pitch_confidence = Some(0.9);
        assert!(evidence.validate(0, 1_000_000).is_err());
        evidence.provenance.calibration_version = Some("calibration-v1".to_string());
        assert!(evidence.validate(0, 1_000_000).is_ok());
        evidence.notes[1].range.end = 1_000_001;
        assert!(evidence.validate(0, 1_000_000).is_err());
    }
}
