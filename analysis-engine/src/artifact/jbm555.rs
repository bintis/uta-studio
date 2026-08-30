use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::artifact::{
    TIMED_NOTE_EVIDENCE_CONTRACT, TIMED_NOTE_EVIDENCE_VERSION, TimedNoteExpertEvidenceV1,
    TimedNoteHypothesisV1,
};
use crate::contract::{EngineError, EngineErrorCode, EngineResult};
use crate::fusion::{EvidenceProvenance, ExpertTask, TimeRange};

pub const JBM555_MODEL_ID: &str = "jbm555_cectc_80";
pub const JBM555_FRONTEND_PROFILE: &str =
    "jbm555-native-logfft-44k1-hop1024-midi24-384x48-scales0.5-1-2-v1";
pub const JBM555_DECODE_PROFILE: &str = "jbm555-cectc-onset0.32-offset0.70-v1";
pub const JBM555_ONSET_THRESHOLD: f32 = 0.32;
pub const JBM555_OFFSET_THRESHOLD: f32 = 0.70;
const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;

/// Native-worker output before normalization. Both input identities are
/// mandatory because changing only the prepared vocal changes model evidence.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jbm555NoteEvidenceV1 {
    pub range: TimeRange,
    pub midi: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub onset_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_score: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pitch_score: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Jbm555EvidenceV1 {
    pub schema_version: u32,
    pub model_id: String,
    pub upstream_revision: String,
    pub checkpoint_identity: String,
    pub config_identity: String,
    pub conversion_identity: String,
    pub model_generation: String,
    pub runtime_identity: String,
    pub backend: String,
    pub source_start: u64,
    pub source_duration: u64,
    pub mix_audio_identity: String,
    pub vocal_audio_identity: String,
    pub separator_model_generation: String,
    pub vocal_preparation_generation: String,
    pub frontend_profile: String,
    pub decode_profile: String,
    pub onset_threshold: f32,
    pub offset_threshold: f32,
    pub notes: Vec<Jbm555NoteEvidenceV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jbm555ExpectedInputsV1<'a> {
    pub source_start: u64,
    pub source_duration: u64,
    pub mix_audio_identity: &'a str,
    pub vocal_audio_identity: &'a str,
    pub separator_model_generation: &'a str,
    pub vocal_preparation_generation: &'a str,
}

impl Jbm555EvidenceV1 {
    pub fn validate(&self) -> EngineResult<()> {
        let source_end = self
            .source_start
            .checked_add(self.source_duration)
            .ok_or_else(|| invalid("JBM555 source timeline overflows"))?;
        let identities = [
            self.upstream_revision.as_str(),
            self.checkpoint_identity.as_str(),
            self.config_identity.as_str(),
            self.conversion_identity.as_str(),
            self.model_generation.as_str(),
            self.runtime_identity.as_str(),
            self.backend.as_str(),
            self.mix_audio_identity.as_str(),
            self.vocal_audio_identity.as_str(),
            self.separator_model_generation.as_str(),
            self.vocal_preparation_generation.as_str(),
        ];
        if self.schema_version != 1
            || self.model_id != JBM555_MODEL_ID
            || self.source_duration == 0
            || identities.iter().any(|identity| !valid_identity(identity))
            || self.frontend_profile != JBM555_FRONTEND_PROFILE
            || self.decode_profile != JBM555_DECODE_PROFILE
            || (self.onset_threshold - JBM555_ONSET_THRESHOLD).abs() > f32::EPSILON
            || (self.offset_threshold - JBM555_OFFSET_THRESHOLD).abs() > f32::EPSILON
        {
            return Err(invalid(
                "JBM555 evidence identity or decode contract is invalid",
            ));
        }
        for note in &self.notes {
            if note.range.start < self.source_start
                || note.range.end > source_end
                || note.range.start >= note.range.end
                || [note.onset_score, note.offset_score, note.pitch_score]
                    .into_iter()
                    .flatten()
                    .any(|score| !score.is_finite() || !(0.0..=1.0).contains(&score))
            {
                return Err(invalid("JBM555 note event contract is invalid"));
            }
        }
        if self.notes.windows(2).any(|pair| {
            pair[0].range.start > pair[1].range.start
                || (pair[0].range.start == pair[1].range.start
                    && (pair[0].range.end, pair[0].midi) >= (pair[1].range.end, pair[1].midi))
        }) {
            return Err(invalid("JBM555 notes must be strictly ordered and unique"));
        }
        Ok(())
    }

