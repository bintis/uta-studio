use serde::{Deserialize, Serialize};

use crate::artifact::{AcousticEvidenceV1, BasicPitchEvidenceV3, GameEvidenceV1};

use super::{
    AcousticCandidateFeatures, CanonicalWordBoundary, F0Point, PitchAlternative, SegmentCandidate,
    TechniqueScores, TimeRange,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingFusionEvidence {
    pub schema_version: u32,
    pub candidates: Vec<SegmentCandidate>,
}

/// Explicit GAME fractional-MIDI target decision. GAME is the sole target-note
/// source in the baseline. Nearest-semitone rounding occurs here, after typed
/// parsing, and the fractional estimate remains on the candidate so tuning
/// residuals can be retained in the final chart.
fn decide_game_target(midi: f32) -> Result<u8, String> {
    if !midi.is_finite() || !(0.0..128.0).contains(&midi) {
        return Err("GAME fractional MIDI is outside the target-note domain".to_string());
    }
    let rounded = midi.round();
    if !(0.0..=127.0).contains(&rounded) {
        return Err("GAME fractional MIDI rounds outside MIDI 0..127".to_string());
    }
    Ok(rounded as u8)
}

fn midi_hz(midi: f32) -> f32 {
    440.0 * 2.0_f32.powf((midi - 69.0) / 12.0)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchGrid {
    pub start: u64,
    pub hop: u64,
    pub frame_count: usize,
}

impl PitchGrid {
    pub fn new(start: u64, hop: u64, frame_count: usize) -> Result<Self, String> {
        if hop == 0 || frame_count == 0 {
            return Err("continuous F0 grid is empty or has zero hop".to_string());
        }
        Ok(Self {
            start,
            hop,
            frame_count,
        })
    }

    fn frames_in_range(self, range: TimeRange) -> Result<usize, String> {
        if range.end <= range.start {
            return Err("continuous F0 summary range is empty".to_string());
        }
        if range.end <= self.start {
            return Ok(0);
        }
        let lower = range.start.max(self.start) - self.start;
        let upper = range.end - self.start;
        let first = ceil_div(lower, self.hop);
        let end_exclusive = ceil_div(upper, self.hop);
        let total = u64::try_from(self.frame_count)
            .map_err(|_| "continuous F0 grid frame count overflows".to_string())?;
        usize::try_from(end_exclusive.min(total).saturating_sub(first.min(total)))
            .map_err(|_| "continuous F0 range frame count overflows".to_string())
    }
}

fn ceil_div(value: u64, divisor: u64) -> u64 {
    if value == 0 {
        0
    } else {
        1 + (value - 1) / divisor
    }
}

fn overlap_duration(left: TimeRange, right: TimeRange) -> u64 {
    left.end
        .min(right.end)
        .saturating_sub(left.start.max(right.start))
}

fn linked_word(note: TimeRange, words: &[CanonicalWordBoundary]) -> Option<String> {
    words
        .iter()
        .map(|word| (overlap_duration(note, word.range), word))
        .filter(|(overlap, _)| *overlap > 0)
        .max_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| right.1.word_id.cmp(&left.1.word_id))
        })
        .map(|(_, word)| word.word_id.clone())
}

struct ContinuousF0Summary {
    center_hz: Option<f32>,
    confidence: Option<f32>,
    cents_difference: Option<f32>,
    voiced_ratio: Option<f32>,
    pitch_mad_cents: Option<f32>,
}

fn weighted_median(mut values: Vec<(f32, f32)>) -> Result<f32, String> {
    if values.is_empty()
        || values
            .iter()
            .any(|(value, weight)| !value.is_finite() || !weight.is_finite() || *weight < 0.0)
    {
        return Err("continuous F0 weighted median input is invalid".to_string());
    }
    values.sort_by(|left, right| left.0.total_cmp(&right.0));
    let total = values.iter().map(|(_, weight)| *weight).sum::<f32>();
    if !total.is_finite() || total <= f32::EPSILON {
        return Err("continuous F0 weighted median has no positive weight".to_string());
    }
    let threshold = total * 0.5;
    let mut cumulative = 0.0_f32;
    for (value, weight) in &values {
        cumulative += *weight;
        if cumulative >= threshold {
            return Ok(*value);
        }
    }
    Ok(values.last().expect("non-empty checked").0)
}

