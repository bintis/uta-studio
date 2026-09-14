use super::*;

fn sustain(id: &str, start: u64, end: u64) -> SegmentCandidate {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "range": { "start": start, "end": end },
        "target_midi": 69,
        "boundary_source": "game",
        "boundary_kind": "game",
        "boundary_fractional_midi": 69.0,
        "target_pitch_source": "game",
        "center_pitch_hz": 440.0,
        "rmvpe_center_hz": 440.0,
        "rmvpe_cents_difference": 0.0,
        "rmvpe_voiced_ratio": 1.0,
        "rmvpe_pitch_mad_cents": 0.0,
        "basic_pitch": {
            "onset_activation": 0.0,
            "note_activation": 0.9,
            "contour_activation": 0.9,
            "contour_class": 42,
            "onset_supported": false
        },
        "word_id": "held-word"
    }))
    .unwrap()
}

fn onset_constraint(time: u64) -> BoundaryConstraintEvidence {
    BoundaryConstraintEvidence {
        source_expert: "basic_pitch".to_string(),
        kind: BoundaryConstraintKind::BasicPitchOnset,
        time,
        source_local_strength: Some(0.95),
        calibrated_confidence: None,
        calibration_version: None,
        correlation_group: None,
        depends_on: Vec::new(),
    }
}

fn add_basic_pitch_attack(candidate: &mut SegmentCandidate) {
    let features = candidate.basic_pitch.as_mut().unwrap();
    features.onset_activation = 0.95;
    features.onset_supported = true;
}

fn add_acoustic_attack(candidate: &mut SegmentCandidate) {
    candidate.acoustic = Some(AcousticCandidateFeatures {
        onset_supported: Some(true),
        ..AcousticCandidateFeatures::default()
    });
}

#[test]
fn relabeling_an_attack_as_a_boundary_and_context_does_not_multiply_its_reward() {
    let mut attack = sustain("attack", 500_000, 1_000_000);
    add_basic_pitch_attack(&mut attack);
    let original_reward = attack.emission_utility().unwrap();
    let original_strength = note_event_support(&attack);
    attack.boundary_source = "basic_pitch.onset".to_string();
    attack.boundary_kind = BoundaryEvidenceKind::BasicPitchOnset;
    attack.boundary_support = Some(0.95);
    attack.boundary_constraints = vec![onset_constraint(500_000); 4];

    assert_eq!(attack.emission_utility().unwrap(), original_reward);
    assert_eq!(note_event_support(&attack), original_strength);
}

#[test]
fn an_event_at_the_end_cannot_pay_for_an_onset_at_the_start() {
    let mut note = sustain("held", 0, 500_000);
    let original_reward = note.emission_utility().unwrap();
    note.boundary_constraints.push(onset_constraint(500_000));
    assert_eq!(note.emission_utility().unwrap(), original_reward);
    assert_eq!(note.boundary_constraints.len(), 1, "retain the raw context");
}

#[test]
fn a_peak_shared_by_two_short_states_belongs_to_its_nearest_edge() {
    let mut prefix = sustain("prefix", 460_000, 500_000);
    let mut attack = sustain("attack", 500_000, 560_000);
    for note in [&mut prefix, &mut attack] {
        add_basic_pitch_attack(note);
        note.boundary_constraints.push(onset_constraint(500_000));
    }
    assert!(!onset_supported(&prefix));
    assert_eq!(note_event_support(&prefix), 0.0);
    assert!(onset_supported(&attack));
    assert!(note_event_support(&attack) > 0.0);
}

#[test]
fn independent_acoustic_evidence_adds_support() {
    let mut attack = sustain("attack", 500_000, 1_000_000);
    add_basic_pitch_attack(&mut attack);
    let original_reward = attack.emission_utility().unwrap();
    let original_strength = note_event_support(&attack);
    add_acoustic_attack(&mut attack);
    assert!(attack.emission_utility().unwrap() > original_reward);
    assert!(note_event_support(&attack) > original_strength);
}

