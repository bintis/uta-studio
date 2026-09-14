use super::*;
use crate::artifact::{
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrame,
};
use crate::fusion::{
    BoundarySegmentEvidence, CanonicalLyrics, HarmonyMetadata, LyricsAuthority,
    build_canonical_singing_track, decode_candidate_graph, fuse_singing_evidence,
    validate_candidate_path, validate_candidate_pool,
};

fn range(start: u64, end: u64) -> TimeRange {
    TimeRange::new(start, end).unwrap()
}

fn acoustic(frame_count: usize) -> AcousticEvidence {
    AcousticEvidence {
        contract: ACOUSTIC_EVIDENCE_CONTRACT.into(),
        version: ACOUSTIC_EVIDENCE_VERSION,
        algorithm: crate::fingerprint::ACOUSTIC_DSP_VERSION.into(),
        timebase: 1_000_000,
        start: 0,
        hop: 10_000,
        sample_rate: 16_000,
        window_samples: 512,
        semantic_audio_role: "lead_vocal".into(),
        decoded_audio_sha256: "a".repeat(64),
        frames: (0..frame_count)
            .map(|index| AcousticEvidenceFrame {
                start: index as u64 * 10_000,
                rms: 0.00001,
                spectral_flux: None,
                periodicity: 0.0,
                snr_db: 0.0,
                fundamental_hz: None,
                vibrato_activation: 0.0,
                glide_activation: 0.0,
                ornament_activation: 0.0,
                breath_activation: 0.0,
                voicing_transition_activation: 0.0,
            })
            .collect(),
    }
}

fn curve(parts: &[TimeRange], frame_count: usize) -> Vec<F0Point> {
    (0..frame_count)
        .map(|index| index as u64 * 10_000)
        .filter(|time| {
            parts
                .iter()
                .any(|part| part.start <= *time && *time < part.end)
        })
        .map(|time| F0Point {
            time,
            hz: 440.0,
            confidence: Some(0.9),
        })
        .collect()
}

