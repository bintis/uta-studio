use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::fingerprint::FINALIZE_VOCAL_CHART_VERSION;
use crate::fusion::{
    CanonicalLyrics, CanonicalNote, CanonicalSingingTrack, CanonicalWordBoundary,
    EvidenceProvenance, F0Point, HarmonyMetadata, validate_canonical_singing_track,
};
use crate::quantization::QuantizationReportV1;

pub const CANDIDATE_VOCAL_CHART_CONTRACT: &str = "uta.analysis-engine.candidate-vocal-chart";
pub const CANDIDATE_VOCAL_CHART_VERSION: u32 = 1;
pub const CANDIDATE_VOCAL_CHART_FORMAT_VERSION: &str = "0.3.0";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VocalChartAuthority {
    Candidate,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateVocalChartProvenanceV1 {
    pub execution_fingerprint: String,
    pub finalize_algorithm: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quantization: Option<QuantizationReportV1>,
    #[serde(default)]
    pub evidence: Vec<EvidenceProvenance>,
}

/// Stable Engine-owned Candidate chart. The authority type deliberately has no
/// authored variant: only Studio's explicit authoring workflow may create one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateVocalChartV1 {
    pub contract: String,
    pub version: u32,
    pub format_version: String,
    pub authority: VocalChartAuthority,
    pub timebase: u32,
    pub transcript: CanonicalLyrics,
    #[serde(default)]
    pub words: Vec<CanonicalWordBoundary>,
    #[serde(default)]
    pub notes: Vec<CanonicalNote>,
    /// Exact continuous evidence on the canonical timeline. This is never
    /// rhythm- or semitone-quantized by finalization.
    #[serde(default)]
    pub continuous_pitch: Vec<F0Point>,
    pub harmony_metadata: HarmonyMetadata,
    pub provenance: CandidateVocalChartProvenanceV1,
}

impl CandidateVocalChartV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != CANDIDATE_VOCAL_CHART_CONTRACT
            || self.version != CANDIDATE_VOCAL_CHART_VERSION
            || self.format_version != CANDIDATE_VOCAL_CHART_FORMAT_VERSION
            || self.timebase != CANONICAL_TIMEBASE
            || self.provenance.execution_fingerprint.trim().is_empty()
            || self.provenance.finalize_algorithm != FINALIZE_VOCAL_CHART_VERSION
            || self
                .provenance
                .quantization
                .as_ref()
                .is_some_and(|report| report.validate().is_err())
        {
            return Err(invalid(
                "Candidate VocalChart contract or provenance is invalid",
            ));
        }
        validate_canonical_singing_track(&CanonicalSingingTrack {
            schema_version: 1,
            transcript: self.transcript.clone(),
            words: self.words.clone(),
            notes: self.notes.clone(),
            f0_curve: self.continuous_pitch.clone(),
            harmony_metadata: self.harmony_metadata.clone(),
            provenance: self.provenance.evidence.clone(),
        })
        .map_err(invalid)?;
        if self
            .notes
            .windows(2)
            .any(|pair| pair[0].range.end > pair[1].range.start)
            || self.notes.iter().any(|note| {
                note.range.end <= note.range.start
                    || !note.center_pitch_hz.is_finite()
                    || note.center_pitch_hz <= 0.0
                    || !note.center_offset_cents.is_finite()
                    || note
                        .confidence
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            })
        {
            return Err(invalid(
                "Candidate VocalChart notes are invalid or overlapping",
            ));
        }
        if self
            .continuous_pitch
            .windows(2)
            .any(|pair| pair[0].time >= pair[1].time)
            || self.continuous_pitch.iter().any(|point| {
                !point.hz.is_finite()
                    || point.hz <= 0.0
                    || point
                        .confidence
                        .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
            })
        {
            return Err(invalid("Candidate VocalChart continuous pitch is invalid"));
        }
        Ok(())
    }
}

