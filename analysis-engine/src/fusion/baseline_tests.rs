use super::*;
use crate::artifact::{
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
    BasicPitchEvidenceV3, BasicPitchFrameV3, GameNoteEvidenceV1, TechniqueEvidenceV1,
    TechniqueIntervalV1,
};
use crate::fingerprint::{ACOUSTIC_DSP_VERSION, FUSION_VERSION};
use crate::fusion::{EvidenceProvenance, ExpertTask};

fn acoustic() -> AcousticEvidenceV1 {
    AcousticEvidenceV1 {
        contract: ACOUSTIC_EVIDENCE_CONTRACT.to_string(),
        version: ACOUSTIC_EVIDENCE_VERSION,
        algorithm: ACOUSTIC_DSP_VERSION.to_string(),
        timebase: 1_000_000,
        start: 0,
        hop: 10_000,
        sample_rate: 16_000,
        window_samples: 512,
        semantic_audio_role: "lead_vocal".to_string(),
        decoded_audio_sha256: "a".repeat(64),
        frames: (0..60)
            .map(|index| AcousticEvidenceFrameV1 {
                start: index * 10_000,
                rms: 0.2,
                spectral_flux: (index > 0).then_some(if index == 10 { 0.3 } else { 0.01 }),
                periodicity: 0.8,
                snr_db: 20.0,
                fundamental_hz: Some(440.0),
                vibrato_activation: 0.0,
                glide_activation: 0.0,
                ornament_activation: 0.0,
                breath_activation: 0.0,
                voicing_transition_activation: 0.0,
            })
            .collect(),
    }
}

fn game(midi: f32) -> GameEvidenceV1 {
    GameEvidenceV1 {
        schema_version: 1,
        model_id: "game".to_string(),
        variant: "fixture".to_string(),
        source_asset_sha256: "b".repeat(64),
        source_commit: "fixture".to_string(),
        model_manifest_sha256: "c".repeat(64),
        runtime_manifest_sha256: "d".repeat(64),
        backend: "openvino_gpu".to_string(),
        sample_rate: 44_100,
        timestep_ms: 10,
        d3pm_steps: 8,
        estimator_note_buckets: vec![32],
        notes: vec![GameNoteEvidenceV1 {
            range: TimeRange::new(100_000, 500_000).unwrap(),
            midi,
            boundary_decision_threshold: 0.2,
            presence_decision_threshold: 0.2,
        }],
    }
}

fn boundaries(midi: f32) -> BoundaryEvidenceSet {
    BoundaryEvidenceSet::from_game(&game(midi)).unwrap()
}

fn technique_evidence() -> TechniqueEvidenceV1 {
    TechniqueEvidenceV1 {
        contract: "uta.analysis-engine.technique-evidence".to_string(),
        version: 1,
        model_id: "stars".to_string(),
        taxonomy: vec![
            "vibrato".to_string(),
            "glissando".to_string(),
            "falsetto".to_string(),
        ],
        calibration: "source_local_sigmoid_uncalibrated".to_string(),
        intervals: vec![TechniqueIntervalV1 {
            range: TimeRange::new(100_000, 500_000).unwrap(),
            phoneme_id: 1,
            raw_logits: vec![1.0, -1.0, -2.0],
            source_local_scores: vec![0.8, 0.2, 0.1],
        }],
        style_scope: "segment_global".to_string(),
        styles: Vec::new(),
        provenance: EvidenceProvenance {
            expert_id: "stars".to_string(),
            task: ExpertTask::Technique,
            model_hash: Some("e".repeat(64)),
            runtime_identity: Some("f".repeat(64)),
            calibration_version: Some("source_local_sigmoid_uncalibrated".to_string()),
            correlation_group: Some("conditioned:fixture".to_string()),
            depends_on: vec!["alignment:fixture".to_string()],
        },
    }
}