fn fusion(
    parts: &[TimeRange],
    voiced: &[TimeRange],
    frame_count: usize,
) -> super::super::SingingFusionEvidence {
    let boundaries = BoundaryEvidenceSet {
        source_expert: "game".into(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: parts
            .iter()
            .map(|range| BoundarySegmentEvidence {
                range: *range,
                fractional_midi: Some(69.0),
                boundary_decision_parameter: None,
                presence_decision_parameter: None,
            })
            .collect(),
    };
    let points = curve(voiced, frame_count);
    let grid = Some(PitchGrid::new(0, 10_000, frame_count).unwrap());
    fuse_singing_evidence(
        &[],
        &boundaries,
        "rmvpe",
        &points,
        grid,
        &points,
        grid,
        Some(&acoustic(frame_count)),
        None,
    )
    .unwrap()
}

#[test]
fn missing_expert_is_unknown_and_does_not_make_a_silence_state() {
    let grid = Some(PitchGrid::new(0, 10_000, 100).unwrap());
    assert!(
        unsupported_voicing_ranges(&[], grid, &[], None, Some(&acoustic(100)))
            .unwrap()
            .is_empty()
    );
    assert!(
        unsupported_voicing_ranges(&[], grid, &[], grid, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn weak_neural_tail_and_quiet_periodic_tail_remain_supported() {
    let mut points = curve(&[range(0, 400_000)], 100);
    for point in &mut points {
        point.confidence = Some(0.01);
    }
    let mut dsp = acoustic(100);
    for frame in &mut dsp.frames[80..] {
        frame.periodicity = 0.9;
        frame.snr_db = 15.0;
        frame.rms = 0.00000001;
    }
    let grid = Some(PitchGrid::new(0, 10_000, 100).unwrap());
    assert_eq!(
        unsupported_voicing_ranges(&points, grid, &[], grid, Some(&dsp)).unwrap(),
        vec![range(400_000, 800_000)],
    );
}

#[test]
fn independent_absolute_grids_clip_support_without_erasing_subframe_edges() {
    let primary = vec![F0Point {
        time: 0,
        hz: 440.0,
        confidence: Some(0.1),
    }];
    let secondary = vec![F0Point {
        time: 5_000,
        hz: 440.0,
        confidence: None,
    }];
    let primary_grid = Some(PitchGrid::new(0, 10_000, 10).unwrap());
    let secondary_grid = Some(PitchGrid::new(5_000, 10_000, 10).unwrap());
    assert_eq!(
        unsupported_voicing_ranges(
            &primary,
            primary_grid,
            &secondary,
            secondary_grid,
            Some(&acoustic(11)),
        )
        .unwrap(),
        vec![range(15_000, 100_000)]
    );
}

#[test]
fn no_support_tail_terminates_while_exact_coverage_and_raw_pitch_remain_intact() {
    let voiced = [range(0, 800_000)];
    let result = fusion(&[range(0, 1_000_000)], &voiced, 100);
    let original = result
        .candidates
        .iter()
        .find(|candidate| candidate.boundary_role == BoundaryCandidateRole::Primary)
        .unwrap()
        .clone();
    assert_eq!(original.range, range(0, 1_000_000));
    let selected = decode_candidate_graph(&result.candidates).unwrap();
    validate_candidate_path(&result.candidates, &selected).unwrap();
    assert_eq!(selected.first().unwrap().range.start, 0);
    assert_eq!(selected.last().unwrap().range.end, 1_000_000);
    assert_eq!(
        selected
            .iter()
            .filter(|state| state.target.is_pitched())
            .count(),
        1
    );
    assert_eq!(
        selected
            .iter()
            .find(|state| state.target.is_pitched())
            .unwrap()
            .range,
        voiced[0]
    );
    let lyrics = CanonicalLyrics {
        text: "あ".into(),
        language: None,
        authority: LyricsAuthority::CallerCanonical,
        tokens: Vec::new(),
        confidence: None,
        source_experts: vec!["caller".into()],
        alternatives: Vec::new(),
    };
    let points = curve(&voiced, 100);
    let canonical = build_canonical_singing_track(
        lyrics.clone(),
        Vec::new(),
        selected.clone(),
        points.clone(),
        "rmvpe",
        HarmonyMetadata::default(),
        Vec::new(),
    )
    .unwrap();
    assert_eq!(canonical.notes.len(), 1);
    assert_eq!(canonical.transcript, lyrics);
    assert_eq!(canonical.f0_curve, points);
    let pitched_only = selected
        .into_iter()
        .filter(|state| state.target.is_pitched())
        .collect::<Vec<_>>();
    assert!(validate_candidate_path(&result.candidates, &pitched_only).is_err());
}

#[test]
fn a_small_recovery_without_its_own_onset_is_absorbed_by_the_next_native_note() {
    let result = fusion(
        &[range(0, 500_000), range(500_000, 1_000_000)],
        &[range(0, 400_000), range(480_000, 1_000_000)],
        100,
    );
    let selected = decode_candidate_graph(&result.candidates).unwrap();
    validate_candidate_path(&result.candidates, &selected).unwrap();
    let pitched = selected
        .iter()
        .filter(|state| state.target.is_pitched())
        .collect::<Vec<_>>();
    assert_eq!(
        pitched.len(),
        2,
        "a 20ms inherited target must not become a third note"
    );
    assert_eq!(pitched[0].range, range(0, 400_000));
    assert_eq!(pitched[1].range, range(480_000, 1_000_000));
}

#[test]
fn voiced_recovery_can_split_a_long_native_note_without_inventing_a_native_vote() {
    let result = fusion(
        &[range(0, 1_000_000)],
        &[range(0, 400_000), range(600_000, 1_000_000)],
        100,
    );
    let selected = decode_candidate_graph(&result.candidates).unwrap();
    let pitched = selected
        .iter()
        .filter(|state| state.target.is_pitched())
        .collect::<Vec<_>>();
    assert_eq!(pitched.len(), 2);
    assert_eq!(pitched[0].range, range(0, 400_000));
    assert_eq!(pitched[1].range, range(600_000, 1_000_000));
    // GAME, RMVPE and FCPE target proposals share the same native
    // duration observation; pitch alternatives must not count as new onsets.
    let native_boundaries = result
        .candidates
        .iter()
        .filter(|state| state.boundary_kind == BoundaryEvidenceKind::Game)
        .map(|state| {
            (
                state.boundary_source.as_str(),
                state.range.start,
                state.range.end,
            )
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        native_boundaries,
        std::collections::BTreeSet::from([("game", 0, 1_000_000)]),
    );
}

#[test]
fn a_legitimate_short_voiced_note_is_never_removed_by_the_gap_duration_rule() {
    let result = fusion(
        &[
            range(0, 400_000),
            range(400_000, 420_000),
            range(420_000, 1_000_000),
        ],
        &[
            range(0, 300_000),
            range(400_000, 420_000),
            range(520_000, 1_000_000),
        ],
        100,
    );
    let selected = decode_candidate_graph(&result.candidates).unwrap();
    assert!(selected.iter().any(|state| state.target.is_pitched()
        && state.range.start <= 400_000
        && state.range.end >= 420_000));
    assert!(
        selected
            .iter()
            .filter(|state| !state.target.is_pitched())
            .all(|state| !state.range.overlaps(range(400_000, 420_000)))
    );
}

#[test]
fn unpitched_covering_states_need_evidence_and_cannot_multiply_occupancy_credit() {
    let result = fusion(&[range(0, 1_000_000)], &[range(0, 800_000)], 100);
    let whole = result
        .candidates
        .iter()
        .find(|state| !state.target.is_pitched() && state.range == range(800_000, 1_000_000))
        .unwrap();
    let mut before = whole.clone();
    before.id = "rest-before".into();
    before.range.end = 877_777;
    before.voicing_evidence = voicing_evidence(before.range, &[whole.range]);
    let mut after = whole.clone();
    after.id = "rest-after".into();
    after.range.start = before.range.end;
    after.voicing_evidence = voicing_evidence(after.range, &[whole.range]);
    assert!(
        (before.emission_utility().unwrap() + after.emission_utility().unwrap()
            - whole.emission_utility().unwrap())
        .abs()
            < 0.000001
    );
    let mut invented = whole.clone();
    invented.voicing_evidence = None;
    assert!(validate_candidate_pool(&[invented]).is_err());
    assert_eq!(
        serde_json::to_value(whole.target).unwrap(),
        serde_json::json!({"kind":"unpitched"})
    );
}

#[test]
fn an_explicit_unpitched_state_resets_pitch_motion_without_an_onset_bonus() {
    let result = fusion(&[range(0, 1_000_000)], &[range(0, 800_000)], 100);
    let pitched = result
        .candidates
        .iter()
        .find(|state| state.target.is_pitched())
        .unwrap();
    let rest = result
        .candidates
        .iter()
        .find(|state| !state.target.is_pitched())
        .unwrap();
    let boundaries = crate::fusion::HardBoundarySet::default();
    assert_eq!(
        crate::fusion::hsmm::transition_utility(pitched, rest, &boundaries, &[]),
        0.0
    );
    assert_eq!(
        crate::fusion::hsmm::transition_utility(rest, pitched, &boundaries, &[]),
        0.0
    );
}

#[test]
fn derived_voicing_geometry_keeps_native_ids_and_has_distinct_candidate_ids() {
    let result = fusion(&[range(0, 1_000_000)], &[range(0, 800_000)], 100);
    validate_candidate_pool(&result.candidates).unwrap();
    let native = result
        .candidates
        .iter()
        .find(|state| state.boundary_kind == BoundaryEvidenceKind::Game)
        .unwrap();
    assert_eq!(native.id, "game-segment-0");
    assert!(result.candidates.iter().any(|state| {
        state.target.is_pitched() && state.boundary_kind == BoundaryEvidenceKind::Voicing
    }));
    let ids = result
        .candidates
        .iter()
        .map(|state| state.id.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), result.candidates.len());
}