fn summarize_f0(
    range: TimeRange,
    target_hz: f32,
    f0_curve: &[F0Point],
    grid: Option<PitchGrid>,
) -> Result<ContinuousF0Summary, String> {
    if !target_hz.is_finite() || target_hz <= 0.0 {
        return Err("continuous F0 target is invalid".to_string());
    }
    let observed = f0_curve
        .iter()
        .filter(|point| point.time >= range.start && point.time < range.end)
        .collect::<Vec<_>>();
    if observed.iter().any(|point| {
        !point.hz.is_finite()
            || point.hz <= 0.0
            || point
                .confidence
                .is_some_and(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
    }) {
        return Err("continuous F0 contains invalid values".to_string());
    }

    let expected_frames = grid.map(|grid| grid.frames_in_range(range)).transpose()?;
    if expected_frames.is_some_and(|expected| observed.len() > expected) {
        return Err("continuous F0 contains more voiced points than grid frames".to_string());
    }
    let voiced_ratio = expected_frames
        .filter(|expected| *expected > 0)
        .map(|expected| observed.len() as f32 / expected as f32);
    if observed.is_empty() {
        return Ok(ContinuousF0Summary {
            center_hz: None,
            confidence: None,
            cents_difference: None,
            voiced_ratio,
            pitch_mad_cents: None,
        });
    }

    let all_confident = observed.iter().all(|point| point.confidence.is_some());
    let confidence_total = observed
        .iter()
        .filter_map(|point| point.confidence)
        .sum::<f32>();
    let use_confidence_weights = all_confident && confidence_total > f32::EPSILON;
    let cents_and_weights = observed
        .iter()
        .map(|point| {
            let cents = 1_200.0 * (point.hz / target_hz).log2();
            let weight = if use_confidence_weights {
                point.confidence.expect("all confidence values checked")
            } else {
                1.0
            };
            (cents, weight)
        })
        .collect::<Vec<_>>();
    let center_cents = weighted_median(cents_and_weights.clone())?;
    let center = target_hz * 2.0_f32.powf(center_cents / 1_200.0);
    let pitch_mad_cents = weighted_median(
        cents_and_weights
            .iter()
            .map(|(cents, weight)| ((cents - center_cents).abs(), *weight))
            .collect(),
    )?;
    let confidence = all_confident.then(|| confidence_total / observed.len() as f32);
    if !center.is_finite()
        || center <= 0.0
        || !center_cents.is_finite()
        || !pitch_mad_cents.is_finite()
        || voiced_ratio.is_some_and(|ratio| !ratio.is_finite() || !(0.0..=1.0).contains(&ratio))
    {
        return Err("continuous F0 summary is invalid".to_string());
    }
    Ok(ContinuousF0Summary {
        center_hz: Some(center),
        confidence,
        cents_difference: Some(center_cents),
        voiced_ratio,
        pitch_mad_cents: Some(pitch_mad_cents),
    })
}

fn summarize_acoustic(
    range: TimeRange,
    evidence: &AcousticEvidenceV1,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
) -> Result<AcousticCandidateFeatures, String> {
    evidence.validate().map_err(|error| error.message)?;
    let window_duration = u64::from(evidence.window_samples)
        .checked_mul(1_000_000)
        .map(|units| units / u64::from(evidence.sample_rate))
        .ok_or_else(|| "acoustic DSP window duration overflows".to_string())?;
    let frames = evidence
        .frames
        .iter()
        .filter(|frame| {
            frame.start < range.end && frame.start.saturating_add(window_duration) > range.start
        })
        .collect::<Vec<_>>();
    if frames.is_empty() {
        return Err("GAME note has no overlapping acoustic DSP frame".to_string());
    }
    let frame_count = frames.len();
    let mean_rms = frames.iter().map(|frame| frame.rms).sum::<f32>() / frame_count as f32;
    let mean_periodicity =
        frames.iter().map(|frame| frame.periodicity).sum::<f32>() / frame_count as f32;
    let mean_snr_db = frames.iter().map(|frame| frame.snr_db).sum::<f32>() / frame_count as f32;
    let onset_index = evidence
        .frames
        .iter()
        .enumerate()
        .min_by_key(|(_, frame)| frame.start.abs_diff(range.start))
        .map(|(index, _)| index)
        .expect("validated acoustic evidence is non-empty");
    let onset_flux = evidence.frames[onset_index].spectral_flux;
    let preceding_flux = onset_index
        .checked_sub(1)
        .and_then(|index| evidence.frames[index].spectral_flux);
    // fusion-v2 mapping: an onset is supported when measured flux is at least
    // twice the immediately preceding frame and exceeds the finite DSP floor.
    // This boolean is a transition feature, never a probability/confidence.
    let onset_supported = onset_flux
        .zip(preceding_flux)
        .map(|(onset, preceding)| onset >= (preceding * 2.0).max(1.0e-6));
    const ONSET_WINDOW: u64 = 60_000;
    let onset_window_start = range.start.saturating_sub(ONSET_WINDOW);
    let onset_window_end = range.start.saturating_add(ONSET_WINDOW);
    let basic_pitch_onset_activation = basic_pitch.and_then(|evidence| {
        evidence
            .frames
            .iter()
            .filter(|frame| frame.time >= onset_window_start && frame.time < onset_window_end)
            .map(|frame| frame.onset_activation)
            .max_by(f32::total_cmp)
    });
    // fusion-v4 source-local decision. It is evidence supporting a transition,
    // never a calibrated probability and never a GAME replacement.
    let basic_pitch_onset_supported =
        basic_pitch_onset_activation.map(|activation| activation >= 0.5);
    Ok(AcousticCandidateFeatures {
        frame_count,
        mean_rms,
        mean_periodicity,
        mean_snr_db,
        onset_flux,
        preceding_flux,
        onset_supported,
        basic_pitch_onset_activation,
        basic_pitch_onset_supported,
    })
}

/// Produces GAME-anchored duration candidates without decoding them. RMVPE is
/// retained as primary continuous agreement/conflict evidence; FCPE is a
/// correlated secondary opinion and never replaces RMVPE. Neither continuous
/// expert creates or replaces a GAME target MIDI note. Acoustic DSP contributes
/// raw summaries and the versioned onset-support transition feature.
#[allow(clippy::too_many_arguments)]
pub fn fuse_singing_evidence(
    words: &[CanonicalWordBoundary],
    game: &GameEvidenceV1,
    rmvpe_curve: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe_curve: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
    acoustic: &AcousticEvidenceV1,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
) -> Result<SingingFusionEvidence, String> {
    if game.notes.is_empty() {
        return Err("real GAME evidence is required for baseline note fusion".to_string());
    }
    if rmvpe_curve
        .windows(2)
        .any(|pair| pair[0].time >= pair[1].time)
        || fcpe_curve
            .windows(2)
            .any(|pair| pair[0].time >= pair[1].time)
    {
        return Err("continuous F0 must be strictly ordered".to_string());
    }
    let mut candidates = Vec::with_capacity(game.notes.len());
    for (index, note) in game.notes.iter().enumerate() {
        let target_midi = decide_game_target(note.midi)?;
        let center_pitch_hz = midi_hz(note.midi);
        let rmvpe = summarize_f0(note.range, center_pitch_hz, rmvpe_curve, rmvpe_grid)?;
        let fcpe = summarize_f0(note.range, center_pitch_hz, fcpe_curve, fcpe_grid)?;
        let mut alternatives = rmvpe
            .center_hz
            .zip(rmvpe.cents_difference)
            .filter(|(_, cents)| cents.abs() > 50.0)
            .map(|(center_hz, cents_from_target)| {
                vec![PitchAlternative {
                    source_expert: "rmvpe".to_string(),
                    center_hz,
                    cents_from_target,
                    confidence: rmvpe.confidence,
                }]
            })
            .unwrap_or_default();
        let fcpe_relation = rmvpe
            .center_hz
            .zip(fcpe.center_hz)
            .map(|(primary, secondary)| 1_200.0 * (secondary / primary).log2());
        if let Some((center_hz, cents_from_target)) = fcpe
            .center_hz
            .zip(fcpe.cents_difference)
            .filter(|_| fcpe_relation.is_some_and(|cents| cents.abs() > 50.0))
        {
            alternatives.push(PitchAlternative {
                source_expert: "fcpe".to_string(),
                center_hz,
                cents_from_target,
                confidence: None,
            });
        }
        candidates.push(SegmentCandidate {
            id: format!("game-note-{index}"),
            range: note.range,
            target_midi,
            game_midi: note.midi,
            game_boundary_decision_threshold: note.boundary_decision_threshold,
            game_presence_decision_threshold: note.presence_decision_threshold,
            center_pitch_hz,
            rmvpe_center_hz: rmvpe.center_hz,
            rmvpe_confidence: rmvpe.confidence,
            rmvpe_cents_difference: rmvpe.cents_difference,
            rmvpe_voiced_ratio: rmvpe.voiced_ratio,
            rmvpe_pitch_mad_cents: rmvpe.pitch_mad_cents,
            fcpe_center_hz: fcpe.center_hz,
            fcpe_observed_ratio: fcpe.voiced_ratio,
            fcpe_pitch_mad_cents: fcpe.pitch_mad_cents,
            fcpe_cents_from_rmvpe: fcpe_relation,
            fcpe_supports_rmvpe: fcpe_relation.map(|cents| cents.abs() <= 50.0),
            acoustic: Some(summarize_acoustic(note.range, acoustic, basic_pitch)?),
            techniques: TechniqueScores::default(),
            word_id: linked_word(note.range, words),
            alternatives,
        });
    }
    Ok(SingingFusionEvidence {
        schema_version: 1,
        candidates,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::artifact::{
        ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
        BasicPitchEvidenceV3, BasicPitchFrameV3, GameNoteEvidenceV1,
    };
    use crate::fingerprint::{ACOUSTIC_DSP_VERSION, FUSION_VERSION};

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

    #[test]
    fn fractional_game_midi_is_retained_at_explicit_target_decision() {
        assert_eq!(FUSION_VERSION, "fusion-v4");
        let fused =
            fuse_singing_evidence(&[], &game(69.25), &[], None, &[], None, &acoustic(), None)
                .unwrap();
        assert_eq!(fused.candidates[0].target_midi, 69);
        assert_eq!(fused.candidates[0].game_midi, 69.25);
        assert!(fused.candidates[0].rmvpe_center_hz.is_none());
    }

    #[test]
    fn octave_f0_disagreement_remains_continuous_review_evidence() {
        let fused = fuse_singing_evidence(
            &[],
            &game(69.0),
            &[F0Point {
                time: 110_000,
                hz: 880.0,
                confidence: Some(0.9),
            }],
            None,
            &[],
            None,
            &acoustic(),
            None,
        )
        .unwrap();
        let alternative = &fused.candidates[0].alternatives[0];
        assert_eq!(alternative.center_hz, 880.0);
        assert!((alternative.cents_from_target - 1_200.0).abs() < 0.01);
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
            &game(69.0),
            &[point(440.0)],
            None,
            &[point(441.0)],
            None,
            &acoustic(),
            None,
        )
        .unwrap();
        assert_eq!(agreed.candidates[0].fcpe_supports_rmvpe, Some(true));
        assert_eq!(agreed.candidates[0].center_pitch_hz, 440.0);
        assert!(agreed.candidates[0].alternatives.is_empty());

        let disagreed = fuse_singing_evidence(
            &[],
            &game(69.0),
            &[point(440.0)],
            None,
            &[point(880.0)],
            None,
            &acoustic(),
            None,
        )
        .unwrap();
        assert_eq!(disagreed.candidates[0].fcpe_supports_rmvpe, Some(false));
        assert_eq!(
            disagreed.candidates[0].alternatives[0].source_expert,
            "fcpe"
        );
        assert_eq!(disagreed.candidates[0].alternatives[0].confidence, None);

        let secondary_only = fuse_singing_evidence(
            &[],
            &game(69.0),
            &[],
            None,
            &[point(440.0)],
            None,
            &acoustic(),
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
            &game(69.25),
            &[],
            None,
            &[],
            None,
            &acoustic(),
            Some(&evidence),
        )
        .unwrap();
        let candidate = &fused.candidates[0];
        assert_eq!(candidate.target_midi, 69);
        assert_eq!(
            candidate
                .acoustic
                .as_ref()
                .unwrap()
                .basic_pitch_onset_activation,
            Some(0.8)
        );
        assert_eq!(
            candidate
                .acoustic
                .as_ref()
                .unwrap()
                .basic_pitch_onset_supported,
            Some(true)
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
    fn missing_game_fails_closed() {
        let mut evidence = game(69.0);
        evidence.notes.clear();
        assert!(
            fuse_singing_evidence(&[], &evidence, &[], None, &[], None, &acoustic(), None,)
                .is_err()
        );
    }
}