pub fn finalize_candidate_vocal_chart(
    track: &CanonicalSingingTrack,
    execution_fingerprint: &str,
    preserve_continuous_pitch: bool,
    quantization: Option<&QuantizationReportV1>,
) -> EngineResult<CandidateVocalChartV1> {
    if track.schema_version != 1 || execution_fingerprint.trim().is_empty() {
        return Err(invalid(
            "candidate graph version or execution fingerprint is invalid",
        ));
    }
    let mut notes = track.notes.clone();
    if !preserve_continuous_pitch {
        for note in &mut notes {
            note.f0_curve.clear();
            note.pitch_bend.clear();
        }
    }
    let chart = CandidateVocalChartV1 {
        contract: CANDIDATE_VOCAL_CHART_CONTRACT.to_string(),
        version: CANDIDATE_VOCAL_CHART_VERSION,
        format_version: CANDIDATE_VOCAL_CHART_FORMAT_VERSION.to_string(),
        authority: VocalChartAuthority::Candidate,
        timebase: CANONICAL_TIMEBASE,
        transcript: track.transcript.clone(),
        words: track.words.clone(),
        notes,
        continuous_pitch: if preserve_continuous_pitch {
            track.f0_curve.clone()
        } else {
            Vec::new()
        },
        harmony_metadata: track.harmony_metadata.clone(),
        provenance: CandidateVocalChartProvenanceV1 {
            execution_fingerprint: execution_fingerprint.to_string(),
            finalize_algorithm: FINALIZE_VOCAL_CHART_VERSION.to_string(),
            quantization: quantization.cloned(),
            evidence: track.provenance.clone(),
        },
    };
    chart.validate()?;
    Ok(chart)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::artifact::write_json_artifact;
    use crate::fusion::{
        AcousticCandidateFeatures, CanonicalNoteEvidence, LyricsAuthority, TechniqueScores,
        TimeRange,
    };

    fn track() -> CanonicalSingingTrack {
        let f0 = vec![
            F0Point {
                time: 100_001,
                hz: 439.73,
                confidence: Some(0.91),
            },
            F0Point {
                time: 110_001,
                hz: 440.18,
                confidence: Some(0.89),
            },
        ];
        CanonicalSingingTrack {
            schema_version: 1,
            transcript: CanonicalLyrics {
                text: "sing".to_string(),
                language: Some("en".to_string()),
                authority: LyricsAuthority::CallerCanonical,
                tokens: Vec::new(),
                confidence: None,
                source_experts: vec!["caller".to_string()],
                alternatives: Vec::new(),
            },
            words: Vec::new(),
            notes: vec![CanonicalNote {
                id: "note-1".to_string(),
                range: TimeRange::new(100_001, 500_003).unwrap(),
                midi_note: 69,
                center_pitch_hz: 439.95,
                center_offset_cents: -0.2,
                confidence: None,
                uncertain: false,
                alternatives: Vec::new(),
                f0_curve: f0.clone(),
                pitch_bend: Vec::new(),
                techniques: TechniqueScores::default(),
                word_id: None,
                evidence: CanonicalNoteEvidence {
                    source_experts: vec!["game".to_string(), "rmvpe".to_string()],
                    game_fractional_midi: 68.992,
                    game_boundary_decision_threshold: 0.2,
                    game_presence_decision_threshold: 0.2,
                    rmvpe_center_hz: Some(439.95),
                    rmvpe_confidence: Some(0.9),
                    rmvpe_cents_difference: Some(-0.2),
                    rmvpe_voiced_ratio: Some(1.0),
                    rmvpe_pitch_mad_cents: Some(0.2),
                    fcpe_center_hz: None,
                    fcpe_observed_ratio: None,
                    fcpe_pitch_mad_cents: None,
                    fcpe_cents_from_rmvpe: None,
                    fcpe_supports_rmvpe: None,
                    acoustic: Some(AcousticCandidateFeatures {
                        frame_count: 2,
                        mean_rms: 0.2,
                        mean_periodicity: 0.8,
                        mean_snr_db: 20.0,
                        onset_flux: Some(0.2),
                        preceding_flux: Some(0.01),
                        onset_supported: Some(true),
                        basic_pitch_onset_activation: None,
                        basic_pitch_onset_supported: None,
                    }),
                },
            }],
            f0_curve: f0,
            harmony_metadata: HarmonyMetadata::default(),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn finalization_is_candidate_only_and_preserves_exact_continuous_timing() {
        let track = track();
        let chart = finalize_candidate_vocal_chart(&track, &"a".repeat(64), true, None).unwrap();
        assert_eq!(chart.authority, VocalChartAuthority::Candidate);
        assert_eq!(chart.notes[0].range, track.notes[0].range);
        assert_eq!(chart.continuous_pitch, track.f0_curve);
        assert_eq!(chart.continuous_pitch[0].time, 100_001);
    }

    #[test]
    fn explicit_continuous_pitch_disable_does_not_change_note_timing() {
        let track = track();
        let chart = finalize_candidate_vocal_chart(&track, &"b".repeat(64), false, None).unwrap();
        assert!(chart.continuous_pitch.is_empty());
        assert!(chart.notes[0].f0_curve.is_empty());
        assert!(chart.notes[0].pitch_bend.is_empty());
        assert_eq!(chart.notes[0].range, track.notes[0].range);
    }

    #[test]
    fn finalized_artifact_has_stable_hash_and_byte_metadata() {
        let root = std::env::temp_dir().join(format!(
            "uta-candidate-chart-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let chart = finalize_candidate_vocal_chart(&track(), &"c".repeat(64), true, None).unwrap();
        let reference = write_json_artifact(
            &root,
            Path::new("candidate/vocal-chart.json"),
            "application/vnd.uta.vocal-chart+json;version=0.3",
            &chart,
        )
        .unwrap();
        assert!(reference.bytes > 0);
        assert_eq!(reference.sha256.len(), 64);
        let decoded: CandidateVocalChartV1 =
            serde_json::from_slice(&std::fs::read(root.join(reference.path)).unwrap()).unwrap();
        assert_eq!(decoded, chart);
        std::fs::remove_dir_all(root).unwrap();
    }
}
