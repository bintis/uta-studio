mod alignment_fusion;
mod baseline;
mod calibration;
mod candidate_states;
mod canonical;
mod evidence;
mod hsmm;
mod review;
mod scalar;
mod transcript_fusion;
mod types;

pub use alignment_fusion::{CanonicalWordBoundary, WordBoundaryEvidence, fuse_word_boundaries};
pub(crate) use baseline::fuse_singing_evidence_with_challengers;
pub use baseline::{
    BoundaryEvidenceSet, BoundarySegmentEvidence, PitchGrid, SingingFusionEvidence,
    fuse_singing_evidence,
};
pub use calibration::{CalibrationMethod, ScoreCalibrator};
pub(crate) use candidate_states::{persistent_f0_shifts, trustworthy_f0_point};
pub use canonical::{
    CanonicalNote, CanonicalNoteEvidence, CanonicalSingingTrack, F0Point, FusionContextSignalV1,
    FusionDecisionTraceV1, HarmonyMetadata, PitchBendPoint, PitchSelectionReasonV1,
    build_canonical_singing_track, validate_canonical_singing_track,
};
pub use evidence::{EvidenceFrame, EvidenceSeries, ScalarEvidence};
pub use hsmm::{
    AcousticCandidateFeatures, BasicPitchCandidateFeatures, BoundaryAlternative,
    BoundaryCandidateRole, BoundaryConstraintEvidenceV1, BoundaryConstraintKindV1,
    BoundaryEvidenceKind, PitchAlternative, SegmentCandidate, TechniqueCandidateFeatures,
    attach_boundary_constraints, decode_candidate_graph, decode_candidate_graph_with_boundaries,
    validate_candidate_path, validate_candidate_path_with_boundaries, validate_candidate_pool,
};
pub(crate) use review::merge_regions;
pub use review::{SingingReviewReason, SingingReviewRegion, build_review_regions};
pub use scalar::{FusedEstimate, WeightedEstimate, correlation_aware_score, fuse_scalar};
pub use transcript_fusion::{
    CanonicalLyrics, LyricsAuthority, TranscriptHypothesis, TranscriptTokenEvidence,
    fuse_transcripts,
};
pub use types::{
    CANONICAL_TIMELINE_STEP, CANONICAL_TIMELINE_STEP_MS, EvidenceProvenance, ExpertTask,
    HardBoundarySetV1, HardBoundaryV1, TechniqueScores, TimeRange,
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
            boundary_source: "game".to_string(),
            boundary_kind: BoundaryEvidenceKind::Game,
            boundary_role: BoundaryCandidateRole::Primary,
            boundary_fractional_midi: Some(f32::from(midi)),
            boundary_decision_parameter: Some(0.2),
            presence_decision_parameter: Some(0.2),
            boundary_hard: false,
            boundary_support: None,
            target_pitch_source: "game".to_string(),
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
            basic_pitch: None,
            boundary_calibrated_confidence: None,
            boundary_alternatives: Vec::new(),
            boundary_constraints: Vec::new(),
            technique_evidence: Vec::new(),
            techniques: TechniqueScores::default(),
            word_id: Some(id.to_string()),
            alternatives: Vec::new(),
        }
    }

    fn dense_candidates(widths: &[usize]) -> Vec<SegmentCandidate> {
        let layer_duration = 1.0 / widths.len() as f64;
        widths
            .iter()
            .enumerate()
            .flat_map(|(layer, width)| {
                let start = layer as f64 * layer_duration;
                let end = (layer + 1) as f64 * layer_duration;
                (0..*width).map(move |index| {
                    candidate(&format!("dense-{layer}-{index:04}"), start, end, 69)
                })
            })
            .collect()
    }

    fn shifted_dense_component(
        widths: &[usize],
        prefix: &str,
        offset: u64,
    ) -> Vec<SegmentCandidate> {
        dense_candidates(widths)
            .into_iter()
            .map(|mut candidate| {
                candidate.id = format!("{prefix}-{}", candidate.id);
                candidate.range.start += offset;
                candidate.range.end += offset;
                candidate
            })
            .collect()
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
            preference_rank: 0,
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
    fn duration_decoder_charges_a_strict_fragmentation_cost() {
        let first = candidate("a", 0.0, 0.5, 69);
        let second = candidate("b", 0.5, 1.0, 71);
        let competing = candidate("wide", 0.0, 1.0, 69);
        let decoded = decode_candidate_graph(&[competing, second, first]).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["wide"]
        );
    }

    #[test]
    fn final_candidate_path_rejects_overlap_for_every_selector() {
        let first = candidate("a", 0.0, 0.75, 69);
        let overlapping = candidate("b", 0.5, 1.0, 71);
        let error =
            validate_candidate_path(&[first.clone(), overlapping.clone()], &[first, overlapping])
                .expect_err("an overlapping selected path must fail closed");
        assert!(error.contains("ordered and non-overlapping"));
    }

    #[test]
    fn final_candidate_path_rejects_crossing_a_hard_boundary() {
        let selected = candidate("selected", 0.0, 1.0, 69);
        let error = validate_candidate_path_with_boundaries(
            std::slice::from_ref(&selected),
            std::slice::from_ref(&selected),
            &hard_boundary_set(),
        )
        .expect_err("a path that crosses a hard boundary must fail closed");
        assert!(error.contains("crosses a hard boundary"));

        let near_edge = HardBoundarySetV1 {
            boundaries: vec![HardBoundaryV1 {
                source: "caller.near-edge".to_string(),
                level: crate::BoundaryLevel::Word,
                range: range(0.01, 0.02),
            }],
        };
        let error = validate_candidate_path_with_boundaries(
            std::slice::from_ref(&selected),
            std::slice::from_ref(&selected),
            &near_edge,
        )
        .expect_err("a near-edge caller-hard boundary must remain exact");
        assert!(error.contains("crosses a hard boundary"));
    }

    #[test]
    fn final_candidate_path_allows_a_real_pool_level_silence_gap() {
        let before_gap = candidate("before-gap", 0.0, 0.5, 69);
        let after_gap = candidate("after-gap", 0.75, 1.0, 71);
        let pool = [before_gap.clone(), after_gap.clone()];
        validate_candidate_path_with_boundaries(
            &pool,
            &[before_gap, after_gap],
            &hard_boundary_set(),
        )
        .expect("a gap with no candidate is a represented silence component");
    }

    #[test]
    fn final_candidate_path_rejects_an_empty_selection() {
        let source = candidate("source", 0.0, 0.5, 69);
        let error = validate_candidate_path(&[source], &[])
            .expect_err("every selector must return at least one real candidate");
        assert!(error.contains("empty"));
    }

    #[test]
    fn final_candidate_path_accepts_the_algorithmic_decoder_output() {
        let candidates = vec![
            candidate("wide", 0.0, 1.0, 81),
            candidate("a", 0.0, 0.5, 69),
            candidate("b", 0.5, 1.0, 71),
        ];
        let decoded = decode_candidate_graph(&candidates).unwrap();
        validate_candidate_path(&candidates, &decoded)
            .expect("the deterministic decoder must pass the shared final-path gate");
    }

    #[test]
    fn candidate_utility_is_not_biased_against_fcpe_primary_evidence() {
        let mut rmvpe = candidate("rmvpe", 0.0, 0.5, 69);
        rmvpe.rmvpe_center_hz = Some(rmvpe.center_pitch_hz);
        rmvpe.rmvpe_cents_difference = Some(0.0);
        let mut fcpe = candidate("fcpe", 0.0, 0.5, 69);
        fcpe.fcpe_center_hz = Some(fcpe.center_pitch_hz);
        assert_eq!(
            rmvpe.emission_utility().unwrap(),
            fcpe.emission_utility().unwrap()
        );
    }

    #[test]
    fn acoustic_fundamental_support_is_relative_to_the_selected_target() {
        let acoustic = AcousticCandidateFeatures {
            frame_count: 20,
            mean_periodicity: 0.8,
            fundamental_center_hz: Some(440.0),
            ..AcousticCandidateFeatures::default()
        };
        let mut matching = candidate("acoustic-a4", 0.0, 0.5, 69);
        matching.acoustic = Some(acoustic.clone());
        let mut wrong_octave = candidate("acoustic-a5", 0.0, 0.5, 81);
        wrong_octave.acoustic = Some(acoustic);

        assert!(super::hsmm::acoustic_fundamental_support(&matching) > 0.79);
        assert_eq!(
            super::hsmm::acoustic_fundamental_support(&wrong_octave),
            0.0
        );
    }

    #[test]
    fn stable_peer_f0_support_is_relative_to_the_selected_target() {
        let mut wrong_octave = candidate("game-a5", 0.0, 0.5, 81);
        wrong_octave.rmvpe_center_hz = Some(440.0);
        wrong_octave.rmvpe_voiced_ratio = Some(1.0);
        wrong_octave.rmvpe_pitch_mad_cents = Some(0.0);
        wrong_octave.fcpe_center_hz = Some(441.0);
        wrong_octave.fcpe_observed_ratio = Some(1.0);
        wrong_octave.fcpe_pitch_mad_cents = Some(0.0);
        wrong_octave.fcpe_cents_from_rmvpe = Some(3.93);
        wrong_octave.fcpe_supports_rmvpe = Some(true);
        assert_eq!(super::hsmm::sustained_pitch_support(&wrong_octave), 0.0);

        let mut matching = wrong_octave.clone();
        matching.id = "rmvpe-a4".to_string();
        matching.target_midi = 69;
        matching.center_pitch_hz = 440.0;
        matching.target_pitch_source = "rmvpe".to_string();
        assert!(super::hsmm::sustained_pitch_support(&matching) > 0.9);

        wrong_octave.range = range(0.5, 0.6);
        matching.range = range(0.5, 0.6);
        let before = candidate("before-a4", 0.0, 0.5, 69);
        let after = candidate("after-a4", 0.6, 1.0, 69);
        let decoded = decode_candidate_graph(&[before, wrong_octave, matching, after]).unwrap();
        assert_eq!(
            decoded
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["before-a4", "rmvpe-a4", "after-a4"]
        );
    }

    fn hard_boundary_set() -> HardBoundarySetV1 {
        HardBoundarySetV1 {
            boundaries: vec![HardBoundaryV1 {
                source: "caller.hard".to_string(),
                level: crate::BoundaryLevel::Word,
                range: range(0.5, 1.0),
            }],
        }
    }

    #[test]
    fn pool_level_hard_boundaries_reset_every_aligned_selected_state() {
        let previous = candidate("previous", 0.0, 0.5, 69);
        let next = candidate("next", 0.5, 1.0, 81);
        let hard_boundaries = hard_boundary_set();
        assert_eq!(
            super::hsmm::transition_utility(&previous, &next, &hard_boundaries, &[]),
            0.0
        );

        let mut carrier = next.clone();
        carrier.boundary_hard = true;
        carrier.boundary_alternatives.push(BoundaryAlternative {
            source_expert: "candidate-local-noise".to_string(),
            range: range(0.0, 0.5),
            kind: BoundaryEvidenceKind::Constraint,
            fractional_midi: None,
            source_local_score: None,
            hard: true,
        });
        let without_carrier = decode_candidate_graph_with_boundaries(
            &[previous.clone(), next.clone()],
            &hard_boundaries,
        )
        .unwrap();
        let with_carrier =
            decode_candidate_graph_with_boundaries(&[previous, carrier], &hard_boundaries).unwrap();
        assert_eq!(
            without_carrier
                .iter()
                .map(|candidate| &candidate.id)
                .collect::<Vec<_>>(),
            with_carrier
                .iter()
                .map(|candidate| &candidate.id)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn typed_phrase_start_confidence_relaxes_only_melodic_priors() {
        let previous = candidate("phrase-before", 0.0, 0.5, 69);
        let next = candidate("phrase-after", 0.5, 1.0, 81);
        let hard_boundaries = HardBoundarySetV1::default();
        let baseline = super::hsmm::transition_utility(&previous, &next, &hard_boundaries, &[]);
        assert!(baseline < 0.0);

        let with_confidence = |confidence| {
            let mut candidate = next.clone();
            candidate
                .boundary_constraints
                .push(BoundaryConstraintEvidenceV1 {
                    source_expert: "constraint.user.phrase.p1".to_string(),
                    kind: BoundaryConstraintKindV1::PhraseStart,
                    time: candidate.range.start,
                    source_local_strength: Some(confidence),
                    calibrated_confidence: None,
                    calibration_version: None,
                    correlation_group: Some("constraint.user".to_string()),
                    depends_on: Vec::new(),
                });
            candidate
        };
        let zero = with_confidence(0.0);
        let half = with_confidence(0.5);
        let full = with_confidence(1.0);
        let zero_utility = super::hsmm::transition_utility(&previous, &zero, &hard_boundaries, &[]);
        let half_utility = super::hsmm::transition_utility(&previous, &half, &hard_boundaries, &[]);
        let full_utility = super::hsmm::transition_utility(&previous, &full, &hard_boundaries, &[]);
        assert_eq!(zero_utility, baseline);
        assert!(half_utility > zero_utility);
        assert!(full_utility > half_utility);
        assert!(full_utility < 0.0, "event cost must remain active");
        assert!(
            validate_candidate_path_with_boundaries(
                &[previous.clone(), full.clone()],
                &[previous, full],
                &hard_boundaries,
            )
            .is_ok()
        );
    }

    #[test]
    fn typed_phrase_start_confidence_attenuates_short_octave_return_prior() {
        let before = candidate("octave-before", 0.0, 0.5, 69);
        let middle = candidate("octave-middle", 0.5, 0.6, 81);
        let after = candidate("octave-after", 0.6, 1.1, 69);
        let hard_boundaries = HardBoundarySetV1::default();
        let baseline = super::hsmm::short_octave_return_penalty_for_test(
            &before,
            &middle,
            &after,
            &hard_boundaries,
            &[],
        );
        assert!(baseline < 0.0);

        let with_confidence = |confidence| {
            let mut candidate = middle.clone();
            candidate
                .boundary_constraints
                .push(BoundaryConstraintEvidenceV1 {
                    source_expert: "constraint.user.phrase.p1".to_string(),
                    kind: BoundaryConstraintKindV1::PhraseStart,
                    time: candidate.range.start,
                    source_local_strength: Some(confidence),
                    calibrated_confidence: None,
                    calibration_version: None,
                    correlation_group: Some("constraint.user".to_string()),
                    depends_on: Vec::new(),
                });
            candidate
        };
        let zero = super::hsmm::short_octave_return_penalty_for_test(
            &before,
            &with_confidence(0.0),
            &after,
            &hard_boundaries,
            &[],
        );
        let half = super::hsmm::short_octave_return_penalty_for_test(
            &before,
            &with_confidence(0.5),
            &after,
            &hard_boundaries,
            &[],
        );
        let full = super::hsmm::short_octave_return_penalty_for_test(
            &before,
            &with_confidence(1.0),
            &after,
            &hard_boundaries,
            &[],
        );
        assert_eq!(zero, baseline);
        assert!(half > zero && half < 0.0);
        assert_eq!(full, 0.0);
    }

    #[test]
    fn pool_level_hard_boundary_blocks_crossing_but_voicing_reset_does_not() {
        let crossing = candidate("crossing", 0.0, 1.0, 69);
        let hard_boundaries = hard_boundary_set();
        assert!(
            validate_candidate_path_with_boundaries(
                std::slice::from_ref(&crossing),
                std::slice::from_ref(&crossing),
                &hard_boundaries,
            )
            .is_err()
        );

        let mut voicing_only = crossing.clone();
        voicing_only
            .boundary_constraints
            .push(BoundaryConstraintEvidenceV1 {
                source_expert: "voicing".to_string(),
                kind: BoundaryConstraintKindV1::VoicingTransition,
                time: 500_000,
                source_local_strength: Some(1.0),
                calibrated_confidence: None,
                calibration_version: None,
                correlation_group: None,
                depends_on: Vec::new(),
            });
        validate_candidate_path_with_boundaries(
            std::slice::from_ref(&voicing_only),
            std::slice::from_ref(&voicing_only),
            &HardBoundarySetV1::default(),
        )
        .expect("voicing evidence resets melody scoring but is not a structural barrier");
    }

    #[test]
    fn dense_candidate_graph_has_exact_pair_state_limit_semantics() {
        let at_limit = dense_candidates(&[256, 256]);
        let selected = decode_candidate_graph(&at_limit).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["dense-0-0000", "dense-1-0000"]
        );

        let above_limit = dense_candidates(&[256, 257]);
        let error = decode_candidate_graph(&above_limit)
            .expect_err("one pair state above the documented limit must fail closed");
        assert!(error.contains("bounded pair-state limit"), "{error}");
    }

    #[test]
    fn disconnected_components_share_the_pair_state_budget() {
        let mut at_limit = shifted_dense_component(&[256, 128], "first", 0);
        at_limit.extend(shifted_dense_component(&[256, 128], "second", 2_000_000));
        decode_candidate_graph(&at_limit).unwrap();
        at_limit.extend(shifted_dense_component(&[1, 1], "over", 4_000_000));
        assert!(
            decode_candidate_graph(&at_limit)
                .unwrap_err()
                .contains("bounded pair-state limit")
        );
    }

    #[test]
    fn dense_candidate_graph_has_exact_transition_limit_semantics() {
        let at_limit = dense_candidates(&[100, 100, 200]);
        let selected = decode_candidate_graph(&at_limit).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["dense-0-0000", "dense-1-0000", "dense-2-0000"]
        );

        let above_limit = dense_candidates(&[100, 100, 201]);
        let error = decode_candidate_graph(&above_limit)
            .expect_err("one transition batch above the documented limit must fail closed");
        assert!(error.contains("bounded transition limit"), "{error}");
    }

    #[test]
    fn disconnected_components_share_the_transition_budget() {
        let mut at_limit = shifted_dense_component(&[100, 100, 100], "first", 0);
        at_limit.extend(shifted_dense_component(
            &[100, 100, 100],
            "second",
            2_000_000,
        ));
        decode_candidate_graph(&at_limit).unwrap();
        at_limit.extend(shifted_dense_component(&[1, 1, 1], "over", 4_000_000));
        assert!(
            decode_candidate_graph(&at_limit)
                .unwrap_err()
                .contains("bounded transition limit")
        );
    }

    #[test]
    fn contextual_onset_evidence_can_promote_a_challenger_segmentation_path() {
        let wide = candidate("wide", 0.0, 1.0, 69);
        let mut first = candidate("rosvot-a", 0.0, 0.5, 69);
        first.boundary_source = "rosvot".to_string();
        first.boundary_kind = BoundaryEvidenceKind::AdvancedNote;
        first.boundary_role = BoundaryCandidateRole::Challenger;
        first.target_pitch_source = "rosvot".to_string();
        let mut second = candidate("rosvot-b", 0.5, 1.0, 71);
        second.boundary_source = "rosvot".to_string();
        second.boundary_kind = BoundaryEvidenceKind::AdvancedNote;
        second.boundary_role = BoundaryCandidateRole::Challenger;
        second.target_pitch_source = "rosvot".to_string();

        let baseline =
            decode_candidate_graph(&[wide.clone(), first.clone(), second.clone()]).unwrap();
        assert_eq!(
            baseline
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["wide"]
        );

        second.basic_pitch = Some(BasicPitchCandidateFeatures {
            onset_activation: 0.9,
            note_activation: 0.9,
            contour_activation: 0.8,
            contour_class: 42,
            onset_supported: true,
        });
        let contextual = decode_candidate_graph(&[wide, first, second]).unwrap();
        assert_eq!(
            contextual
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            ["rosvot-a", "rosvot-b"]
        );
    }

    #[test]
    fn acoustic_expressive_continuity_rejects_a_marginal_false_split() {
        let wide = candidate("wide", 0.0, 1.0, 69);
        let mut first = candidate("split-a", 0.0, 0.5, 69);
        let mut second = candidate("split-b", 0.5, 1.0, 70);
        for split in [&mut first, &mut second] {
            split.boundary_source = "rosvot".to_string();
            split.boundary_kind = BoundaryEvidenceKind::AdvancedNote;
            split.boundary_role = BoundaryCandidateRole::Challenger;
            split.target_pitch_source = "rosvot".to_string();
            split.boundary_calibrated_confidence = Some(1.0);
        }
        let without_expression =
            decode_candidate_graph(&[wide.clone(), first.clone(), second.clone()]).unwrap();
        assert_eq!(
            without_expression
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["split-a", "split-b"]
        );
        let expressive = AcousticCandidateFeatures {
            frame_count: 20,
            mean_rms: 0.2,
            mean_periodicity: 0.9,
            fundamental_center_hz: None,
            mean_snr_db: 20.0,
            mean_vibrato_activation: 0.8,
            mean_glide_activation: 0.0,
            mean_ornament_activation: 0.0,
            mean_breath_activation: 0.0,
            max_voicing_transition_activation: 0.0,
            onset_flux: Some(0.01),
            preceding_flux: Some(0.01),
            onset_supported: Some(false),
        };
        first.acoustic = Some(expressive.clone());
        second.acoustic = Some(expressive);
        let with_expression = decode_candidate_graph(&[wide, first, second]).unwrap();
        assert_eq!(
            with_expression
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["wide"]
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
    fn weak_micro_split_loses_but_strong_short_onset_survives() {
        let wide = candidate("wide", 0.0, 1.0, 69);
        let mut first = candidate("split-a", 0.0, 0.475, 69);
        let mut short = candidate("split-short", 0.475, 0.525, 71);
        let mut last = candidate("split-b", 0.525, 1.0, 69);
        for split in [&mut first, &mut short, &mut last] {
            split.boundary_source = "f0.transition".to_string();
            split.boundary_kind = BoundaryEvidenceKind::F0Transition;
            split.boundary_role = BoundaryCandidateRole::Challenger;
            split.target_pitch_source = "rmvpe".to_string();
            split.word_id = Some("same-word".to_string());
        }
        let weak =
            decode_candidate_graph(&[wide.clone(), first.clone(), short.clone(), last.clone()])
                .unwrap();
        assert_eq!(
            weak.iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["wide"]
        );

        short.basic_pitch = Some(BasicPitchCandidateFeatures {
            onset_activation: 0.98,
            note_activation: 0.95,
            contour_activation: 0.9,
            contour_class: 42,
            onset_supported: true,
        });
        first.boundary_calibrated_confidence = Some(0.9);
        short.boundary_calibrated_confidence = Some(1.0);
        last.boundary_calibrated_confidence = Some(0.9);
        short.rmvpe_voiced_ratio = Some(0.95);
        short.rmvpe_pitch_mad_cents = Some(20.0);
        let strong = decode_candidate_graph(&[wide, first, short, last]).unwrap();
        assert_eq!(
            strong
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["split-a", "split-short", "split-b"]
        );
    }

    #[test]
    fn short_octave_return_prefers_an_evidence_backed_covering_state() {
        let stable = candidate("stable-cover", 0.0, 1.0, 69);
        let mut before = candidate("before", 0.0, 0.475, 69);
        let mut outlier = candidate("octave-outlier", 0.475, 0.525, 81);
        let mut after = candidate("after", 0.525, 1.0, 69);
        for candidate in [&mut before, &mut outlier, &mut after] {
            candidate.word_id = Some("same-word".to_string());
        }
        outlier.boundary_role = BoundaryCandidateRole::Challenger;
        outlier.boundary_kind = BoundaryEvidenceKind::F0Transition;
        let selected = decode_candidate_graph(&[stable, before, outlier, after]).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["stable-cover"]
        );
    }

    #[test]
    fn long_rest_resets_melody_prior_and_same_pitch_attack_survives() {
        let before_rest = candidate("before-rest", 0.0, 0.5, 48);
        let after_rest = candidate("after-rest", 1.0, 1.5, 84);
        let selected = decode_candidate_graph(&[before_rest, after_rest]).unwrap();
        assert_eq!(selected.len(), 2);

        let first = candidate("first", 0.0, 0.5, 69);
        let mut repeated = candidate("repeated", 0.5, 0.58, 69);
        repeated.basic_pitch = Some(BasicPitchCandidateFeatures {
            onset_activation: 0.99,
            note_activation: 0.9,
            contour_activation: 0.8,
            contour_class: 42,
            onset_supported: true,
        });
        let selected = decode_candidate_graph(&[first, repeated]).unwrap();
        assert_eq!(
            selected
                .iter()
                .map(|candidate| candidate.id.as_str())
                .collect::<Vec<_>>(),
            ["first", "repeated"]
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

    #[test]
    fn contextual_constraints_are_boundary_local_and_correlated_votes_are_discounted() {
        let mut one = candidate("one", 1.0, 2.0, 69);
        let mut duplicated = one.clone();
        let primary = BoundaryConstraintEvidenceV1 {
            source_expert: "rmvpe.transition".to_string(),
            kind: BoundaryConstraintKindV1::PitchDiscontinuity,
            time: one.range.start,
            source_local_strength: Some(0.8),
            calibrated_confidence: None,
            calibration_version: Some("source-local-v1".to_string()),
            correlation_group: Some("continuous-pitch-neural".to_string()),
            depends_on: vec!["rmvpe".to_string()],
        };
        let mut duplicate = primary.clone();
        duplicate.source_expert = "fcpe.transition".to_string();
        duplicate.source_local_strength = Some(0.7);
        duplicate.depends_on = vec!["fcpe".to_string()];
        let mut distant = primary.clone();
        distant.time = one.range.end + 500_000;

        attach_boundary_constraints(std::slice::from_mut(&mut one), &[primary.clone(), distant])
            .unwrap();
        attach_boundary_constraints(std::slice::from_mut(&mut duplicated), &[primary, duplicate])
            .unwrap();
        assert_eq!(one.boundary_constraints.len(), 1);
        assert_eq!(duplicated.boundary_constraints.len(), 2);
        assert_eq!(one.emission_utility(), duplicated.emission_utility());
    }

    #[test]
    fn isolated_peer_pitch_is_not_privileged_by_boundary_carrier_identity() {
        let mut wrong_boundary_pitch = candidate("wrong-game-octave", 0.0, 1.0, 81);
        wrong_boundary_pitch.rmvpe_center_hz = Some(440.0);
        wrong_boundary_pitch.rmvpe_cents_difference = Some(-1_200.0);

        let mut matching_peer = wrong_boundary_pitch.clone();
        matching_peer.id = "matching-rmvpe".to_string();
        matching_peer.target_midi = 69;
        matching_peer.target_pitch_source = "rmvpe".to_string();
        matching_peer.center_pitch_hz = 440.0;
        matching_peer.rmvpe_cents_difference = Some(0.0);

        assert!(
            matching_peer.emission_utility().unwrap()
                > wrong_boundary_pitch.emission_utility().unwrap()
        );
    }

    #[test]
    fn legacy_game_aliases_are_readable_but_not_fabricated_for_f0_regions() {
        let legacy: CanonicalNoteEvidence = serde_json::from_value(serde_json::json!({
            "source_experts": ["game"],
            "boundary_source": "game",
            "boundary_kind": "game",
            "game_fractional_midi": 69.25,
            "game_boundary_decision_threshold": 0.2,
            "game_presence_decision_threshold": 0.3,
            "target_pitch_source": "game"
        }))
        .unwrap();
        assert_eq!(legacy.boundary_fractional_midi, Some(69.25));
        assert_eq!(legacy.boundary_decision_parameter, Some(0.2));
        assert_eq!(legacy.presence_decision_parameter, Some(0.3));
        assert_eq!(legacy.boundary_role, BoundaryCandidateRole::Primary);

        let mut evidence = legacy;
        evidence.boundary_source = "rmvpe.f0_segmentation".to_string();
        evidence.boundary_kind = BoundaryEvidenceKind::F0Derived;
        evidence.boundary_fractional_midi = None;
        evidence.boundary_decision_parameter = None;
        evidence.presence_decision_parameter = None;
        evidence.target_pitch_source = "rmvpe".to_string();
        let f0_json = serde_json::to_value(&evidence).unwrap();
        assert!(f0_json.get("game_fractional_midi").is_none());
        assert!(f0_json.get("game_boundary_decision_threshold").is_none());
        assert!(f0_json.get("game_presence_decision_threshold").is_none());
        assert!(f0_json.get("boundary_fractional_midi").is_none());
        assert_eq!(f0_json["boundary_kind"], "f0_derived");
    }
}
