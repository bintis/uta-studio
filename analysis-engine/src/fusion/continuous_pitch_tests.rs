use super::*;

fn fit_curve(
    range: TimeRange,
    target_hz: f32,
    curve: &[F0Point],
    grid: Option<PitchGrid>,
) -> Result<Option<ContinuousPitchFit>, String> {
    let observations = continuous_pitch_observations(curve, grid, None)?;
    Ok(continuous_pitch_fit(
        range,
        target_hz,
        observations.as_deref(),
    ))
}

fn acoustic_curve(start: u64, pitches: &[f32], periodicity: f32, snr_db: f32) -> AcousticEvidence {
    use crate::artifact::{
        ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrame,
    };
    AcousticEvidence {
        contract: ACOUSTIC_EVIDENCE_CONTRACT.to_string(),
        version: ACOUSTIC_EVIDENCE_VERSION,
        algorithm: crate::fingerprint::ACOUSTIC_DSP_VERSION.to_string(),
        timebase: 1_000_000,
        start,
        hop: 10_000,
        sample_rate: 16_000,
        window_samples: 512,
        semantic_audio_role: "lead_vocal".to_string(),
        decoded_audio_sha256: "a".repeat(64),
        frames: pitches
            .iter()
            .enumerate()
            .map(|(index, pitch)| AcousticEvidenceFrame {
                start: start + index as u64 * 10_000,
                rms: 0.2,
                spectral_flux: None,
                periodicity,
                snr_db,
                fundamental_hz: Some(midi_hz(*pitch)),
                vibrato_activation: 0.0,
                glide_activation: 0.0,
                ornament_activation: 0.0,
                breath_activation: 0.0,
                voicing_transition_activation: 0.0,
            })
            .collect(),
    }
}

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

    let merged = fit_curve(whole, midi_hz(60.0), &curve, grid)
        .unwrap()
        .unwrap();
    let lower = fit_curve(
        TimeRange::new(100_000, 260_000).unwrap(),
        midi_hz(58.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let upper = fit_curve(
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
    let whole = fit_curve(
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
            fit_curve(
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
    let missing = fit_curve(
        TimeRange::new(110_000, 120_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let measured = fit_curve(
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
        fit_curve(
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
    let fit = fit_curve(
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
            if candidate.target.midi() == Some(expected_midi) {
                assert_eq!(error, 0.0);
            } else {
                assert!((error - 0.6).abs() < 0.00001);
            }
        }
    }
}

#[test]
fn independent_reliable_dsp_retains_an_octave_alternative_without_rewriting_primary() {
    let curve = measured_curve(std::iter::repeat_n(81.0, 30));
    let original = curve.clone();
    let acoustic = acoustic_curve(100_000, &[69.0; 30], 0.8, 20.0);
    let observations = continuous_pitch_observations(
        &curve,
        Some(PitchGrid::new(100_000, 10_000, 30).unwrap()),
        Some(&acoustic),
    )
    .unwrap();
    for midi in [69.0, 81.0] {
        let fit = continuous_pitch_fit(
            TimeRange::new(100_000, 400_000).unwrap(),
            midi_hz(midi),
            observations.as_deref(),
        )
        .unwrap();
        assert_eq!(fit.error_integral, 0.0);
        assert_eq!(fit.observed_duration, 300_000);
    }
    assert_eq!(curve, original);
}

#[test]
fn noisy_or_weakly_periodic_dsp_cannot_erase_a_real_short_pitch_plateau() {
    let curve = measured_curve(std::iter::repeat_n(58.0, 16).chain(std::iter::repeat_n(60.0, 42)));
    for (periodicity, snr_db) in [(0.8, 3.0), (0.2, 20.0)] {
        let acoustic = acoustic_curve(100_000, &[60.0; 58], periodicity, snr_db);
        let observations = continuous_pitch_observations(
            &curve,
            Some(PitchGrid::new(100_000, 10_000, 58).unwrap()),
            Some(&acoustic),
        )
        .unwrap();
        let fit = continuous_pitch_fit(
            TimeRange::new(100_000, 680_000).unwrap(),
            midi_hz(60.0),
            observations.as_deref(),
        )
        .unwrap();
        assert!((fit.error_integral - 0.24).abs() < 0.00001);
        assert_eq!(fit.observed_duration, 580_000);
    }
}

#[test]
fn pitch_fit_stays_additive_across_offset_dsp_frames_and_subframe_cuts() {
    let curve = measured_curve([81.0; 3]);
    let acoustic = acoustic_curve(105_000, &[69.0, 81.0], 0.8, 20.0);
    let observations = continuous_pitch_observations(
        &curve,
        Some(PitchGrid::new(100_000, 10_000, 3).unwrap()),
        Some(&acoustic),
    )
    .unwrap();
    let fit = |start, end| {
        continuous_pitch_fit(
            TimeRange::new(start, end).unwrap(),
            midi_hz(69.0),
            observations.as_deref(),
        )
        .unwrap()
    };
    let whole = fit(100_000, 130_000);
    let cuts = [100_000, 103_111, 107_222, 115_555, 122_777, 130_000];
    let parts = cuts
        .windows(2)
        .map(|pair| fit(pair[0], pair[1]))
        .collect::<Vec<_>>();
    let error_sum = parts.iter().map(|part| part.error_integral).sum::<f32>();
    let covered_sum = parts.iter().map(|part| part.observed_duration).sum::<u64>();
    assert!((whole.error_integral - 0.23).abs() < 0.00001);
    assert!((error_sum - whole.error_integral).abs() < 0.000001);
    assert_eq!(covered_sum, whole.observed_duration);
    assert_eq!(covered_sum, 30_000);
}

#[test]
fn isolated_large_pitch_excursions_have_bounded_but_additive_influence() {
    let curve = measured_curve([69.0, 93.0, 69.0]);
    let grid = Some(PitchGrid::new(100_000, 10_000, 3).unwrap());
    let whole = fit_curve(
        TimeRange::new(100_000, 130_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let left = fit_curve(
        TimeRange::new(100_000, 115_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    let right = fit_curve(
        TimeRange::new(115_000, 130_000).unwrap(),
        midi_hz(69.0),
        &curve,
        grid,
    )
    .unwrap()
    .unwrap();
    assert!((whole.error_integral - 0.015).abs() < 0.000001);
    assert!((whole.error_integral - left.error_integral - right.error_integral).abs() < 0.000001);
    assert_eq!(whole.observed_duration, 30_000);
    assert_eq!(curve[1].hz, midi_hz(93.0));
}
