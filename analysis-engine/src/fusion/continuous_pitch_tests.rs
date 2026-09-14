use super::*;

fn measured_curve(pitches: impl IntoIterator<Item = f32>) -> Vec<F0Point> {
    pitches
        .into_iter()
        .enumerate()
        .map(|(index, midi)| F0Point {
            time: 100_000 + index as u64 * 10_000,
            hz: midi_hz(midi),
            confidence: Some(0.9),
        })
        .collect()
}

#[test]
fn pitch_loss_retains_a_short_real_plateau_hidden_by_the_range_median() {
    let curve = measured_curve(std::iter::repeat_n(58.0, 16).chain(std::iter::repeat_n(60.0, 42)));
    let grid = Some(PitchGrid::new(100_000, 10_000, 58).unwrap());
    let whole = TimeRange::new(100_000, 680_000).unwrap();
    let median = summarize_f0(whole, midi_hz(60.0), &curve, grid).unwrap();
    assert!(median.cents_difference.unwrap().abs() < 0.01);

    let merged = continuous_pitch_fit(whole, midi_hz(60.0), &curve, grid)
        .unwrap()
        .unwrap();
    let lower = continuous_pitch_fit(
        TimeRange::new(100_000, 260_000).unwrap(),
        midi_hz(58.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let upper = continuous_pitch_fit(
        TimeRange::new(260_000, 680_000).unwrap(),
        midi_hz(60.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    assert!((merged.error_integral - 0.24).abs() < 0.00001);
    assert_eq!(merged.observed_duration, 580_000);
    assert_eq!(lower.error_integral + upper.error_integral, 0.0);
}

#[test]
fn pitch_loss_is_additive_for_arbitrary_subframe_cuts_and_gaps() {
    let mut curve = measured_curve([58.0, 59.0, 60.0, 61.0, 62.0, 60.0]);
    curve.remove(2);
    let grid = Some(PitchGrid::new(100_000, 10_000, 6).unwrap());
    let cuts = [100_003, 104_321, 112_999, 126_001, 139_876, 159_999];
    let whole = continuous_pitch_fit(
        TimeRange::new(cuts[0], *cuts.last().unwrap()).unwrap(),
        midi_hz(60.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let parts = cuts
        .windows(2)
        .map(|pair| {
            continuous_pitch_fit(
                TimeRange::new(pair[0], pair[1]).unwrap(),
                midi_hz(60.0),
                &curve,
                grid,
            )
            .unwrap()
            .unwrap()
        })
        .collect::<Vec<_>>();
    let error_sum = parts.iter().map(|part| part.error_integral).sum::<f32>();
    let covered_sum = parts.iter().map(|part| part.observed_duration).sum::<u64>();
    assert!((error_sum - whole.error_integral).abs() < 0.000001);
    assert_eq!(covered_sum, whole.observed_duration);
    assert_eq!(covered_sum, 49_996);
}

#[test]
fn pitch_loss_distinguishes_missing_low_confidence_and_measured_zero_error() {
    let mut curve = measured_curve([69.0, 81.0, 69.0]);
    curve[1].confidence = Some(0.1);
    let grid = Some(PitchGrid::new(100_000, 10_000, 3).unwrap());
    let missing = continuous_pitch_fit(
        TimeRange::new(110_000, 120_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let measured = continuous_pitch_fit(
        TimeRange::new(100_000, 110_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    assert_eq!(missing.error_integral, 0.0);
    assert_eq!(missing.observed_duration, 0);
    assert_eq!(measured.error_integral, 0.0);
    assert_eq!(measured.observed_duration, 10_000);
    assert!(
        continuous_pitch_fit(
            TimeRange::new(100_000, 130_000).unwrap(),
            midi_hz(69.0),
            &curve,
            None,
        )
        .unwrap()
        .is_none()
    );
}

#[test]
fn pitch_loss_tolerates_vibrato_without_erasing_continuous_contour() {
    let curve = measured_curve([68.6, 68.8, 69.0, 69.2, 69.4, 69.0]);
    let original = curve.clone();
    let fit = continuous_pitch_fit(
        TimeRange::new(100_000, 160_000).unwrap(),
        midi_hz(69.0),
        &curve,
        Some(PitchGrid::new(100_000, 10_000, 6).unwrap()),
    )
    .unwrap()
    .unwrap();
    assert_eq!(fit.error_integral, 0.0);
    assert_eq!(fit.observed_duration, 60_000);
    assert_eq!(curve, original);
}

#[test]
fn every_pitch_proposal_is_scored_against_the_declared_owner() {
    let lower_curve = measured_curve(std::iter::repeat_n(67.0, 40));
    let upper_curve = measured_curve(std::iter::repeat_n(69.0, 40));
    let grid = Some(PitchGrid::new(100_000, 10_000, 40).unwrap());
    let boundaries = BoundaryEvidenceSet {
        source_expert: "game".to_string(),
        kind: BoundaryEvidenceKind::Game,
        model_hash: None,
        runtime_identity: None,
        segments: vec![BoundarySegmentEvidence {
            range: TimeRange::new(100_000, 500_000).unwrap(),
            fractional_midi: Some(69.0),
            boundary_decision_parameter: Some(0.2),
            presence_decision_parameter: Some(0.2),
        }],
    };
    for (owner, expected_midi) in [("rmvpe", 67), ("fcpe", 69)] {
        let evidence = fuse_singing_evidence(
            &[],
            &boundaries,
            owner,
            &lower_curve,
            grid,
            &upper_curve,
            grid,
            None,
            None,
        )
        .unwrap();
        assert!(evidence.candidates.len() >= 2);
        for candidate in evidence.candidates {
            assert_eq!(candidate.continuous_pitch_observed_duration, Some(400_000));
            let error = candidate.continuous_pitch_error_integral.unwrap();
            if candidate.target_midi == expected_midi {
                assert_eq!(error, 0.0);
            } else {
                assert!((error - 0.6).abs() < 0.00001);
            }
        }
    }
}