#[test]
fn fractional_game_midi_is_retained_at_explicit_target_decision() {
    assert_eq!(FUSION_VERSION, "fusion-v16");
    let fused = fuse_singing_evidence(
        &[],
        &boundaries(69.25),
        "rmvpe",
        &[],
        None,
        &[],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap();
    assert_eq!(fused.candidates[0].target_midi, 69);
    assert_eq!(fused.candidates[0].boundary_fractional_midi, Some(69.25));
    assert!(fused.candidates[0].rmvpe_center_hz.is_none());
}

#[test]
fn octave_f0_disagreement_remains_continuous_review_evidence() {
    let fused = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[F0Point {
            time: 110_000,
            hz: 880.0,
            confidence: Some(0.9),
        }],
        None,
        &[],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap();
    assert_eq!(fused.candidates.len(), 2);
    let base = fused
        .candidates
        .iter()
        .find(|candidate| candidate.target_pitch_source == "game")
        .unwrap();
    let alternative = &base.alternatives[0];
    assert_eq!(alternative.center_hz, 880.0);
    assert!((alternative.cents_from_target - 1_200.0).abs() < 0.01);
    assert!(fused.candidates.iter().any(|candidate| {
        candidate.target_pitch_source == "rmvpe" && candidate.target_midi == 81
    }));
}

#[test]
fn globally_coherent_segment_pitch_alternative_can_win() {
    let mut candidates = fuse_singing_evidence(
        &[],
        &boundaries(81.0),
        "rmvpe",
        &[F0Point {
            time: 110_000,
            hz: 440.0,
            confidence: Some(0.9),
        }],
        None,
        &[],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap()
    .candidates;
    let smooth = candidates
        .iter()
        .find(|candidate| candidate.target_pitch_source == "rmvpe")
        .unwrap()
        .clone();
    for (id, range) in [
        ("context-before", TimeRange::new(0, 100_000).unwrap()),
        ("context-after", TimeRange::new(500_000, 600_000).unwrap()),
    ] {
        let mut context = smooth.clone();
        context.id = id.to_string();
        context.range = range;
        context.boundary_source = "game".to_string();
        context.target_pitch_source = "game".to_string();
        context.boundary_fractional_midi = Some(69.0);
        context.target_midi = 69;
        context.center_pitch_hz = 440.0;
        context.rmvpe_cents_difference = Some(0.0);
        context.alternatives.clear();
        candidates.push(context);
    }
    for candidate in &mut candidates {
        candidate.acoustic = None;
        candidate.basic_pitch = None;
    }
    let selected = crate::fusion::decode_candidate_graph(&candidates).unwrap();
    let middle = selected
        .iter()
        .find(|candidate| candidate.range.start == 100_000)
        .unwrap();
    assert_eq!(middle.target_midi, 69);
    assert_eq!(middle.target_pitch_source, "rmvpe");
}

#[test]
fn fcpe_records_support_and_disagreement_without_replacing_rmvpe() {
    let point = |hz| F0Point {
        time: 110_000,
        hz,
        confidence: None,
    };
    let agreed = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[point(440.0)],
        None,
        &[point(441.0)],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap();
    assert_eq!(agreed.candidates[0].fcpe_supports_rmvpe, Some(true));
    assert_eq!(agreed.candidates[0].center_pitch_hz, 440.0);
    let pitch_sources = agreed
        .candidates
        .iter()
        .map(|candidate| candidate.target_pitch_source.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        pitch_sources,
        ["fcpe", "game", "rmvpe"].into_iter().collect()
    );

    let disagreed = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[point(440.0)],
        None,
        &[point(880.0)],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap();
    assert_eq!(disagreed.candidates[0].fcpe_supports_rmvpe, Some(false));
    let fcpe_alternative = disagreed.candidates[0]
        .alternatives
        .iter()
        .find(|alternative| alternative.source_expert == "fcpe")
        .expect("FCPE disagreement remains an explicit pitch hypothesis");
    assert_eq!(fcpe_alternative.confidence, None);

    let secondary_only = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[],
        None,
        &[point(440.0)],
        None,
        Some(&acoustic()),
        None,
    )
    .unwrap();
    assert_eq!(secondary_only.candidates[0].rmvpe_center_hz, None);
    assert_eq!(secondary_only.candidates[0].fcpe_center_hz, Some(440.0));
    assert_eq!(secondary_only.candidates[0].fcpe_supports_rmvpe, None);
}

#[test]
fn basic_pitch_is_source_local_onset_support_not_note_authority() {
    let evidence = BasicPitchEvidenceV3 {
        frames: vec![
            BasicPitchFrameV3 {
                time: 110_000,
                note_activation: 0.9,
                onset_activation: 0.8,
                contour_class: 42,
                contour_activation: 0.7,
            },
            BasicPitchFrameV3 {
                time: 300_000,
                note_activation: 0.9,
                onset_activation: 0.99,
                contour_class: 42,
                contour_activation: 0.7,
            },
        ],
        model_manifest_sha256: "a".repeat(64),
        runtime_manifest_sha256: "b".repeat(64),
    };
    let fused = fuse_singing_evidence(
        &[],
        &boundaries(69.25),
        "rmvpe",
        &[],
        None,
        &[],
        None,
        Some(&acoustic()),
        Some(&evidence),
    )
    .unwrap();
    let candidate = &fused.candidates[0];
    assert_eq!(candidate.target_midi, 69);
    assert_eq!(
        candidate.basic_pitch.as_ref().unwrap().onset_activation,
        0.8
    );
    assert_eq!(candidate.basic_pitch.as_ref().unwrap().note_activation, 0.9);
    assert_eq!(
        candidate.basic_pitch.as_ref().unwrap().contour_activation,
        0.7
    );
    assert_eq!(candidate.basic_pitch.as_ref().unwrap().contour_class, 42);
    assert!(candidate.basic_pitch.as_ref().unwrap().onset_supported);
}

#[test]
fn basic_pitch_onset_creates_a_real_contextual_split_path() {
    let evidence = BasicPitchEvidenceV3 {
        frames: vec![
            BasicPitchFrameV3 {
                time: 100_000,
                note_activation: 0.8,
                onset_activation: 0.1,
                contour_class: 42,
                contour_activation: 0.7,
            },
            BasicPitchFrameV3 {
                time: 300_000,
                note_activation: 0.9,
                onset_activation: 0.95,
                contour_class: 42,
                contour_activation: 0.8,
            },
            BasicPitchFrameV3 {
                time: 400_000,
                note_activation: 0.8,
                onset_activation: 0.1,
                contour_class: 42,
                contour_activation: 0.7,
            },
        ],
        model_manifest_sha256: "a".repeat(64),
        runtime_manifest_sha256: "b".repeat(64),
    };
    let f0 = [110_000, 200_000, 310_000, 400_000]
        .into_iter()
        .map(|time| F0Point {
            time,
            hz: 440.0,
            confidence: Some(0.9),
        })
        .collect::<Vec<_>>();
    let fused = fuse_singing_evidence_with_challengers(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &f0,
        None,
        &[],
        None,
        None,
        true,
        Some(&evidence),
        &[],
        &[],
    )
    .unwrap();
    let onset_candidates = fused
        .candidates
        .iter()
        .filter(|candidate| candidate.boundary_kind == BoundaryEvidenceKind::BasicPitchOnset)
        .collect::<Vec<_>>();
    let onset_ranges = onset_candidates
        .iter()
        .map(|candidate| (candidate.range.start, candidate.range.end))
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(onset_ranges.len(), 2);
    assert!(
        onset_candidates
            .iter()
            .all(|candidate| candidate.boundary_role == BoundaryCandidateRole::Challenger)
    );
    let selected = crate::fusion::decode_candidate_graph(&fused.candidates).unwrap();
    assert_eq!(
        selected.len(),
        2,
        "selected candidates: {selected:#?}; onset candidates: {onset_candidates:#?}"
    );
    assert!(
        selected
            .iter()
            .all(|candidate| candidate.boundary_kind == BoundaryEvidenceKind::BasicPitchOnset)
    );
}

#[test]
fn enabled_advanced_note_expert_contributes_real_candidate_states() {
    let challenger = BoundaryAlternative {
        source_expert: "rosvot".to_string(),
        range: TimeRange::new(100_000, 300_000).unwrap(),
        kind: BoundaryEvidenceKind::AdvancedNote,
        fractional_midi: Some(69.0),
        source_local_score: None,
        hard: false,
    };
    let fused = fuse_singing_evidence_with_challengers(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[F0Point {
            time: 110_000,
            hz: 440.0,
            confidence: Some(0.9),
        }],
        None,
        &[],
        None,
        None,
        true,
        None,
        &[challenger],
        &[],
    )
    .unwrap();
    let boundary_states = fused
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.boundary_source.as_str(),
                candidate.range.start,
                candidate.range.end,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(boundary_states.len(), 2);
    let challengers = fused
        .candidates
        .iter()
        .filter(|candidate| candidate.boundary_source == "rosvot")
        .collect::<Vec<_>>();
    assert!(challengers.len() >= 2);
    assert!(challengers.iter().all(|candidate| {
        candidate.boundary_kind == BoundaryEvidenceKind::AdvancedNote
            && candidate.boundary_role == BoundaryCandidateRole::Challenger
            && candidate
                .boundary_alternatives
                .iter()
                .any(|alternative| alternative.source_expert == "game")
    }));
    let pitch_sources = challengers
        .iter()
        .map(|candidate| candidate.target_pitch_source.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert!(pitch_sources.contains("rosvot"));
    assert!(pitch_sources.contains("rmvpe"));
}

#[test]
fn source_local_technique_evidence_reaches_context_without_fake_confidence() {
    let technique = technique_evidence();
    let fused = fuse_singing_evidence_with_challengers(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[F0Point {
            time: 110_000,
            hz: 440.0,
            confidence: Some(0.9),
        }],
        None,
        &[],
        None,
        None,
        true,
        None,
        &[],
        &[technique],
    )
    .unwrap();
    let candidate = &fused.candidates[0];
    assert_eq!(candidate.technique_evidence.len(), 1);
    assert_eq!(
        candidate.technique_evidence[0].vibrato_activation,
        Some(0.8)
    );
    assert_eq!(candidate.techniques, TechniqueScores::default());
}

fn curve_from_cents(cents: impl IntoIterator<Item = f32>) -> Vec<F0Point> {
    cents
        .into_iter()
        .enumerate()
        .map(|(index, cents)| F0Point {
            time: 100_000 + index as u64 * 10_000,
            hz: 440.0 * 2.0_f32.powf(cents / 1_200.0),
            confidence: Some(0.9),
        })
        .collect()
}

#[test]
fn stable_f0_coarser_state_removes_unsupported_sequential_game_fragments() {
    let fragmented = BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: [(100_000, 200_000), (200_000, 300_000), (300_000, 400_000)]
            .into_iter()
            .map(|(start, end)| BoundarySegmentEvidence {
                range: TimeRange::new(start, end).unwrap(),
                fractional_midi: Some(69.0),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
            .collect(),
    };
    let curve = (0..30)
        .map(|index| F0Point {
            time: 100_000 + index * 10_000,
            hz: 440.0,
            confidence: Some(0.9),
        })
        .collect::<Vec<_>>();
    let fused = fuse_singing_evidence_with_challengers(
        &[],
        &fragmented,
        "rmvpe",
        &curve,
        None,
        &[],
        None,
        None,
        true,
        None,
        &[],
        &[],
    )
    .unwrap();
    let original_game_ranges = fused
        .candidates
        .iter()
        .filter(|candidate| {
            candidate.boundary_kind == BoundaryEvidenceKind::Game
                && candidate.target_pitch_source == "game"
        })
        .map(|candidate| (candidate.range, candidate.boundary_fractional_midi))
        .collect::<Vec<_>>();
    assert_eq!(
        original_game_ranges,
        vec![
            (TimeRange::new(100_000, 200_000).unwrap(), Some(69.0)),
            (TimeRange::new(200_000, 300_000).unwrap(), Some(69.0)),
            (TimeRange::new(300_000, 400_000).unwrap(), Some(69.0)),
        ],
        "the auditable pool must retain every original GAME range and pitch"
    );
    let selected = crate::fusion::decode_candidate_graph(&fused.candidates).unwrap();
    assert_eq!(selected.len(), 1, "selected candidates: {selected:#?}");
    assert_eq!(
        selected[0].boundary_kind,
        BoundaryEvidenceKind::F0Consolidation
    );
    assert_eq!(selected[0].range, TimeRange::new(100_000, 400_000).unwrap());
}

#[test]
fn persistent_f0_transition_rejects_vibrato_single_frame_octave_and_glide() {
    let vibrato = curve_from_cents((0..40).map(|index| match index % 4 {
        0 => -90.0,
        2 => 90.0,
        _ => 0.0,
    }));
    let octave_outlier =
        curve_from_cents((0..40).map(|index| if index == 20 { 1_200.0 } else { 0.0 }));
    let glide = curve_from_cents((0..40).map(|index| index as f32 * 18.0));
    for curve in [&vibrato, &octave_outlier, &glide] {
        assert!(
            f0_transition_challengers(&boundaries(69.0), "rmvpe", curve)
                .unwrap()
                .is_empty()
        );
    }
}

#[test]
fn persistent_f0_transition_retains_a_sustained_octave_leap() {
    let curve = curve_from_cents((0..40).map(|index| if index < 20 { 0.0 } else { 1_200.0 }));
    let challengers = f0_transition_challengers(&boundaries(69.0), "rmvpe", &curve).unwrap();
    assert_eq!(challengers.len(), 2);
    assert_eq!(
        challengers[0].range,
        TimeRange::new(100_000, 300_000).unwrap()
    );
    assert_eq!(
        challengers[1].range,
        TimeRange::new(300_000, 500_000).unwrap()
    );
}

#[test]
fn robust_cents_center_resists_a_short_octave_outlier_and_reports_coverage() {
    let range = TimeRange::new(100_000, 200_000).unwrap();
    let mut points = (0..9)
        .map(|index| F0Point {
            time: 100_000 + index * 10_000,
            hz: 440.0,
            confidence: Some(0.9),
        })
        .collect::<Vec<_>>();
    points.push(F0Point {
        time: 190_000,
        hz: 880.0,
        confidence: Some(0.9),
    });
    let summary = summarize_f0(
        range,
        440.0,
        &points,
        Some(PitchGrid::new(100_000, 10_000, 10).unwrap()),
    )
    .unwrap();
    assert!((summary.center_hz.unwrap() - 440.0).abs() < 0.01);
    assert_eq!(summary.voiced_ratio, Some(1.0));
    assert!(summary.pitch_mad_cents.unwrap().abs() < 0.01);
}

#[test]
fn sparse_pitch_grid_is_preserved_as_low_voiced_coverage() {
    let summary = summarize_f0(
        TimeRange::new(100_000, 500_000).unwrap(),
        440.0,
        &[F0Point {
            time: 110_000,
            hz: 440.0,
            confidence: Some(0.9),
        }],
        Some(PitchGrid::new(100_000, 10_000, 40).unwrap()),
    )
    .unwrap();
    assert_eq!(summary.center_hz, Some(440.0));
    assert!((summary.voiced_ratio.unwrap() - 0.025).abs() < 1.0e-6);
}

#[test]
fn supplied_low_confidence_f0_is_not_projected_as_voiced_pitch_support() {
    let summary = summarize_f0(
        TimeRange::new(100_000, 130_000).unwrap(),
        440.0,
        &[
            F0Point {
                time: 100_000,
                hz: 880.0,
                confidence: Some(0.01),
            },
            F0Point {
                time: 110_000,
                hz: 880.0,
                confidence: Some(0.01),
            },
            F0Point {
                time: 120_000,
                hz: 880.0,
                confidence: Some(0.01),
            },
        ],
        Some(PitchGrid::new(100_000, 10_000, 3).unwrap()),
    )
    .unwrap();
    assert_eq!(summary.center_hz, None);
    assert_eq!(summary.voiced_ratio, Some(0.0));
    assert_eq!(summary.confidence, None);
}

#[test]
fn hard_constraint_near_an_edge_bypasses_soft_cut_denoising() {
    let hard = BoundaryAlternative {
        source_expert: "caller".to_string(),
        range: TimeRange::new(110_000, 110_001).unwrap(),
        kind: BoundaryEvidenceKind::Constraint,
        fractional_midi: None,
        source_local_score: Some(1.0),
        hard: true,
    };
    let partitions = constraint_partition_challengers(&boundaries(69.0), &[hard]).unwrap();
    let ranges = partitions
        .iter()
        .map(|candidate| candidate.range)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(ranges.contains(&TimeRange::new(100_000, 110_000).unwrap()));
    assert!(ranges.contains(&TimeRange::new(110_000, 110_001).unwrap()));
    assert!(ranges.contains(&TimeRange::new(110_001, 500_000).unwrap()));
}

#[test]
fn candidate_evidence_relation_limit_is_exact() {
    use crate::fusion::candidate_states::MAX_CANDIDATE_EVIDENCE_RELATIONS;
    validate_candidate_evidence_relation_count(MAX_CANDIDATE_EVIDENCE_RELATIONS, 1).unwrap();
    assert!(
        validate_candidate_evidence_relation_count(MAX_CANDIDATE_EVIDENCE_RELATIONS + 1, 1)
            .is_err()
    );
}

#[test]
fn missing_note_length_evidence_fails_closed() {
    let evidence = BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: Vec::new(),
    };
    assert!(
        fuse_singing_evidence(
            &[],
            &evidence,
            "rmvpe",
            &[],
            None,
            &[],
            None,
            Some(&acoustic()),
            None,
        )
        .is_err()
    );
}

#[test]
fn acoustic_evidence_is_optional() {
    let fused = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[],
        None,
        &[],
        None,
        None,
        None,
    )
    .unwrap();
    assert!(fused.candidates[0].acoustic.is_none());
}

#[test]
fn basic_pitch_onset_survives_without_acoustic_dsp() {
    let evidence = BasicPitchEvidenceV3 {
        frames: vec![BasicPitchFrameV3 {
            time: 110_000,
            note_activation: 0.9,
            onset_activation: 0.8,
            contour_class: 42,
            contour_activation: 0.7,
        }],
        model_manifest_sha256: "a".repeat(64),
        runtime_manifest_sha256: "b".repeat(64),
    };
    let fused = fuse_singing_evidence(
        &[],
        &boundaries(69.0),
        "rmvpe",
        &[],
        None,
        &[],
        None,
        None,
        Some(&evidence),
    )
    .unwrap();
    let features = fused.candidates[0]
        .basic_pitch
        .as_ref()
        .expect("Basic Pitch onset support remains available without acoustic DSP");
    assert_eq!(features.onset_activation, 0.8);
    assert!(features.onset_supported);
}
