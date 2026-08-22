mod alignment_fusion;
mod calibration;
mod canonical;
mod evidence;
mod fusion;
mod hsmm;
mod review;
mod transcript_fusion;
mod types;

pub use alignment_fusion::{CanonicalWordBoundary, WordBoundaryEvidence, fuse_word_boundaries};
pub use calibration::{CalibrationMethod, ScoreCalibrator};
pub use canonical::{
    CanonicalNote, CanonicalNoteEvidence, CanonicalSingingTrack, F0Point, HarmonyMetadata,
    PitchBendPoint, build_canonical_singing_track,
};
pub use evidence::{EvidenceFrame, EvidenceSeries, ScalarEvidence};
pub use fusion::{FusedEstimate, WeightedEstimate, correlation_aware_score, fuse_scalar};
pub use hsmm::{PitchAlternative, SegmentCandidate, decode_candidate_graph};
pub use review::{SingingReviewReason, SingingReviewRegion, build_review_regions};
pub use transcript_fusion::{
    CanonicalLyrics, TranscriptHypothesis, TranscriptTokenEvidence, fuse_transcripts,
};
pub use types::{
    CANONICAL_TIMELINE_STEP_MS, EvidenceProvenance, ExpertTask, TechniqueScores, TimeRange,
};

#[cfg(test)]
mod tests {
    use super::*;

    fn range(start: f64, end: f64) -> TimeRange {
        TimeRange::new(start, end).unwrap()
    }

    fn token(text: &str) -> TranscriptTokenEvidence {
        TranscriptTokenEvidence {
            text: text.to_string(),
            range: None,
            confidence: 0.8,
        }
    }

    fn candidate(id: &str, start: f64, end: f64, midi: u8, score: f32) -> SegmentCandidate {
        SegmentCandidate {
            id: id.to_string(),
            range: range(start, end),
            midi_note: Some(midi),
            center_pitch_hz: 440.0 * 2.0_f32.powf((midi as f32 - 69.0) / 12.0),
            pitch_score: score,
            boundary_score: score,
            duration_score: score,
            alignment_score: score,
            technique_score: 0.0,
            symbolic_prior_score: 0.0,
            onset_strength: 0.8,
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
                confidence: 0.7,
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
    fn transcript_fusion_preserves_independent_consensus_and_alternatives() {
        let hypothesis = |expert: &str, text: &str, confidence: f32| TranscriptHypothesis {
            expert_id: expert.to_string(),
            language: "zh".to_string(),
            tokens: vec![token(text)],
            confidence,
            correlation_group: None,
            dependencies: Vec::new(),
        };
        let fused = fuse_transcripts(&[
            hypothesis("firered", "你好", 0.62),
            hypothesis("qwen", "你好", 0.62),
            hypothesis("challenger", "你号", 0.9),
        ])
        .unwrap();
        assert_eq!(fused.text, "你好");
        assert_eq!(fused.source_experts, ["firered", "qwen"]);
        assert_eq!(fused.alternatives, ["你号"]);
    }

    #[test]
    fn alignment_fusion_retains_disagreement_instead_of_hiding_it() {
        let item = |expert: &str, start: f64, end: f64| WordBoundaryEvidence {
            word_id: "word-1".to_string(),
            text: "sing".to_string(),
            range: range(start, end),
            confidence: 0.8,
            expert_id: expert.to_string(),
            correlation_group: None,
            dependencies: Vec::new(),
        };
        let words =
            fuse_word_boundaries(&[item("qwen-align", 1.0, 1.5), item("firered-time", 1.2, 1.8)])
                .unwrap();
        assert_eq!(words.len(), 1);
        assert!(words[0].disagreement_seconds >= 0.1);
        assert_eq!(words[0].source_experts.len(), 2);
    }

    #[test]
    fn duration_decoder_prefers_a_coherent_non_overlapping_path() {
        let first = candidate("a", 0.0, 0.5, 69, 0.45);
        let second = candidate("b", 0.5, 1.0, 71, 0.45);
        let competing = candidate("wide", 0.0, 1.0, 81, 0.75);
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
    fn canonical_track_keeps_uncertainty_and_creates_review_work() {
        let transcript = CanonicalLyrics {
            text: "sing".to_string(),
            language: "en".to_string(),
            tokens: vec![token("sing")],
            confidence: 0.8,
            source_experts: vec!["qwen".to_string()],
            alternatives: Vec::new(),
        };
        let mut note = candidate("note-1", 0.0, 0.5, 69, 0.5);
        note.boundary_score = 0.3;
        note.alternatives.push(PitchAlternative {
            midi_note: 81,
            probability: 0.42,
        });
        let track = build_canonical_singing_track(
            transcript,
            Vec::new(),
            vec![note],
            vec![F0Point {
                time: 0.1,
                hz: 440.0,
                confidence: 0.8,
            }],
            HarmonyMetadata::default(),
            Vec::new(),
        )
        .unwrap();
        assert!(track.notes[0].uncertain);
        let review = build_review_regions(&track);
        assert!(review[0].reasons.contains(&SingingReviewReason::OctaveRisk));
        assert!(
            review[0]
                .reasons
                .contains(&SingingReviewReason::BoundaryDisagreement)
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