#[test]
fn one_source_copied_into_three_views_cannot_split_a_stable_pitch_plateau() {
    let wide = sustain("wide", 0, 1_000_000);
    let mut first = sustain("first", 0, 500_000);
    let mut second = sustain("second", 500_000, 1_000_000);
    for note in [&mut first, &mut second] {
        note.boundary_source = "basic_pitch.onset".to_string();
        note.boundary_kind = BoundaryEvidenceKind::BasicPitchOnset;
        note.boundary_role = BoundaryCandidateRole::Challenger;
    }
    add_basic_pitch_attack(&mut second);
    second.boundary_support = Some(0.95);
    second.boundary_constraints.push(onset_constraint(500_000));

    let selected = decode_candidate_graph(&[wide.clone(), first.clone(), second.clone()]).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "wide");

    add_acoustic_attack(&mut second);
    let selected = decode_candidate_graph(&[wide, first, second]).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|note| note.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "second"]
    );
}

#[test]
fn short_independent_reattacks_remain_valid_segmentation_states() {
    let wide = sustain("wide", 0, 1_000_000);
    let first = sustain("first", 0, 475_000);
    let mut short = sustain("short", 475_000, 525_000);
    let mut last = sustain("last", 525_000, 1_000_000);
    for note in [&mut short, &mut last] {
        add_basic_pitch_attack(note);
        add_acoustic_attack(note);
        note.boundary_constraints
            .push(onset_constraint(note.range.start));
    }
    let selected = decode_candidate_graph(&[wide, first, short, last]).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|note| note.id.as_str())
            .collect::<Vec<_>>(),
        ["first", "short", "last"]
    );
    assert_eq!(selected[1].range.end - selected[1].range.start, 50_000);
}

#[test]
fn a_single_peak_before_two_neighboring_starts_is_not_a_second_attack() {
    let mut first = sustain("first", 500_000, 530_000);
    let mut second = sustain("second", 530_000, 1_000_000);
    for note in [&mut first, &mut second] {
        add_basic_pitch_attack(note);
        note.boundary_constraints.push(onset_constraint(490_000));
    }
    assert!(onset_supported(&first));
    assert!(onset_supported(&second));
    assert!(!distinct_onset_supported(&first, &second));
    assert!(repeated_attack_reward(&first, &second) > 0.0);

    second.boundary_constraints = vec![onset_constraint(550_000)];
    assert!(distinct_onset_supported(&first, &second));
    assert_eq!(repeated_attack_reward(&first, &second), 0.0);
}

#[test]
fn full_attack_window_keeps_the_event_identity_for_path_deduplication() {
    let mut notes = vec![sustain("first", 500_000, 540_000)];
    attach_boundary_constraints(&mut notes, &[onset_constraint(445_000)]).unwrap();
    assert_eq!(notes[0].boundary_constraints.len(), 1);
}

#[test]
fn real_pitch_change_can_defeat_a_long_note_whose_median_hides_the_shorter_pitch() {
    let mut wide = sustain("wide", 0, 580_000);
    wide.continuous_pitch_error_integral = Some(0.24);
    wide.continuous_pitch_observed_duration = Some(580_000);
    let mut before = sustain("lower", 0, 160_000);
    let mut after = sustain("upper", 160_000, 580_000);
    for note in [&mut before, &mut after] {
        note.boundary_kind = BoundaryEvidenceKind::F0Transition;
        note.boundary_source = "rmvpe.transition".to_string();
        note.boundary_role = BoundaryCandidateRole::Challenger;
        note.boundary_support = Some(1.0 / 6.0);
        note.continuous_pitch_error_integral = Some(0.0);
        note.continuous_pitch_observed_duration = Some(note.range.end - note.range.start);
    }
    before.target_midi = 67;
    before.center_pitch_hz = 440.0 * 2.0_f32.powf(-2.0 / 12.0);
    let selected = decode_candidate_graph(&[wide, before, after]).unwrap();
    assert_eq!(
        selected
            .iter()
            .map(|note| note.id.as_str())
            .collect::<Vec<_>>(),
        ["lower", "upper"]
    );
}

#[test]
fn dividing_one_pitch_target_cannot_improve_the_integrated_pitch_fit() {
    let mut wide = sustain("wide", 0, 1_000_000);
    let mut first = sustain("first", 0, 500_000);
    let mut last = sustain("last", 500_000, 1_000_000);
    wide.continuous_pitch_error_integral = Some(0.1);
    first.continuous_pitch_error_integral = Some(0.04);
    last.continuous_pitch_error_integral = Some(0.06);
    let selected = decode_candidate_graph(&[wide, first, last]).unwrap();
    assert_eq!(selected.len(), 1);
    assert_eq!(selected[0].id, "wide");
}
