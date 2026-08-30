use serde::{Deserialize, Serialize};

use crate::artifact::{
    AcousticEvidenceV1, BasicPitchEvidenceV3, GameEvidenceV1, TechniqueEvidenceV1,
};

use super::candidate_states::{
    MAX_EXPANDED_CANDIDATES, expand_pitch_alternative_states, f0_consolidation_challengers,
    persistent_f0_shifts, trustworthy_f0_point, validate_candidate_context_relations,
    validate_candidate_evidence_relation_count,
};
use super::{
    AcousticCandidateFeatures, BasicPitchCandidateFeatures, BoundaryAlternative,
    BoundaryCandidateRole, BoundaryEvidenceKind, CanonicalWordBoundary, F0Point, HardBoundarySetV1,
    PitchAlternative, SegmentCandidate, TechniqueCandidateFeatures, TechniqueScores, TimeRange,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SingingFusionEvidence {
    pub schema_version: u32,
    pub candidates: Vec<SegmentCandidate>,
    #[serde(default)]
    pub hard_boundaries: HardBoundarySetV1,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundarySegmentEvidence {
    pub range: TimeRange,
    pub fractional_midi: Option<f32>,
    pub boundary_decision_parameter: Option<f32>,
    pub presence_decision_parameter: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundaryEvidenceSet {
    pub source_expert: String,
    pub kind: BoundaryEvidenceKind,
    pub model_hash: Option<String>,
    pub runtime_identity: Option<String>,
    pub segments: Vec<BoundarySegmentEvidence>,
}

impl BoundaryEvidenceSet {
    pub fn from_game(game: &GameEvidenceV1) -> Result<Self, String> {
        if game.notes.is_empty() {
            return Err("GAME produced no note/boundary evidence".to_string());
        }
        Ok(Self {
            source_expert: game.model_id.clone(),
            kind: BoundaryEvidenceKind::Game,
            model_hash: Some(game.model_manifest_sha256.clone()),
            runtime_identity: Some(game.runtime_manifest_sha256.clone()),
            segments: game
                .notes
                .iter()
                .map(|note| BoundarySegmentEvidence {
                    range: note.range,
                    fractional_midi: Some(note.midi),
                    boundary_decision_parameter: Some(note.boundary_decision_threshold),
                    presence_decision_parameter: Some(note.presence_decision_threshold),
                })
                .collect(),
        })
    }
}

/// Converts one stable segment-level fractional proposal into an explicit MIDI
/// candidate; continuous F0 is never rounded frame-by-frame.
pub(crate) fn decide_fractional_target(midi: f32, source: &str) -> Result<u8, String> {
    if !midi.is_finite() || !(0.0..128.0).contains(&midi) {
        return Err(format!(
            "{source} fractional MIDI is outside the target-note domain"
        ));
    }
    let rounded = midi.round();
    if !(0.0..=127.0).contains(&rounded) {
        return Err(format!(
            "{source} fractional MIDI rounds outside MIDI 0..127"
        ));
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
    let first = f0_curve.partition_point(|point| point.time < range.start);
    let end = f0_curve.partition_point(|point| point.time < range.end);
    let observed = &f0_curve[first..end];
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
    let trustworthy = observed
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .collect::<Vec<_>>();
    let voiced_ratio = expected_frames
        .filter(|expected| *expected > 0)
        .map(|expected| trustworthy.len() as f32 / expected as f32);
    if trustworthy.is_empty() {
        return Ok(ContinuousF0Summary {
            center_hz: None,
            confidence: None,
            cents_difference: None,
            voiced_ratio,
            pitch_mad_cents: None,
        });
    }

    let all_confident = trustworthy.iter().all(|point| point.confidence.is_some());
    let confidence_total = trustworthy
        .iter()
        .filter_map(|point| point.confidence)
        .sum::<f32>();
    let use_confidence_weights = all_confident && confidence_total > f32::EPSILON;
    let cents_and_weights = trustworthy
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
    let confidence = all_confident.then(|| confidence_total / trustworthy.len() as f32);
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
) -> Result<AcousticCandidateFeatures, String> {
    let window_duration = u64::from(evidence.window_samples)
        .checked_mul(1_000_000)
        .map(|units| units / u64::from(evidence.sample_rate))
        .ok_or_else(|| "acoustic DSP window duration overflows".to_string())?;
    let first = evidence
        .frames
        .partition_point(|frame| frame.start.saturating_add(window_duration) <= range.start);
    let end = evidence
        .frames
        .partition_point(|frame| frame.start < range.end);
    let frames = &evidence.frames[first..end];
    if frames.is_empty() {
        return Err("boundary segment has no overlapping acoustic DSP frame".to_string());
    }
    let frame_count = frames.len();
    let mean_rms = frames.iter().map(|frame| frame.rms).sum::<f32>() / frame_count as f32;
    let mean_periodicity =
        frames.iter().map(|frame| frame.periodicity).sum::<f32>() / frame_count as f32;
    let mut fundamentals = frames
        .iter()
        .filter_map(|frame| frame.fundamental_hz)
        .filter(|hz| hz.is_finite() && *hz > 0.0)
        .collect::<Vec<_>>();
    fundamentals.sort_by(f32::total_cmp);
    let fundamental_center_hz = if fundamentals.is_empty() {
        None
    } else {
        Some(fundamentals[fundamentals.len() / 2])
    };
    let mean_snr_db = frames.iter().map(|frame| frame.snr_db).sum::<f32>() / frame_count as f32;
    let mean_vibrato_activation = frames
        .iter()
        .map(|frame| frame.vibrato_activation)
        .sum::<f32>()
        / frame_count as f32;
    let mean_glide_activation = frames
        .iter()
        .map(|frame| frame.glide_activation)
        .sum::<f32>()
        / frame_count as f32;
    let mean_ornament_activation = frames
        .iter()
        .map(|frame| frame.ornament_activation)
        .sum::<f32>()
        / frame_count as f32;
    let mean_breath_activation = frames
        .iter()
        .map(|frame| frame.breath_activation)
        .sum::<f32>()
        / frame_count as f32;
    let max_voicing_transition_activation = frames
        .iter()
        .map(|frame| frame.voicing_transition_activation)
        .max_by(f32::total_cmp)
        .unwrap_or(0.0);
    let after = evidence
        .frames
        .partition_point(|frame| frame.start < range.start);
    let onset_index = [
        after.checked_sub(1),
        (after < evidence.frames.len()).then_some(after),
    ]
    .into_iter()
    .flatten()
    .min_by_key(|index| evidence.frames[*index].start.abs_diff(range.start))
    .expect("validated acoustic evidence is non-empty");
    let onset_flux = evidence.frames[onset_index].spectral_flux;
    let preceding_flux = onset_index
        .checked_sub(1)
        .and_then(|index| evidence.frames[index].spectral_flux);
    // A measured DSP onset supports a transition when flux is at least twice
    // the preceding frame and above the finite floor. This is not probability.
    let onset_supported = onset_flux
        .zip(preceding_flux)
        .map(|(onset, preceding)| onset >= (preceding * 2.0).max(1.0e-6));
    Ok(AcousticCandidateFeatures {
        frame_count,
        mean_rms,
        mean_periodicity,
        fundamental_center_hz,
        mean_snr_db,
        mean_vibrato_activation,
        mean_glide_activation,
        mean_ornament_activation,
        mean_breath_activation,
        max_voicing_transition_activation,
        onset_flux,
        preceding_flux,
        onset_supported,
    })
}

fn validate_basic_pitch_evidence(evidence: &BasicPitchEvidenceV3) -> Result<(), String> {
    if evidence.frames.is_empty()
        || evidence
            .frames
            .windows(2)
            .any(|pair| pair[0].time >= pair[1].time)
        || evidence.frames.iter().any(|frame| {
            frame.contour_class >= 264
                || [
                    frame.note_activation,
                    frame.onset_activation,
                    frame.contour_activation,
                ]
                .into_iter()
                .any(|value| !value.is_finite() || !(0.0..=1.0).contains(&value))
        })
    {
        return Err("Basic Pitch evidence is invalid".to_string());
    }
    Ok(())
}

fn summarize_basic_pitch(
    range: TimeRange,
    evidence: &BasicPitchEvidenceV3,
) -> Result<BasicPitchCandidateFeatures, String> {
    const ONSET_WINDOW: u64 = 60_000;
    let onset_window_start = range.start.saturating_sub(ONSET_WINDOW);
    let onset_window_end = range.start.saturating_add(ONSET_WINDOW);
    let onset_first = evidence
        .frames
        .partition_point(|frame| frame.time < onset_window_start);
    let onset_end = evidence
        .frames
        .partition_point(|frame| frame.time < onset_window_end);
    let onset_activation = evidence.frames[onset_first..onset_end]
        .iter()
        .map(|frame| frame.onset_activation)
        .max_by(f32::total_cmp)
        .unwrap_or(0.0);
    let local_first = evidence
        .frames
        .partition_point(|frame| frame.time < range.start);
    let local_end = evidence
        .frames
        .partition_point(|frame| frame.time < range.end);
    let local_frames = &evidence.frames[local_first..local_end];
    let note_activation = if local_frames.is_empty() {
        0.0
    } else {
        local_frames
            .iter()
            .map(|frame| frame.note_activation)
            .sum::<f32>()
            / local_frames.len() as f32
    };
    let (contour_class, contour_activation) = local_frames
        .iter()
        .max_by(|left, right| left.contour_activation.total_cmp(&right.contour_activation))
        .map(|frame| (frame.contour_class, frame.contour_activation))
        .unwrap_or((0, 0.0));
    Ok(BasicPitchCandidateFeatures {
        onset_activation,
        note_activation,
        contour_activation,
        contour_class,
        onset_supported: onset_activation >= 0.5,
    })
}

fn basic_pitch_onset_challengers(
    boundaries: &BoundaryEvidenceSet,
    evidence: &BasicPitchEvidenceV3,
) -> Result<Vec<BoundaryAlternative>, String> {
    validate_basic_pitch_evidence(evidence)?;
    const ONSET_THRESHOLD: f32 = 0.5;
    const EDGE_MARGIN: u64 = 60_000;
    const MIN_ONSET_DISTANCE: u64 = 100_000;

    let mut alternatives = Vec::new();
    for segment in &boundaries.segments {
        if segment.range.end.saturating_sub(segment.range.start) <= EDGE_MARGIN * 2 {
            continue;
        }
        let lower = segment.range.start.saturating_add(EDGE_MARGIN);
        let upper = segment.range.end.saturating_sub(EDGE_MARGIN);
        let first = evidence.frames.partition_point(|frame| frame.time < lower);
        let end = evidence.frames.partition_point(|frame| frame.time <= upper);
        let mut peaks: Vec<(u64, f32)> = Vec::new();
        for frame in &evidence.frames[first..end] {
            if frame.onset_activation < ONSET_THRESHOLD {
                continue;
            }
            if let Some(last) = peaks.last_mut()
                && frame.time.saturating_sub(last.0) < MIN_ONSET_DISTANCE
            {
                if frame.onset_activation > last.1 {
                    *last = (frame.time, frame.onset_activation);
                }
            } else {
                peaks.push((frame.time, frame.onset_activation));
            }
        }
        if peaks.is_empty() {
            continue;
        }
        let peak_scores = peaks
            .iter()
            .copied()
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut cuts = Vec::with_capacity(peaks.len() + 2);
        cuts.push(segment.range.start);
        cuts.extend(peaks.iter().map(|(time, _)| *time));
        cuts.push(segment.range.end);
        alternatives.extend(cuts.windows(2).filter_map(|pair| {
            TimeRange::new(pair[0], pair[1])
                .ok()
                .map(|range| BoundaryAlternative {
                    source_expert: "basic_pitch.onset".to_string(),
                    range,
                    kind: BoundaryEvidenceKind::BasicPitchOnset,
                    fractional_midi: None,
                    source_local_score: peak_scores.get(&pair[0]).copied(),
                    source_local_pitch_score: None,
                    calibrated_boundary_confidence: None,
                    calibrated_pitch_confidence: None,
                    hard: false,
                })
        }));
    }
    Ok(alternatives)
}

const MIN_CONTEXT_SEGMENT: u64 = 40_000;
const CONTEXT_EDGE_MARGIN: u64 = 30_000;
const MAX_CONTEXT_CUTS_PER_PRIMARY: usize = 4_096;

#[derive(Debug, Clone)]
struct ContextCut {
    time: u64,
    score: Option<f32>,
    hard: bool,
}

fn partition_context_challengers(
    boundaries: &BoundaryEvidenceSet,
    source_expert: &str,
    kind: BoundaryEvidenceKind,
    cuts: &[ContextCut],
) -> Result<Vec<BoundaryAlternative>, String> {
    if source_expert.trim().is_empty() {
        return Err("contextual boundary source identity is empty".to_string());
    }
    if cuts.iter().any(|cut| {
        cut.score
            .is_some_and(|score| !score.is_finite() || !(0.0..=1.0).contains(&score))
    }) {
        return Err(format!(
            "{source_expert} contextual boundary evidence contains an invalid source-local score"
        ));
    }

    let mut sorted_cuts = cuts.to_vec();
    sorted_cuts.sort_by_key(|cut| cut.time);
    let mut alternatives = Vec::new();
    for primary in &boundaries.segments {
        let lower = primary.range.start.saturating_add(CONTEXT_EDGE_MARGIN);
        let upper = primary.range.end.saturating_sub(CONTEXT_EDGE_MARGIN);
        if lower >= upper {
            continue;
        }
        let first = sorted_cuts.partition_point(|cut| cut.time <= lower);
        let end = sorted_cuts.partition_point(|cut| cut.time < upper);
        let local = sorted_cuts[first..end].to_vec();
        let mut merged = Vec::<ContextCut>::new();
        for cut in local {
            if let Some(previous) = merged.last_mut()
                && cut.time.saturating_sub(previous.time) < MIN_CONTEXT_SEGMENT
            {
                if cut.hard && !previous.hard {
                    *previous = cut;
                } else {
                    previous.hard |= cut.hard;
                    previous.score = match (previous.score, cut.score) {
                        (Some(left), Some(right)) => Some(left.max(right)),
                        (left, right) => left.or(right),
                    };
                }
            } else {
                merged.push(cut);
            }
        }
        if merged.len() > MAX_CONTEXT_CUTS_PER_PRIMARY {
            return Err(format!(
                "{source_expert} produced too many contextual boundaries in one primary region"
            ));
        }
        if merged.is_empty() {
            continue;
        }
        let mut points = Vec::with_capacity(merged.len() + 2);
        points.push(ContextCut {
            time: primary.range.start,
            score: None,
            hard: false,
        });
        points.extend(merged);
        points.push(ContextCut {
            time: primary.range.end,
            score: None,
            hard: false,
        });
        for pair in points.windows(2) {
            let duration = pair[1].time.saturating_sub(pair[0].time);
            if duration < MIN_CONTEXT_SEGMENT {
                continue;
            }
            alternatives.push(BoundaryAlternative {
                source_expert: source_expert.to_string(),
                range: TimeRange::new(pair[0].time, pair[1].time)?,
                kind,
                fractional_midi: None,
                source_local_score: pair[0].score,
                source_local_pitch_score: None,
                calibrated_boundary_confidence: None,
                calibrated_pitch_confidence: None,
                hard: false,
            });
        }
    }
    Ok(alternatives)
}

fn alignment_boundary_challengers(
    boundaries: &BoundaryEvidenceSet,
    words: &[CanonicalWordBoundary],
) -> Result<Vec<BoundaryAlternative>, String> {
    let cuts = words
        .iter()
        .flat_map(|word| {
            [
                ContextCut {
                    time: word.range.start,
                    score: word.confidence,
                    hard: false,
                },
                ContextCut {
                    time: word.range.end,
                    score: word.confidence,
                    hard: false,
                },
            ]
        })
        .collect::<Vec<_>>();
    partition_context_challengers(
        boundaries,
        "alignment.words",
        BoundaryEvidenceKind::Alignment,
        &cuts,
    )
}

/// Shares the F0-derived primary detector so one-frame noise or a glide cannot
/// re-enter the Candidate Pool under a different evidence label.
fn f0_transition_challengers(
    boundaries: &BoundaryEvidenceSet,
    source_expert: &str,
    curve: &[F0Point],
) -> Result<Vec<BoundaryAlternative>, String> {
    if boundaries.kind == BoundaryEvidenceKind::F0Derived || curve.len() < 2 {
        return Ok(Vec::new());
    }
    let cuts = persistent_f0_shifts(curve)
        .into_iter()
        .map(|(time, score)| ContextCut {
            time,
            score: Some(score),
            hard: false,
        })
        .collect::<Vec<_>>();
    partition_context_challengers(
        boundaries,
        &format!("{source_expert}.transition"),
        BoundaryEvidenceKind::F0Transition,
        &cuts,
    )
}

fn acoustic_onset_challengers(
    boundaries: &BoundaryEvidenceSet,
    evidence: &AcousticEvidenceV1,
) -> Result<Vec<BoundaryAlternative>, String> {
    evidence.validate().map_err(|error| error.message)?;
    let mut cuts = Vec::new();
    for pair in evidence.frames.windows(2) {
        let Some(previous) = pair[0].spectral_flux else {
            continue;
        };
        let Some(onset) = pair[1].spectral_flux else {
            continue;
        };
        let threshold = (previous * 2.0).max(1.0e-6);
        if onset < threshold {
            continue;
        }
        let score = (onset / (onset + previous.abs() + 1.0e-6)).clamp(0.0, 1.0);
        cuts.push(ContextCut {
            time: pair[1].start,
            score: Some(score),
            hard: false,
        });
    }
    partition_context_challengers(
        boundaries,
        "acoustic.onset",
        BoundaryEvidenceKind::AcousticOnset,
        &cuts,
    )
}

fn constraint_partition_challengers(
    boundaries: &BoundaryEvidenceSet,
    events: &[BoundaryAlternative],
) -> Result<Vec<BoundaryAlternative>, String> {
    if events.is_empty() {
        return Ok(Vec::new());
    }
    let mut sources = events
        .iter()
        .map(|event| event.source_expert.as_str())
        .collect::<Vec<_>>();
    sources.sort_unstable();
    sources.dedup();
    let source_expert = format!("constraints:{}", sources.join("+"));
    let mut alternatives = Vec::new();

    // Caller-hard cuts are structural authority, not denoisable context. They
    // bypass edge margins, minimum-duration filtering and nearby-cut merging.
    let hard_cuts = events
        .iter()
        .filter(|event| event.hard)
        .flat_map(|event| [event.range.start, event.range.end])
        .collect::<std::collections::BTreeSet<_>>();
    for primary in &boundaries.segments {
        let local = hard_cuts
            .range((
                std::ops::Bound::Excluded(primary.range.start),
                std::ops::Bound::Excluded(primary.range.end),
            ))
            .copied()
            .collect::<Vec<_>>();
        if local.len() > MAX_CONTEXT_CUTS_PER_PRIMARY {
            return Err(format!(
                "{source_expert} produced too many hard boundaries in one primary region"
            ));
        }
        if local.is_empty() {
            continue;
        }
        let mut points = Vec::with_capacity(local.len() + 2);
        points.push(primary.range.start);
        points.extend(local);
        points.push(primary.range.end);
        alternatives.extend(points.windows(2).map(|pair| BoundaryAlternative {
            source_expert: source_expert.clone(),
            range: TimeRange::new(pair[0], pair[1]).expect("strict hard-cut ordering"),
            kind: BoundaryEvidenceKind::Constraint,
            fractional_midi: None,
            source_local_score: None,
            source_local_pitch_score: None,
            calibrated_boundary_confidence: None,
            calibrated_pitch_confidence: None,
            hard: false,
        }));
    }

    for kind in [
        BoundaryEvidenceKind::PhraseConstraint,
        BoundaryEvidenceKind::Constraint,
    ] {
        let soft_cuts = events
            .iter()
            .filter(|event| !event.hard && event.kind == kind)
            .flat_map(|event| {
                [
                    ContextCut {
                        time: event.range.start,
                        score: event.source_local_score,
                        hard: false,
                    },
                    ContextCut {
                        time: event.range.end,
                        score: event.source_local_score,
                        hard: false,
                    },
                ]
            })
            .collect::<Vec<_>>();
        alternatives.extend(partition_context_challengers(
            boundaries,
            &source_expert,
            kind,
            &soft_cuts,
        )?);
    }
    Ok(alternatives)
}

struct TechniqueEvidenceIndex<'a> {
    artifact: &'a TechniqueEvidenceV1,
    vibrato: Option<usize>,
    glissando: Option<usize>,
    falsetto: Option<usize>,
}

fn index_technique_evidence(
    evidence: &[TechniqueEvidenceV1],
) -> Result<Vec<TechniqueEvidenceIndex<'_>>, String> {
    evidence
        .iter()
        .map(|artifact| {
            if artifact.model_id.trim().is_empty() || artifact.calibration.trim().is_empty() {
                return Err("technique evidence identity is invalid".to_string());
            }
            if artifact.intervals.iter().any(|interval| {
                interval.source_local_scores.len() != artifact.taxonomy.len()
                    || interval
                        .source_local_scores
                        .iter()
                        .any(|value| !value.is_finite() || !(0.0..=1.0).contains(value))
            }) {
                return Err(format!(
                    "{} technique evidence does not match its taxonomy or contains an invalid source-local activation",
                    artifact.model_id
                ));
            }
            Ok(TechniqueEvidenceIndex {
                artifact,
                vibrato: artifact.taxonomy.iter().position(|item| item == "vibrato"),
                glissando: artifact.taxonomy.iter().position(|item| item == "glissando"),
                falsetto: artifact.taxonomy.iter().position(|item| item == "falsetto"),
            })
        })
        .collect()
}

fn summarize_technique_evidence(
    range: TimeRange,
    evidence: &[TechniqueEvidenceIndex<'_>],
) -> Vec<TechniqueCandidateFeatures> {
    evidence
        .iter()
        .filter_map(|indexed| {
            let mut activations = [None::<f32>; 3];
            for interval in indexed
                .artifact
                .intervals
                .iter()
                .filter(|interval| interval.range.overlaps(range))
            {
                for (target, taxonomy_index) in activations.iter_mut().zip([
                    indexed.vibrato,
                    indexed.glissando,
                    indexed.falsetto,
                ]) {
                    let Some(taxonomy_index) = taxonomy_index else {
                        continue;
                    };
                    let value = interval.source_local_scores[taxonomy_index];
                    *target = Some(target.map_or(value, |current| current.max(value)));
                }
            }
            activations
                .iter()
                .any(Option::is_some)
                .then(|| TechniqueCandidateFeatures {
                    source_expert: indexed.artifact.model_id.clone(),
                    calibration: indexed.artifact.calibration.clone(),
                    vibrato_activation: activations[0],
                    glissando_activation: activations[1],
                    falsetto_activation: activations[2],
                })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_segment_candidate(
    words: &[CanonicalWordBoundary],
    source_expert: &str,
    boundary_kind: BoundaryEvidenceKind,
    boundary_role: BoundaryCandidateRole,
    index: usize,
    segment: &BoundarySegmentEvidence,
    primary_pitch_owner: &str,
    rmvpe_curve: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe_curve: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
    acoustic: Option<&AcousticEvidenceV1>,
    acoustic_onset_enabled: bool,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
    technique_evidence: &[TechniqueEvidenceIndex<'_>],
    all_boundary_evidence: &[BoundaryAlternative],
) -> Result<SegmentCandidate, String> {
    let (target_midi, center_pitch_hz, target_pitch_source) =
        if let Some(fractional_midi) = segment.fractional_midi {
            (
                decide_fractional_target(fractional_midi, source_expert)?,
                midi_hz(fractional_midi),
                source_expert.to_string(),
            )
        } else {
            let center_hz = primary_f0_center(
                segment.range,
                primary_pitch_owner,
                rmvpe_curve,
                rmvpe_grid,
                fcpe_curve,
                fcpe_grid,
            )?;
            let fractional_midi = 69.0 + 12.0 * (center_hz / 440.0).log2();
            (
                decide_fractional_target(fractional_midi, primary_pitch_owner)?,
                center_hz,
                primary_pitch_owner.to_string(),
            )
        };
    let rmvpe = summarize_f0(segment.range, center_pitch_hz, rmvpe_curve, rmvpe_grid)?;
    let fcpe = summarize_f0(segment.range, center_pitch_hz, fcpe_curve, fcpe_grid)?;
    let fcpe_relation = rmvpe
        .center_hz
        .zip(fcpe.center_hz)
        .map(|(primary, secondary)| 1_200.0 * (secondary / primary).log2());
    let mut alternatives = Vec::new();
    for (expert, summary) in [("rmvpe", &rmvpe), ("fcpe", &fcpe)] {
        if expert == target_pitch_source {
            continue;
        }
        if let Some((center_hz, cents_from_target)) =
            summary.center_hz.zip(summary.cents_difference)
        {
            alternatives.push(PitchAlternative {
                source_expert: expert.to_string(),
                center_hz,
                cents_from_target,
                confidence: summary.confidence,
            });
        }
    }
    for boundary in all_boundary_evidence.iter().filter(|boundary| {
        boundary.source_expert != target_pitch_source
            && boundary.range.overlaps(segment.range)
            && boundary.fractional_midi.is_some()
    }) {
        let center_hz = midi_hz(boundary.fractional_midi.expect("filtered above"));
        if alternatives.iter().any(|alternative| {
            alternative.source_expert == boundary.source_expert
                && (alternative.center_hz - center_hz).abs() < f32::EPSILON
        }) {
            continue;
        }
        alternatives.push(PitchAlternative {
            source_expert: boundary.source_expert.clone(),
            center_hz,
            cents_from_target: 1_200.0 * (center_hz / center_pitch_hz).log2(),
            confidence: boundary.calibrated_pitch_confidence,
        });
    }
    let selected_boundary_evidence = all_boundary_evidence.iter().find(|alternative| {
        alternative.source_expert == source_expert
            && alternative.range == segment.range
            && alternative.kind == boundary_kind
            && alternative.fractional_midi == segment.fractional_midi
    });

    Ok(SegmentCandidate {
        id: format!("{source_expert}-segment-{index}"),
        range: segment.range,
        target_midi,
        boundary_source: source_expert.to_string(),
        boundary_kind,
        boundary_role,
        boundary_fractional_midi: segment.fractional_midi,
        boundary_decision_parameter: segment.boundary_decision_parameter,
        presence_decision_parameter: segment.presence_decision_parameter,
        boundary_hard: selected_boundary_evidence.is_some_and(|alternative| alternative.hard),
        boundary_support: selected_boundary_evidence
            .and_then(|alternative| alternative.source_local_score),
        boundary_calibrated_confidence: selected_boundary_evidence
            .and_then(|alternative| alternative.calibrated_boundary_confidence),
        target_pitch_source_local_score: selected_boundary_evidence
            .filter(|_| target_pitch_source == source_expert)
            .and_then(|alternative| alternative.source_local_pitch_score),
        target_pitch_calibrated_confidence: selected_boundary_evidence
            .filter(|_| target_pitch_source == source_expert)
            .and_then(|alternative| alternative.calibrated_pitch_confidence),
        target_pitch_source,
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
        acoustic: acoustic
            .map(|evidence| {
                let mut features = summarize_acoustic(segment.range, evidence)?;
                if !acoustic_onset_enabled {
                    features.onset_supported = None;
                }
                Ok::<_, String>(features)
            })
            .transpose()?,
        basic_pitch: basic_pitch
            .map(|evidence| summarize_basic_pitch(segment.range, evidence))
            .transpose()?,
        boundary_alternatives: all_boundary_evidence
            .iter()
            .filter(|alternative| {
                alternative.source_expert != source_expert
                    && alternative.range.overlaps(segment.range)
            })
            .cloned()
            .collect(),
        boundary_constraints: Vec::new(),
        technique_evidence: summarize_technique_evidence(segment.range, technique_evidence),
        techniques: TechniqueScores::default(),
        word_id: linked_word(segment.range, words),
        alternatives,
    })
}

fn primary_f0_center(
    range: TimeRange,
    owner: &str,
    rmvpe_curve: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe_curve: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
) -> Result<f32, String> {
    let summary = match owner {
        "rmvpe" => summarize_f0(range, 440.0, rmvpe_curve, rmvpe_grid)?,
        "fcpe" => summarize_f0(range, 440.0, fcpe_curve, fcpe_grid)?,
        other => return Err(format!("unsupported primary F0 expert: {other}")),
    };
    summary
        .center_hz
        .ok_or_else(|| format!("{owner} produced no voiced F0 in a derived boundary segment"))
}

/// Builds segment-level candidates from a typed boundary source without optional
/// boundary challengers. This is the stable baseline entry point.
#[allow(clippy::too_many_arguments)]
pub fn fuse_singing_evidence(
    words: &[CanonicalWordBoundary],
    boundaries: &BoundaryEvidenceSet,
    primary_pitch_owner: &str,
    rmvpe_curve: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe_curve: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
    acoustic: Option<&AcousticEvidenceV1>,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
) -> Result<SingingFusionEvidence, String> {
    fuse_singing_evidence_with_challengers(
        words,
        boundaries,
        primary_pitch_owner,
        rmvpe_curve,
        rmvpe_grid,
        fcpe_curve,
        fcpe_grid,
        acoustic,
        true,
        basic_pitch,
        &[],
        &[],
    )
}

/// Builds candidates from typed duration boundaries and optional fractional pitch.
/// F0 fallback pitch remains a separately identified selected-expert proposal.
#[allow(clippy::too_many_arguments)]
pub(crate) fn fuse_singing_evidence_with_challengers(
    words: &[CanonicalWordBoundary],
    boundaries: &BoundaryEvidenceSet,
    primary_pitch_owner: &str,
    rmvpe_curve: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe_curve: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
    acoustic: Option<&AcousticEvidenceV1>,
    acoustic_onset_enabled: bool,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
    boundary_challengers: &[BoundaryAlternative],
    technique_evidence: &[TechniqueEvidenceV1],
) -> Result<SingingFusionEvidence, String> {
    if boundaries.source_expert.trim().is_empty() || boundaries.segments.is_empty() {
        return Err("note-length evidence is required for singing fusion".to_string());
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
    if let Some(evidence) = acoustic {
        evidence.validate().map_err(|error| error.message)?;
    }
    if let Some(evidence) = basic_pitch {
        validate_basic_pitch_evidence(evidence)?;
    }
    let constraint_events = boundary_challengers
        .iter()
        .filter(|alternative| {
            matches!(
                alternative.kind,
                BoundaryEvidenceKind::Constraint | BoundaryEvidenceKind::PhraseConstraint
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut generated_challengers = boundary_challengers
        .iter()
        .filter(|alternative| {
            !matches!(
                alternative.kind,
                BoundaryEvidenceKind::Constraint | BoundaryEvidenceKind::PhraseConstraint
            )
        })
        .cloned()
        .collect::<Vec<_>>();
    generated_challengers.extend(alignment_boundary_challengers(boundaries, words)?);
    let selected_f0 = match primary_pitch_owner {
        "rmvpe" => rmvpe_curve,
        "fcpe" => fcpe_curve,
        other => return Err(format!("unsupported primary F0 expert: {other}")),
    };
    generated_challengers.extend(f0_transition_challengers(
        boundaries,
        primary_pitch_owner,
        selected_f0,
    )?);
    if acoustic_onset_enabled && let Some(evidence) = acoustic {
        generated_challengers.extend(acoustic_onset_challengers(boundaries, evidence)?);
    }
    if let Some(evidence) = basic_pitch {
        generated_challengers.extend(basic_pitch_onset_challengers(boundaries, evidence)?);
    }
    generated_challengers.extend(constraint_partition_challengers(
        boundaries,
        &constraint_events,
    )?);
    let acoustic_for_onset = acoustic_onset_enabled.then_some(acoustic).flatten();
    generated_challengers.extend(f0_consolidation_challengers(
        boundaries,
        words,
        primary_pitch_owner,
        selected_f0,
        acoustic_for_onset,
        basic_pitch,
        boundary_challengers,
    )?);
    if generated_challengers.len() > 100_000 {
        return Err("contextual candidate graph exceeds the bounded candidate limit".to_string());
    }

    let mut deduplicated = std::collections::BTreeMap::<
        (String, u64, u64, BoundaryEvidenceKind, Option<u32>),
        BoundaryAlternative,
    >::new();
    for alternative in generated_challengers {
        let key = (
            alternative.source_expert.clone(),
            alternative.range.start,
            alternative.range.end,
            alternative.kind,
            alternative.fractional_midi.map(f32::to_bits),
        );
        deduplicated
            .entry(key)
            .and_modify(|existing| {
                existing.hard |= alternative.hard;
                existing.source_local_score =
                    [existing.source_local_score, alternative.source_local_score]
                        .into_iter()
                        .flatten()
                        .max_by(f32::total_cmp);
                existing.source_local_pitch_score = [
                    existing.source_local_pitch_score,
                    alternative.source_local_pitch_score,
                ]
                .into_iter()
                .flatten()
                .max_by(f32::total_cmp);
                existing.calibrated_boundary_confidence = [
                    existing.calibrated_boundary_confidence,
                    alternative.calibrated_boundary_confidence,
                ]
                .into_iter()
                .flatten()
                .max_by(f32::total_cmp);
                existing.calibrated_pitch_confidence = [
                    existing.calibrated_pitch_confidence,
                    alternative.calibrated_pitch_confidence,
                ]
                .into_iter()
                .flatten()
                .max_by(f32::total_cmp);
            })
            .or_insert(alternative);
    }
    let effective_boundary_challengers = deduplicated.into_values().collect::<Vec<_>>();
    if boundaries
        .segments
        .len()
        .checked_add(effective_boundary_challengers.len())
        .is_none_or(|count| count > MAX_EXPANDED_CANDIDATES)
    {
        return Err("contextual candidate graph exceeds the bounded candidate limit".to_string());
    }

    let candidate_duration_count = boundaries
        .segments
        .len()
        .checked_add(effective_boundary_challengers.len())
        .ok_or_else(|| "candidate duration-state count overflows".to_string())?;
    let raw_boundary_evidence_count = boundaries
        .segments
        .len()
        .checked_add(constraint_events.len())
        .and_then(|count| count.checked_add(effective_boundary_challengers.len()))
        .ok_or_else(|| "raw boundary evidence count overflows".to_string())?;
    validate_candidate_evidence_relation_count(
        candidate_duration_count,
        raw_boundary_evidence_count,
    )?;
    let mut all_boundary_evidence = Vec::with_capacity(raw_boundary_evidence_count);
    let mut boundary_evidence_index = std::collections::BTreeMap::new();
    for alternative in boundaries
        .segments
        .iter()
        .map(|segment| BoundaryAlternative {
            source_expert: boundaries.source_expert.clone(),
            range: segment.range,
            kind: boundaries.kind,
            fractional_midi: segment.fractional_midi,
            source_local_score: None,
            source_local_pitch_score: None,
            calibrated_boundary_confidence: None,
            calibrated_pitch_confidence: None,
            hard: false,
        })
        .chain(
            constraint_events
                .iter()
                .chain(effective_boundary_challengers.iter())
                .cloned(),
        )
    {
        let key = (
            alternative.source_expert.clone(),
            alternative.range.start,
            alternative.range.end,
            alternative.kind,
            alternative.fractional_midi.map(f32::to_bits),
        );
        if let Some(index) = boundary_evidence_index.get(&key).copied() {
            let existing: &mut BoundaryAlternative = &mut all_boundary_evidence[index];
            existing.hard |= alternative.hard;
            existing.source_local_score =
                [existing.source_local_score, alternative.source_local_score]
                    .into_iter()
                    .flatten()
                    .max_by(f32::total_cmp);
            existing.source_local_pitch_score = [
                existing.source_local_pitch_score,
                alternative.source_local_pitch_score,
            ]
            .into_iter()
            .flatten()
            .max_by(f32::total_cmp);
            existing.calibrated_boundary_confidence = [
                existing.calibrated_boundary_confidence,
                alternative.calibrated_boundary_confidence,
            ]
            .into_iter()
            .flatten()
            .max_by(f32::total_cmp);
            existing.calibrated_pitch_confidence = [
                existing.calibrated_pitch_confidence,
                alternative.calibrated_pitch_confidence,
            ]
            .into_iter()
            .flatten()
            .max_by(f32::total_cmp);
        } else {
            boundary_evidence_index.insert(key, all_boundary_evidence.len());
            all_boundary_evidence.push(alternative);
        }
    }
    validate_candidate_context_relations(
        boundaries
            .segments
            .iter()
            .map(|segment| segment.range)
            .chain(
                effective_boundary_challengers
                    .iter()
                    .map(|alternative| alternative.range),
            ),
        primary_pitch_owner,
        rmvpe_curve,
        fcpe_curve,
        acoustic,
        basic_pitch,
    )?;
    let technique_interval_count =
        technique_evidence
            .iter()
            .try_fold(0usize, |count, artifact| {
                count
                    .checked_add(artifact.intervals.len())
                    .ok_or_else(|| "technique interval evidence count overflows".to_string())
            })?;
    let metadata_evidence_count = all_boundary_evidence
        .len()
        .checked_add(words.len())
        .and_then(|count| count.checked_add(technique_interval_count))
        .ok_or_else(|| "candidate metadata evidence count overflows".to_string())?;
    validate_candidate_evidence_relation_count(candidate_duration_count, metadata_evidence_count)?;
    let indexed_technique_evidence = index_technique_evidence(technique_evidence)?;
    let mut candidates = Vec::with_capacity(candidate_duration_count);
    for (index, segment) in boundaries.segments.iter().enumerate() {
        candidates.push(build_segment_candidate(
            words,
            &boundaries.source_expert,
            boundaries.kind,
            BoundaryCandidateRole::Primary,
            index,
            segment,
            primary_pitch_owner,
            rmvpe_curve,
            rmvpe_grid,
            fcpe_curve,
            fcpe_grid,
            acoustic,
            acoustic_onset_enabled,
            basic_pitch,
            &indexed_technique_evidence,
            &all_boundary_evidence,
        )?);
    }
    for (index, alternative) in effective_boundary_challengers.iter().enumerate() {
        let segment = BoundarySegmentEvidence {
            range: alternative.range,
            fractional_midi: alternative.fractional_midi,
            boundary_decision_parameter: None,
            presence_decision_parameter: None,
        };
        match build_segment_candidate(
            words,
            &alternative.source_expert,
            alternative.kind,
            BoundaryCandidateRole::Challenger,
            index,
            &segment,
            primary_pitch_owner,
            rmvpe_curve,
            rmvpe_grid,
            fcpe_curve,
            fcpe_grid,
            acoustic,
            acoustic_onset_enabled,
            basic_pitch,
            &indexed_technique_evidence,
            &all_boundary_evidence,
        ) {
            Ok(candidate) => candidates.push(candidate),
            Err(error)
                if alternative.fractional_midi.is_none()
                    && error.contains("produced no voiced F0") =>
            {
                // An optional unpitched challenger cannot become a usable note
                // state without local F0. It remains attached as disagreement
                // evidence to overlapping primary candidates.
            }
            Err(error) => return Err(error),
        }
    }
    Ok(SingingFusionEvidence {
        schema_version: 2,
        candidates: expand_pitch_alternative_states(candidates)?,
        hard_boundaries: HardBoundarySetV1::default(),
    })
}

#[cfg(test)]
#[path = "baseline_tests.rs"]
mod tests;