    pub fn validate_expected_inputs(
        &self,
        expected: Jbm555ExpectedInputsV1<'_>,
    ) -> EngineResult<()> {
        self.validate()?;
        if self.source_start != expected.source_start
            || self.source_duration != expected.source_duration
            || self.mix_audio_identity != expected.mix_audio_identity
            || self.vocal_audio_identity != expected.vocal_audio_identity
            || self.separator_model_generation != expected.separator_model_generation
            || self.vocal_preparation_generation != expected.vocal_preparation_generation
        {
            return Err(invalid(
                "JBM555 evidence does not match the requested source inputs",
            ));
        }
        Ok(())
    }

    /// Identity for cache correlation; it deliberately changes when either the
    /// mix, prepared vocal, separator, or preparation generation changes.
    pub fn input_dependency_identity(&self) -> EngineResult<String> {
        self.validate()?;
        serde_json::to_string(&serde_json::json!({
            "mix": self.mix_audio_identity,
            "vocal": self.vocal_audio_identity,
            "separator": self.separator_model_generation,
            "preparation": self.vocal_preparation_generation,
        }))
        .map_err(|error| invalid(format!("could not encode JBM555 dependencies: {error}")))
    }

    pub fn timed_note_evidence(
        &self,
        expected: Jbm555ExpectedInputsV1<'_>,
    ) -> EngineResult<TimedNoteExpertEvidenceV1> {
        self.validate_expected_inputs(expected)?;
        let _dependency_identity = self.input_dependency_identity()?;
        let evidence = TimedNoteExpertEvidenceV1 {
            contract: TIMED_NOTE_EVIDENCE_CONTRACT.to_string(),
            version: TIMED_NOTE_EVIDENCE_VERSION,
            expert_id: self.model_id.clone(),
            model_generation: self.model_generation.clone(),
            backend: self.backend.clone(),
            notes: self
                .notes
                .iter()
                .map(|note| TimedNoteHypothesisV1 {
                    source_id: self.model_id.clone(),
                    range: note.range,
                    midi: Some(note.midi),
                    // The upstream decoder exposes separate onset and offset
                    // activations. Do not invent a cross-edge confidence by
                    // averaging them before a calibration study exists.
                    source_local_boundary_score: None,
                    source_local_pitch_score: note.pitch_score,
                    calibrated_boundary_confidence: None,
                    calibrated_pitch_confidence: None,
                })
                .collect(),
            provenance: EvidenceProvenance {
                expert_id: self.model_id.clone(),
                task: ExpertTask::NoteBoundary,
                model_hash: None,
                runtime_identity: Some(self.runtime_identity.clone()),
                calibration_version: None,
                correlation_group: Some("separator-conditioned:jbm555".to_string()),
                depends_on: vec![
                    format!("mix:{}", self.mix_audio_identity),
                    format!("vocal:{}", self.vocal_audio_identity),
                    format!("separator:{}", self.separator_model_generation),
                    format!("preparation:{}", self.vocal_preparation_generation),
                ],
            },
        };
        evidence.validate(self.source_start, self.source_duration)?;
        Ok(evidence)
    }
}

pub fn parse_jbm555_evidence(
    path: &Path,
    expected: Jbm555ExpectedInputsV1<'_>,
) -> EngineResult<Jbm555EvidenceV1> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("JBM555 evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("JBM555 evidence size is invalid"));
    }
    let evidence: Jbm555EvidenceV1 = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read JBM555 evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("JBM555 evidence JSON is invalid: {error}")))?;
    evidence.validate_expected_inputs(expected)?;
    Ok(evidence)
}

