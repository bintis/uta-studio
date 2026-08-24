mod alignment_fusion;
mod baseline;
mod calibration;
mod canonical;
mod evidence;
mod hsmm;
mod review;
mod scalar;
mod transcript_fusion;
mod types;

pub use alignment_fusion::{CanonicalWordBoundary, WordBoundaryEvidence, fuse_word_boundaries};
pub use baseline::{PitchGrid, SingingFusionEvidence, fuse_singing_evidence};
pub use calibration::{CalibrationMethod, ScoreCalibrator};
pub use canonical::{
    CanonicalNote, CanonicalNoteEvidence, CanonicalSingingTrack, F0Point, HarmonyMetadata,
    PitchBendPoint, build_canonical_singing_track, validate_canonical_singing_track,
};
pub use evidence::{EvidenceFrame, EvidenceSeries, ScalarEvidence};
pub use hsmm::{
    AcousticCandidateFeatures, PitchAlternative, SegmentCandidate, decode_candidate_graph,
};
pub use review::{SingingReviewReason, SingingReviewRegion, build_review_regions};
pub use scalar::{FusedEstimate, WeightedEstimate, correlation_aware_score, fuse_scalar};
pub use transcript_fusion::{
    CanonicalLyrics, LyricsAuthority, TranscriptHypothesis, TranscriptTokenEvidence,
    fuse_transcripts,
};
pub use types::{
    CANONICAL_TIMELINE_STEP, CANONICAL_TIMELINE_STEP_MS, EvidenceProvenance, ExpertTask,
    TechniqueScores, TimeRange,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: f64, end: f64) -> TimeRange {
        TimeRange::from_seconds(start, end).unwrap()
    }

    fn token(text: &str, confidence: Option<f32>) -> TranscriptTokenEvidence {
        TranscriptTokenEvidence {
            id: None,
            text: text.to_string(),
            range: None,
            confidence,
        }
    }

    fn candidate(id: &str, start: f64, end: f64, midi: u8) -> SegmentCandidate {
        SegmentCandidate {
            id: id.to_string(),
            range: range(start, end),
            target_midi: midi,
            game_midi: f32::from(midi),
            game_boundary_decision_threshold: 0.2,
            game_presence_decision_threshold: 0.2,
            center_pitch_hz: 440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0),
            rmvpe_center_hz: None,
            rmvpe_confidence: None,
            rmvpe_cents_difference: None,
            rmvpe_voiced_ratio: None,
            rmvpe_pitch_mad_cents: None,
            fcpe_center_hz: None,
            fcpe_observed_ratio: None,
            fcpe_pitch_mad_cents: None,
            fcpe_cents_from_rmvpe: None,
            fcpe_supports_rmvpe: None,
            acoustic: None,
            techniques: TechniqueScores::default(),
            word_id: Some(id.to_string()),
            alternatives: Vec::new(),
        }
    }

    #[test]
    fn unavailable_evidence_is_not_serialized_as_a_measured_zero() {
        let frame = EvidenceFrame {
            frame_index: 4,
            rmvpe_f0_hz: None,
            fcpe_f0_hz: Some(ScalarEvidence {
                value: 220.0,
                confidence: Some(0.7),
            }),
            game_pitch_hz: None,
            game_boundary: None,
            stars_pitch_hz: None,
            stars_boundary: None,
            basic_pitch_onset: None,
            lyric_boundary: None,
            symbolic_note_prior: None,
            symbolic_boundary_prior: None,
            rms: None,
            spectral_flux: None,
            periodicity: None,
            snr_db: None,
            word_id: None,
            techniques: TechniqueScores::default(),
        };
        let json = serde_json::to_value(frame).unwrap();
        assert!(json.get("rmvpe_f0_hz").is_none());
        assert_eq!(json["fcpe_f0_hz"]["value"], 220.0);
        assert_eq!(json["techniques"], serde_json::json!({}));
    }

    #[test]
    fn correlated_evidence_does_not_count_as_two_independent_votes() {
        let estimate = |id: &str| WeightedEstimate {
            expert_id: id.to_string(),
            value: 440.0,
            calibrated_confidence: 0.8,
            base_weight: 1.0,
            correlation_group: Some("rmvpe-family".to_string()),
            dependencies: Vec::new(),
        };
        let one = correlation_aware_score(&[estimate("rmvpe")]).unwrap();
        let duplicated =
            correlation_aware_score(&[estimate("rmvpe"), estimate("stars-pitch")]).unwrap();
        assert!((one - duplicated).abs() < 1.0e-6);
    }

    #[test]
    fn unknown_transcript_confidence_remains_unknown_with_deterministic_conflict_policy() {
        let hypothesis = |expert: &str, text: &str| TranscriptHypothesis {
            expert_id: expert.to_string(),
            language: Some("en".to_string()),
            text: text.to_string(),
            tokens: vec![token(text, None)],
            confidence: None,
            correlation_group: None,
            dependencies: Vec::new(),
        };
        let fused = fuse_transcripts(&[
            hypothesis("qwen", "sing now"),
            hypothesis("challenger", "sing loud"),
        ])
        .unwrap();
        assert_eq!(fused.text, "sing loud");
        assert_eq!(fused.confidence, None);
        assert_eq!(fused.alternatives, ["sing now"]);
    }

    #[test]
    fn single_unknown_alignment_is_passed_through_without_confidence() {
        let words = fuse_word_boundaries(&[WordBoundaryEvidence {
            word_id: "word-1".to_string(),
            text: "sing".to_string(),
            range: range(1.0, 1.5),
            confidence: None,
            expert_id: "qwen-align".to_string(),
            correlation_group: None,
            dependencies: Vec::new(),
        }])
        .unwrap();
        assert_eq!(words[0].range, range(1.0, 1.5));
        assert_eq!(words[0].confidence, None);
        assert_eq!(words[0].disagreement, None);
    }

    #[test]
    fn conflicting_unknown_alignment_fails_closed() {
        let item = |expert: &str, start: f64| WordBoundaryEvidence {
            word_id: "word-1".to_string(),
            text: "sing".to_string(),
            range: range(start, start + 0.5),
            confidence: None,
            expert_id: expert.to_string(),
            correlation_group: None,
            dependencies: Vec::new(),
        };
        assert!(fuse_word_boundaries(&[item("a", 1.0), item("b", 1.1)]).is_err());
    }

    #[test]
    fn duration_decoder_prefers_a_coherent_non_overlapping_path() {
        let first = candidate("a", 0.0, 0.5, 69);
        let second = candidate("b", 0.5, 1.0, 71);
        let competing = candidate("wide", 0.0, 1.0, 81);
        let decoded = decode_candidate_graph(&[competing, second, first]).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "b"]
        );
    }

    #[test]
    fn non_overlapping_large_leap_is_never_deleted_by_transition_prior() {
        let first = candidate("a", 0.0, 0.5, 48);
        let jump = candidate("jump", 0.5, 1.0, 72);
        let decoded = decode_candidate_graph(&[first, jump]).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "jump"]
        );
    }

    #[test]
    fn calibration_is_versioned_and_rejects_invalid_temperature() {
        let calibrator = ScoreCalibrator {
            version: "rmvpe-intel-v1".to_string(),
            method: CalibrationMethod::Temperature { temperature: 2.0 },
        };
        assert!(calibrator.calibrate(0.9).unwrap() < 0.9);
        let invalid = ScoreCalibrator {
            version: "bad".to_string(),
            method: CalibrationMethod::Temperature { temperature: 0.0 },
        };
        assert!(invalid.calibrate(0.5).is_err());
    }
}