fn valid_identity(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> Jbm555EvidenceV1 {
        Jbm555EvidenceV1 {
            schema_version: 1,
            model_id: JBM555_MODEL_ID.to_string(),
            upstream_revision: "upstream-revision".to_string(),
            checkpoint_identity: "checkpoint-identity".to_string(),
            config_identity: "config-identity".to_string(),
            conversion_identity: "conversion-identity".to_string(),
            model_generation: "model-generation".to_string(),
            runtime_identity: "runtime-manifest".to_string(),
            backend: "openvino_cpu".to_string(),
            source_start: 1_000_000,
            source_duration: 500_000,
            mix_audio_identity: "mix-a".to_string(),
            vocal_audio_identity: "vocal-a".to_string(),
            separator_model_generation: "separator-a".to_string(),
            vocal_preparation_generation: "preparation-a".to_string(),
            frontend_profile: JBM555_FRONTEND_PROFILE.to_string(),
            decode_profile: JBM555_DECODE_PROFILE.to_string(),
            onset_threshold: JBM555_ONSET_THRESHOLD,
            offset_threshold: JBM555_OFFSET_THRESHOLD,
            notes: vec![
                Jbm555NoteEvidenceV1 {
                    range: TimeRange::new(1_000_000, 1_200_000).unwrap(),
                    midi: 69,
                    onset_score: Some(0.8),
                    offset_score: Some(0.9),
                    pitch_score: Some(0.7),
                },
                Jbm555NoteEvidenceV1 {
                    range: TimeRange::new(1_200_000, 1_500_000).unwrap(),
                    midi: 69,
                    onset_score: Some(0.75),
                    offset_score: Some(0.85),
                    pitch_score: Some(0.72),
                },
            ],
        }
    }

    fn expected_inputs(evidence: &Jbm555EvidenceV1) -> Jbm555ExpectedInputsV1<'_> {
        Jbm555ExpectedInputsV1 {
            source_start: evidence.source_start,
            source_duration: evidence.source_duration,
            mix_audio_identity: &evidence.mix_audio_identity,
            vocal_audio_identity: &evidence.vocal_audio_identity,
            separator_model_generation: &evidence.separator_model_generation,
            vocal_preparation_generation: &evidence.vocal_preparation_generation,
        }
    }

    #[test]
    fn dual_input_identity_and_same_pitch_attacks_survive_normalization() {
        let evidence = evidence();
        let first_identity = evidence.input_dependency_identity().unwrap();
        let normalized = evidence
            .timed_note_evidence(expected_inputs(&evidence))
            .unwrap();
        assert_eq!(normalized.notes.len(), 2);
        assert_eq!(normalized.notes[0].midi, normalized.notes[1].midi);
        assert_eq!(
            normalized.notes[0].range.end,
            normalized.notes[1].range.start
        );
        assert_eq!(normalized.provenance.depends_on.len(), 4);
        assert!(normalized.provenance.correlation_group.is_some());

        let mut changed_vocal = evidence;
        changed_vocal.vocal_audio_identity = "vocal-b".to_string();
        assert_ne!(
            first_identity,
            changed_vocal.input_dependency_identity().unwrap()
        );
    }

    #[test]
    fn empty_decoder_observation_remains_typed_evidence() {
        let mut evidence = evidence();
        evidence.notes.clear();
        assert!(evidence.validate().is_ok());
        assert!(
            evidence
                .timed_note_evidence(expected_inputs(&evidence))
                .unwrap()
                .notes
                .is_empty()
        );
    }

    #[test]
    fn mismatched_expected_inputs_fail_closed() {
        let evidence = evidence();
        let mismatched = Jbm555ExpectedInputsV1 {
            vocal_audio_identity: "vocal-b",
            ..expected_inputs(&evidence)
        };
        assert!(evidence.validate_expected_inputs(mismatched).is_err());
        assert!(evidence.timed_note_evidence(mismatched).is_err());
    }

    #[test]
    fn tail_clip_and_decoder_threshold_contract_fail_closed() {
        let mut tail_evidence = evidence();
        assert!(tail_evidence.validate().is_ok());
        tail_evidence.notes[1].range.end += 1;
        assert!(tail_evidence.validate().is_err());

        let mut threshold_evidence = evidence();
        threshold_evidence.onset_threshold = 0.31;
        assert!(threshold_evidence.validate().is_err());
    }
}
