use crate::artifact::{AcousticEvidenceV1, BasicPitchEvidenceV3};

use super::baseline::{BoundaryEvidenceSet, decide_fractional_target};
use super::{
    BoundaryAlternative, BoundaryEvidenceKind, CanonicalWordBoundary, F0Point, PitchAlternative,
    SegmentCandidate, TimeRange,
};

/// Existing bounded Candidate Pool policy, enforced before and after pitch
/// expansion. Serialized AI requests retain their independent 8 MiB byte cap.
pub(crate) const MAX_EXPANDED_CANDIDATES: usize = 100_000;
/// Every call site protected by this bound (`f0_consolidation_challengers`,
/// the two `build_candidate_states` checks, and `attach_boundary_constraints`)
/// resolves its per-item work through sorted/binary-search range queries, not
/// a naive nested scan, so the true cost is near O((a+b) log(a+b)), not
/// O(a*b); this product is a deliberately conservative defensive ceiling, not
/// a tight complexity bound. A real ~3.6-minute production song with dense
/// word/onset/pitch-discontinuity evidence measured ~24.9M
/// (candidates=3888, constraints=6401) at `attach_boundary_constraints`,
/// which the prior 10M ceiling rejected outright. Raised with real-song
/// headroom while remaining a finite, meaningful ceiling against genuinely
/// pathological/corrupted evidence.
pub(crate) const MAX_CANDIDATE_EVIDENCE_RELATIONS: usize = 500_000_000;
pub(crate) const MAX_CANDIDATE_CONTEXT_RELATIONS: usize = 10_000_000;
const MAX_PITCH_PROPOSALS_PER_SEGMENT: usize = 64;
const CONTEXT_DURATION: u64 = 80_000;
const MIN_CONTEXT_FRAMES: usize = 3;
const EXIT_HYSTERESIS_CENTS: f32 = 110.0;
const BOUNDARY_EVIDENCE_TOLERANCE: u64 = 60_000;
const MIN_TRANSITION_CONFIDENCE: f32 = 0.5;

pub(crate) fn trustworthy_f0_point(point: &F0Point) -> bool {
    point.hz.is_finite()
        && point.hz > 0.0
        && point.confidence.is_none_or(|confidence| {
            confidence.is_finite() && confidence >= MIN_TRANSITION_CONFIDENCE
        })
}

fn median(values: &mut [f32]) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f32::total_cmp);
    Some(values[values.len() / 2])
}

fn context_summary(curve: &[F0Point], start: u64, end: u64) -> Option<(f32, f32)> {
    let first = curve.partition_point(|point| point.time < start);
    let end = curve.partition_point(|point| point.time < end);
    let mut cents = curve[first..end]
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .map(|point| 1_200.0 * (point.hz / 440.0).log2())
        .collect::<Vec<_>>();
    if cents.len() < MIN_CONTEXT_FRAMES {
        return None;
    }
    let center = median(&mut cents)?;
    let mut deviations = cents
        .into_iter()
        .map(|value| (value - center).abs())
        .collect::<Vec<_>>();
    Some((center, median(&mut deviations)?))
}

fn maximum_voiced_gap(curve: &[F0Point]) -> u64 {
    let trustworthy = curve
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .collect::<Vec<_>>();
    let mut hops = trustworthy
        .windows(2)
        .filter_map(|pair| pair[1].time.checked_sub(pair[0].time))
        .filter(|hop| *hop > 0)
        .collect::<Vec<_>>();
    hops.sort_unstable();
    hops.get(hops.len() / 2)
        .copied()
        .unwrap_or(10_000)
        .saturating_mul(3)
        .clamp(10_000, 60_000)
}

/// One shared sustained-shift detector feeds F0-derived primary segmentation,
/// contextual discontinuity evidence, and consolidation eligibility.
pub(crate) fn persistent_f0_shifts(curve: &[F0Point]) -> Vec<(u64, f32)> {
    const CONTEXT_FRAMES: usize = 5;
    const ENTER_CENTS: f32 = 175.0;
    let maximum_voiced_gap = maximum_voiced_gap(curve);
    let mut events = Vec::<(u64, f32)>::new();
    for index in 1..curve.len() {
        let context_start = index.saturating_sub(CONTEXT_FRAMES);
        let context_end = (index + CONTEXT_FRAMES).min(curve.len());
        let trustworthy_context = curve[context_start..context_end]
            .iter()
            .filter(|point| trustworthy_f0_point(point))
            .collect::<Vec<_>>();
        if !trustworthy_f0_point(&curve[index])
            || trustworthy_context
                .windows(2)
                .any(|pair| pair[1].time.saturating_sub(pair[0].time) > maximum_voiced_gap)
        {
            continue;
        }
        let before = &curve[context_start..index];
        let after = &curve[index..context_end];
        let mut before_cents = before
            .iter()
            .filter(|point| trustworthy_f0_point(point))
            .map(|point| 1_200.0 * (point.hz / 440.0).log2())
            .collect::<Vec<_>>();
        let mut after_cents = after
            .iter()
            .filter(|point| trustworthy_f0_point(point))
            .map(|point| 1_200.0 * (point.hz / 440.0).log2())
            .collect::<Vec<_>>();
        if before_cents.len() < MIN_CONTEXT_FRAMES
            || after_cents.len() < MIN_CONTEXT_FRAMES
            || before_cents
                .iter()
                .max_by(|left, right| left.total_cmp(right))
                .zip(
                    before_cents
                        .iter()
                        .min_by(|left, right| left.total_cmp(right)),
                )
                .is_none_or(|(maximum, minimum)| maximum - minimum > EXIT_HYSTERESIS_CENTS)
            || after_cents
                .iter()
                .max_by(|left, right| left.total_cmp(right))
                .zip(
                    after_cents
                        .iter()
                        .min_by(|left, right| left.total_cmp(right)),
                )
                .is_none_or(|(maximum, minimum)| maximum - minimum > EXIT_HYSTERESIS_CENTS)
        {
            continue;
        }
        let (Some(before_center), Some(after_center)) =
            (median(&mut before_cents), median(&mut after_cents))
        else {
            continue;
        };
        let current = 1_200.0 * (curve[index].hz / 440.0).log2();
        let shift = (after_center - before_center).abs();
        if !current.is_finite()
            || (current - before_center).abs() <= EXIT_HYSTERESIS_CENTS
            || shift <= ENTER_CENTS
            || after
                .iter()
                .take(3)
                .filter(|point| {
                    trustworthy_f0_point(point)
                        && (1_200.0 * (point.hz / 440.0).log2() - before_center).abs()
                            > EXIT_HYSTERESIS_CENTS
                })
                .count()
                < 2
        {
            continue;
        }
        let strength = (shift / 1_200.0).clamp(0.0, 1.0);
        if let Some(previous) = events.last_mut()
            && curve[index].time.saturating_sub(previous.0) <= CONTEXT_DURATION
        {
            if strength > previous.1 {
                *previous = (curve[index].time, strength);
            }
        } else {
            events.push((curve[index].time, strength));
        }
    }
    events
}

fn has_word_edge(words: &[CanonicalWordBoundary], time: u64) -> bool {
    words.iter().any(|word| {
        word.range.start.abs_diff(time) <= BOUNDARY_EVIDENCE_TOLERANCE
            || word.range.end.abs_diff(time) <= BOUNDARY_EVIDENCE_TOLERANCE
    })
}

fn has_basic_pitch_attack(evidence: Option<&BasicPitchEvidenceV3>, time: u64) -> bool {
    evidence.is_some_and(|evidence| {
        let start = time.saturating_sub(BOUNDARY_EVIDENCE_TOLERANCE);
        let end = time.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE);
        let first = evidence.frames.partition_point(|frame| frame.time < start);
        let end = evidence.frames.partition_point(|frame| frame.time <= end);
        evidence.frames[first..end]
            .iter()
            .any(|frame| frame.onset_activation >= 0.5)
    })
}

fn has_acoustic_attack(evidence: Option<&AcousticEvidenceV1>, time: u64) -> bool {
    evidence.is_some_and(|evidence| {
        let start = time.saturating_sub(BOUNDARY_EVIDENCE_TOLERANCE);
        let end = time.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE);
        let first = evidence
            .frames
            .partition_point(|frame| frame.start < start)
            .saturating_sub(1);
        let end = evidence.frames.partition_point(|frame| frame.start <= end);
        evidence.frames[first..end].windows(2).any(|pair| {
            pair[0]
                .spectral_flux
                .zip(pair[1].spectral_flux)
                .is_some_and(|(previous, current)| current >= (previous * 2.0).max(1.0e-6))
        })
    })
}

fn has_caller_boundary(evidence: &[BoundaryAlternative], time: u64) -> bool {
    evidence.iter().any(|alternative| {
        matches!(
            alternative.kind,
            BoundaryEvidenceKind::Constraint | BoundaryEvidenceKind::PhraseConstraint
        ) && (alternative.range.start.abs_diff(time) <= BOUNDARY_EVIDENCE_TOLERANCE
            || alternative.range.end.abs_diff(time) <= BOUNDARY_EVIDENCE_TOLERANCE)
    })
}

fn inner_consolidation_time(range: TimeRange, time: u64) -> bool {
    time > range.start.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE)
        && time.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE) < range.end
}

fn has_persistent_shift_near(shifts: &[(u64, f32)], time: u64) -> bool {
    let lower = time.saturating_sub(BOUNDARY_EVIDENCE_TOLERANCE);
    let upper = time.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE);
    let first = shifts.partition_point(|(shift_time, _)| *shift_time < lower);
    shifts
        .get(first)
        .is_some_and(|(shift_time, _)| *shift_time <= upper)
}

fn consolidation_range_is_clear(
    range: TimeRange,
    words: &[CanonicalWordBoundary],
    curve: &[F0Point],
    acoustic: Option<&AcousticEvidenceV1>,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
    caller_boundaries: &[BoundaryAlternative],
    persistent_shifts: &[(u64, f32)],
) -> bool {
    const MAX_CONTEXT_MAD_CENTS: f32 = 60.0;
    if words.iter().any(|word| {
        inner_consolidation_time(range, word.range.start)
            || inner_consolidation_time(range, word.range.end)
    }) || caller_boundaries.iter().any(|boundary| {
        matches!(
            boundary.kind,
            BoundaryEvidenceKind::Constraint | BoundaryEvidenceKind::PhraseConstraint
        ) && (inner_consolidation_time(range, boundary.range.start)
            || inner_consolidation_time(range, boundary.range.end))
    }) || basic_pitch.is_some_and(|evidence| {
        let first = evidence
            .frames
            .partition_point(|frame| frame.time <= range.start);
        let end = evidence
            .frames
            .partition_point(|frame| frame.time < range.end);
        evidence.frames[first..end].iter().any(|frame| {
            inner_consolidation_time(range, frame.time) && frame.onset_activation >= 0.5
        })
    }) || acoustic.is_some_and(|evidence| {
        let first = evidence
            .frames
            .partition_point(|frame| frame.start <= range.start)
            .saturating_sub(1);
        let end = evidence
            .frames
            .partition_point(|frame| frame.start < range.end);
        evidence.frames[first..end].windows(2).any(|pair| {
            pair[0]
                .spectral_flux
                .zip(pair[1].spectral_flux)
                .is_some_and(|(previous, current)| {
                    inner_consolidation_time(range, pair[1].start)
                        && current >= (previous * 2.0).max(1.0e-6)
                })
        })
    }) || {
        let first = persistent_shifts.partition_point(|(time, _)| *time <= range.start);
        let end = persistent_shifts.partition_point(|(time, _)| *time < range.end);
        persistent_shifts[first..end]
            .iter()
            .any(|(time, _)| inner_consolidation_time(range, *time))
    } {
        return false;
    }
    let Some((_, mad)) = context_summary(curve, range.start, range.end) else {
        return false;
    };
    if mad > MAX_CONTEXT_MAD_CENTS {
        return false;
    }
    let first = curve.partition_point(|point| point.time < range.start);
    let end = curve.partition_point(|point| point.time < range.end);
    let trustworthy = curve[first..end]
        .iter()
        .filter(|point| trustworthy_f0_point(point))
        .collect::<Vec<_>>();
    trustworthy.len() >= MIN_CONTEXT_FRAMES
        && trustworthy
            .windows(2)
            .all(|pair| pair[1].time.saturating_sub(pair[0].time) <= 60_000)
}

fn f0_is_continuous_at(curve: &[F0Point], time: u64, maximum_gap: u64) -> Option<f32> {
    const MAX_CONTEXT_MAD_CENTS: f32 = 60.0;
    let (before, before_mad) = context_summary(curve, time.saturating_sub(CONTEXT_DURATION), time)?;
    let (after, after_mad) = context_summary(curve, time, time.saturating_add(CONTEXT_DURATION))?;
    if before_mad > MAX_CONTEXT_MAD_CENTS || after_mad > MAX_CONTEXT_MAD_CENTS {
        return None;
    }
    let next_index = curve.partition_point(|point| point.time < time);
    let previous = curve[..next_index]
        .iter()
        .rev()
        .find(|point| trustworthy_f0_point(point))?;
    let next = curve[next_index..]
        .iter()
        .find(|point| trustworthy_f0_point(point))?;
    if next.time.saturating_sub(previous.time) > maximum_gap {
        return None;
    }
    let shift = (after - before).abs();
    (shift <= EXIT_HYSTERESIS_CENTS).then_some(1.0 - shift / EXIT_HYSTERESIS_CENTS)
}

/// Generates a coarser duration state only when every removed primary edge is
/// unsupported by words, caller constraints, onset evidence, or a persistent
/// continuous-F0 transition. It never mutates primary geometry and therefore
/// remains an auditable challenger rather than hidden cleanup.
pub(crate) fn f0_consolidation_challengers(
    boundaries: &BoundaryEvidenceSet,
    words: &[CanonicalWordBoundary],
    primary_pitch_owner: &str,
    curve: &[F0Point],
    acoustic: Option<&AcousticEvidenceV1>,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
    caller_boundaries: &[BoundaryAlternative],
) -> Result<Vec<BoundaryAlternative>, String> {
    if boundaries.kind == BoundaryEvidenceKind::F0Derived || boundaries.segments.len() < 2 {
        return Ok(Vec::new());
    }
    if primary_pitch_owner.trim().is_empty() {
        return Err("F0 consolidation source identity is empty".to_string());
    }
    let consolidation_evidence_count = words
        .len()
        .checked_add(caller_boundaries.len())
        .ok_or_else(|| "F0 consolidation evidence count overflows".to_string())?;
    validate_candidate_evidence_relation_count(
        boundaries.segments.len(),
        consolidation_evidence_count,
    )?;
    validate_candidate_context_relations(
        boundaries.segments.iter().map(|segment| segment.range),
        "rmvpe",
        curve,
        &[],
        acoustic,
        basic_pitch,
    )?;

    let persistent_shifts = persistent_f0_shifts(curve);
    let maximum_gap = maximum_voiced_gap(curve);
    let mut runs = Vec::<(usize, usize, f32)>::new();
    let mut run_start = 0;
    let mut run_support = 1.0_f32;
    for index in 1..boundaries.segments.len() {
        let previous = &boundaries.segments[index - 1];
        let next = &boundaries.segments[index];
        let time = next.range.start;
        let adjacent = previous.range.end == time;
        let support = adjacent
            .then(|| f0_is_continuous_at(curve, time, maximum_gap))
            .flatten()
            .filter(|_| !has_word_edge(words, time))
            .filter(|_| !has_basic_pitch_attack(basic_pitch, time))
            .filter(|_| !has_acoustic_attack(acoustic, time))
            .filter(|_| !has_caller_boundary(caller_boundaries, time))
            .filter(|_| !has_persistent_shift_near(&persistent_shifts, time));
        if let Some(support) = support {
            run_support = run_support.min(support);
        } else {
            if index - run_start >= 2 {
                runs.push((run_start, index - 1, run_support));
            }
            run_start = index;
            run_support = 1.0;
        }
    }
    if boundaries.segments.len() - run_start >= 2 {
        runs.push((run_start, boundaries.segments.len() - 1, run_support));
    }

    let mut challengers = Vec::new();
    for (start, end, support) in runs {
        let range = TimeRange::new(
            boundaries.segments[start].range.start,
            boundaries.segments[end].range.end,
        )?;
        if !consolidation_range_is_clear(
            range,
            words,
            curve,
            acoustic,
            basic_pitch,
            caller_boundaries,
            &persistent_shifts,
        ) {
            continue;
        }
        challengers.push(BoundaryAlternative {
            source_expert: format!("{primary_pitch_owner}.f0_consolidation"),
            range,
            kind: BoundaryEvidenceKind::F0Consolidation,
            fractional_midi: None,
            source_local_score: Some(support.clamp(0.0, 1.0)),
            source_local_pitch_score: None,
            calibrated_boundary_confidence: None,
            calibrated_pitch_confidence: None,
            hard: false,
        });
    }
    Ok(challengers)
}

pub(crate) fn validate_candidate_evidence_relation_count(
    candidate_count: usize,
    evidence_count: usize,
) -> Result<(), String> {
    if candidate_count
        .checked_mul(evidence_count)
        .is_some_and(|relations| relations <= MAX_CANDIDATE_EVIDENCE_RELATIONS)
    {
        Ok(())
    } else {
        Err("candidate graph exceeds the bounded candidate-evidence relation limit".to_string())
    }
}

fn validate_candidate_context_relation_total(total: usize) -> Result<(), String> {
    if total > MAX_CANDIDATE_CONTEXT_RELATIONS {
        return Err("candidate context relations exceed the bounded limit".to_string());
    }
    Ok(())
}

pub(crate) fn validate_candidate_context_relations(
    ranges: impl IntoIterator<Item = TimeRange>,
    primary_pitch_owner: &str,
    rmvpe_curve: &[F0Point],
    fcpe_curve: &[F0Point],
    acoustic: Option<&AcousticEvidenceV1>,
    basic_pitch: Option<&BasicPitchEvidenceV3>,
) -> Result<(), String> {
    let acoustic_window = acoustic.map(|evidence| {
        u64::from(evidence.window_samples).saturating_mul(1_000_000)
            / u64::from(evidence.sample_rate).max(1)
    });
    let primary_curve = match primary_pitch_owner {
        "rmvpe" => rmvpe_curve,
        "fcpe" => fcpe_curve,
        _ => return Err("unsupported primary F0 expert".to_string()),
    };
    let mut total = 0usize;
    let mut add = |count: usize| -> Result<(), String> {
        total = total
            .checked_add(count)
            .ok_or_else(|| "candidate context relation count overflows".to_string())?;
        validate_candidate_context_relation_total(total)
    };
    for range in ranges {
        for curve in [rmvpe_curve, fcpe_curve] {
            let first = curve.partition_point(|point| point.time < range.start);
            let end = curve.partition_point(|point| point.time < range.end);
            add(end.saturating_sub(first))?;
        }
        let primary_first = primary_curve.partition_point(|point| point.time < range.start);
        let primary_end = primary_curve.partition_point(|point| point.time < range.end);
        add(primary_end.saturating_sub(primary_first))?;
        if let (Some(evidence), Some(window)) = (acoustic, acoustic_window) {
            let first = evidence
                .frames
                .partition_point(|frame| frame.start.saturating_add(window) <= range.start);
            let end = evidence
                .frames
                .partition_point(|frame| frame.start < range.end);
            add(end.saturating_sub(first))?;
        }
        if let Some(evidence) = basic_pitch {
            let onset_start = range.start.saturating_sub(60_000);
            let onset_end = range.start.saturating_add(60_000);
            let onset_first = evidence
                .frames
                .partition_point(|frame| frame.time < onset_start);
            let onset_last = evidence
                .frames
                .partition_point(|frame| frame.time < onset_end);
            add(onset_last.saturating_sub(onset_first))?;
            let local_first = evidence
                .frames
                .partition_point(|frame| frame.time < range.start);
            let local_end = evidence
                .frames
                .partition_point(|frame| frame.time < range.end);
            add(local_end.saturating_sub(local_first))?;
        }
    }
    Ok(())
}

pub(crate) fn validate_candidate_state_count(count: usize) -> Result<(), String> {
    if count > MAX_EXPANDED_CANDIDATES {
        return Err("candidate pool exceeds the bounded candidate limit".to_string());
    }
    Ok(())
}

fn validate_projected_state_count(
    counts: impl IntoIterator<Item = usize>,
) -> Result<usize, String> {
    let mut projected = 0usize;
    for count in counts {
        if count > MAX_PITCH_PROPOSALS_PER_SEGMENT {
            return Err("one duration state exceeds the bounded pitch-proposal limit".to_string());
        }
        projected = projected
            .checked_add(count)
            .ok_or_else(|| "expanded candidate graph size overflows".to_string())?;
        validate_candidate_state_count(projected)
            .map_err(|_| "expanded candidate graph exceeds the bounded state limit".to_string())?;
    }
    Ok(projected)
}

fn validate_projected_nested_relations(
    state_relation_counts: impl IntoIterator<Item = (usize, usize)>,
) -> Result<usize, String> {
    let mut projected = 0usize;
    for (state_count, relations_per_state) in state_relation_counts {
        let relations = state_count
            .checked_mul(relations_per_state)
            .ok_or_else(|| "pitch-expanded candidate relation count overflows".to_string())?;
        projected = projected
            .checked_add(relations)
            .ok_or_else(|| "pitch-expanded candidate relation count overflows".to_string())?;
        if projected > MAX_CANDIDATE_EVIDENCE_RELATIONS {
            return Err(format!(
                "pitch-expanded candidate graph exceeds {MAX_CANDIDATE_EVIDENCE_RELATIONS} nested evidence relations"
            ));
        }
    }
    Ok(projected)
}

fn expanded_state_id(candidate_id: &str, proposal_index: usize, center_hz: f32) -> String {
    format!(
        "{candidate_id}-pitch-{proposal_index}-{:08x}",
        center_hz.to_bits()
    )
}

fn reserve_expanded_state_id(
    preferred: String,
    candidate_index: usize,
    proposal_index: usize,
    base_ids: &std::collections::BTreeSet<String>,
    assigned: &mut std::collections::BTreeSet<String>,
) -> String {
    if !base_ids.contains(preferred.as_str()) && assigned.insert(preferred.clone()) {
        return preferred;
    }
    let mut collision_index = 0usize;
    loop {
        let candidate =
            format!("{preferred}-state-{candidate_index}-{proposal_index}-{collision_index}");
        if !base_ids.contains(candidate.as_str()) && assigned.insert(candidate.clone()) {
            return candidate;
        }
        collision_index += 1;
    }
}

/// Materializes every typed pitch proposal as an independent duration state.
/// Bounds apply after expansion because this is the graph consumed by both the
/// exact decoder and the size-limited AI adapter protocol.
pub(crate) fn expand_pitch_alternative_states(
    candidates: Vec<SegmentCandidate>,
) -> Result<Vec<SegmentCandidate>, String> {
    let mut proposal_counts = Vec::with_capacity(candidates.len());
    let mut state_relation_counts = Vec::with_capacity(candidates.len());
    let mut base_ids = std::collections::BTreeSet::new();
    for candidate in &candidates {
        if !base_ids.insert(candidate.id.clone()) {
            return Err(format!(
                "candidate pool contains duplicate candidate id {} before pitch expansion",
                candidate.id
            ));
        }
        let mut identities = std::collections::BTreeSet::new();
        identities.insert((
            candidate.target_pitch_source.as_str(),
            candidate.center_pitch_hz.to_bits(),
        ));
        for alternative in &candidate.alternatives {
            identities.insert((
                alternative.source_expert.as_str(),
                alternative.center_hz.to_bits(),
            ));
        }
        let proposal_count = identities.len();
        proposal_counts.push(proposal_count);
        let nested_relations = candidate
            .boundary_alternatives
            .len()
            .checked_add(candidate.boundary_constraints.len())
            .and_then(|count| count.checked_add(candidate.technique_evidence.len()))
            .and_then(|count| count.checked_add(proposal_count.saturating_sub(1)))
            .and_then(|count| {
                candidate
                    .boundary_constraints
                    .iter()
                    .try_fold(count, |total, constraint| {
                        total.checked_add(constraint.depends_on.len())
                    })
            })
            .ok_or_else(|| "pitch-expanded candidate relation count overflows".to_string())?;
        state_relation_counts.push((proposal_count, nested_relations));
    }
    let projected = validate_projected_state_count(proposal_counts)?;
    validate_projected_nested_relations(state_relation_counts)?;

    let mut expanded = Vec::with_capacity(projected);
    let mut assigned_ids = std::collections::BTreeSet::new();
    for (candidate_index, candidate) in candidates.into_iter().enumerate() {
        let mut proposals = vec![PitchAlternative {
            source_expert: candidate.target_pitch_source.clone(),
            center_hz: candidate.center_pitch_hz,
            cents_from_target: 0.0,
            confidence: None,
        }];
        proposals.extend(candidate.alternatives.clone());
        for (index, selected) in proposals.iter().enumerate() {
            if proposals[..index].iter().any(|previous| {
                previous.source_expert == selected.source_expert
                    && previous.center_hz.to_bits() == selected.center_hz.to_bits()
            }) {
                continue;
            }
            let fractional_midi = 69.0 + 12.0 * (selected.center_hz / 440.0).log2();
            let target_midi = decide_fractional_target(fractional_midi, &selected.source_expert)?;
            let mut state = candidate.clone();
            if index == 0 {
                assigned_ids.insert(state.id.clone());
            } else {
                state.id = reserve_expanded_state_id(
                    expanded_state_id(&candidate.id, index, selected.center_hz),
                    candidate_index,
                    index,
                    &base_ids,
                    &mut assigned_ids,
                );
            }
            state.target_midi = target_midi;
            state.target_pitch_source = selected.source_expert.clone();
            state.center_pitch_hz = selected.center_hz;
            state.rmvpe_cents_difference = state
                .rmvpe_center_hz
                .map(|center| 1_200.0 * (center / selected.center_hz).log2());
            state.alternatives = proposals
                .iter()
                .enumerate()
                .filter(|(proposal_index, _)| *proposal_index != index)
                .map(|(_, proposal)| PitchAlternative {
                    source_expert: proposal.source_expert.clone(),
                    center_hz: proposal.center_hz,
                    cents_from_target: 1_200.0 * (proposal.center_hz / selected.center_hz).log2(),
                    confidence: proposal.confidence,
                })
                .collect();
            expanded.push(state);
        }
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fusion::{BoundaryCandidateRole, BoundarySegmentEvidence, TechniqueScores};

    fn pitch_candidate() -> SegmentCandidate {
        SegmentCandidate {
            id: "candidate".to_string(),
            range: TimeRange::new(100_000, 200_000).unwrap(),
            target_midi: 69,
            boundary_source: "game".to_string(),
            boundary_kind: BoundaryEvidenceKind::Game,
            boundary_role: BoundaryCandidateRole::Primary,
            boundary_fractional_midi: Some(69.0),
            boundary_decision_parameter: Some(0.2),
            presence_decision_parameter: Some(0.2),
            boundary_hard: false,
            boundary_support: None,
            boundary_calibrated_confidence: None,
            target_pitch_source: "pitch/a".to_string(),
            target_pitch_source_local_score: None,
            target_pitch_calibrated_confidence: None,
            center_pitch_hz: 440.1,
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
            boundary_alternatives: Vec::new(),
            boundary_constraints: Vec::new(),
            technique_evidence: Vec::new(),
            techniques: TechniqueScores::default(),
            word_id: None,
            alternatives: Vec::new(),
        }
    }

    fn stable_curve() -> Vec<F0Point> {
        (0..30)
            .map(|index| F0Point {
                time: 100_000 + index * 10_000,
                hz: 440.0,
                confidence: Some(0.9),
            })
            .collect()
    }

    fn fragmented_boundaries() -> BoundaryEvidenceSet {
        BoundaryEvidenceSet {
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
        }
    }

    #[test]
    fn low_confidence_plateau_does_not_create_a_pitch_transition() {
        let curve = (0..12)
            .map(|index| F0Point {
                time: 100_000 + index * 10_000,
                hz: if index < 6 { 440.0 } else { 880.0 },
                confidence: Some(if index < 6 { 0.9 } else { 0.1 }),
            })
            .collect::<Vec<_>>();
        assert!(persistent_f0_shifts(&curve).is_empty());
    }

    #[test]
    fn stable_f0_adds_a_coarser_state_without_an_external_spanning_candidate() {
        let challengers = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &stable_curve(),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(challengers.len(), 1);
        assert_eq!(
            challengers[0].range,
            TimeRange::new(100_000, 400_000).unwrap()
        );
        assert_eq!(challengers[0].kind, BoundaryEvidenceKind::F0Consolidation);
    }

    #[test]
    fn consolidation_is_label_invariant_and_never_crosses_a_real_f0_break() {
        let original = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &stable_curve(),
            None,
            None,
            &[],
        )
        .unwrap();
        let mut relabeled = fragmented_boundaries();
        relabeled.source_expert = "arbitrary-renamed-boundary-source".to_string();
        let renamed = f0_consolidation_challengers(
            &relabeled,
            &[],
            "rmvpe",
            &stable_curve(),
            None,
            None,
            &[],
        )
        .unwrap();
        assert_eq!(original, renamed);

        let sustained_octave = (0..30)
            .map(|index| F0Point {
                time: 100_000 + index * 10_000,
                hz: if index < 10 { 440.0 } else { 880.0 },
                confidence: Some(0.9),
            })
            .collect::<Vec<_>>();
        let shifted = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &sustained_octave,
            None,
            None,
            &[],
        )
        .unwrap();
        assert!(shifted.iter().all(|candidate| {
            !(candidate.range.start < 200_000 && candidate.range.end > 200_000)
        }));

        let gapped_octave = (0..10)
            .map(|index| F0Point {
                time: if index < 5 {
                    100_000 + index * 10_000
                } else {
                    300_000 + (index - 5) * 10_000
                },
                hz: if index < 5 { 440.0 } else { 880.0 },
                confidence: Some(0.9),
            })
            .collect::<Vec<_>>();
        assert!(
            persistent_f0_shifts(&gapped_octave)
                .iter()
                .all(|(time, _)| *time != 300_000)
        );

        let sparse_gap = vec![
            F0Point {
                time: 100_000,
                hz: 440.0,
                confidence: Some(0.9),
            },
            F0Point {
                time: 200_000,
                hz: 440.0,
                confidence: Some(0.9),
            },
            F0Point {
                time: 300_000,
                hz: 440.0,
                confidence: Some(0.9),
            },
            F0Point {
                time: 600_000,
                hz: 880.0,
                confidence: Some(0.9),
            },
            F0Point {
                time: 700_000,
                hz: 880.0,
                confidence: Some(0.9),
            },
            F0Point {
                time: 800_000,
                hz: 880.0,
                confidence: Some(0.9),
            },
        ];
        assert!(persistent_f0_shifts(&sparse_gap).is_empty());

        let smooth_glide = (0..24)
            .map(|index| F0Point {
                time: 100_000 + index * 10_000,
                hz: 440.0 * 2.0_f32.powf(index as f32 * 40.0 / 1_200.0),
                confidence: Some(0.9),
            })
            .collect::<Vec<_>>();
        assert!(persistent_f0_shifts(&smooth_glide).is_empty());

        let unvoiced_gap = stable_curve()
            .into_iter()
            .filter(|point| !(180_000..240_000).contains(&point.time))
            .collect::<Vec<_>>();
        let gapped = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &unvoiced_gap,
            None,
            None,
            &[],
        )
        .unwrap();
        assert!(gapped.iter().all(|candidate| {
            !(candidate.range.start < 200_000 && candidate.range.end > 200_000)
        }));

        let hard_cut = BoundaryAlternative {
            source_expert: "caller".to_string(),
            range: TimeRange::new(200_000, 200_001).unwrap(),
            kind: BoundaryEvidenceKind::Constraint,
            fractional_midi: None,
            source_local_score: Some(1.0),
            source_local_pitch_score: None,
            calibrated_boundary_confidence: None,
            calibrated_pitch_confidence: None,
            hard: true,
        };
        let constrained = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &stable_curve(),
            None,
            None,
            &[hard_cut],
        )
        .unwrap();
        assert!(constrained.iter().all(|candidate| {
            !(candidate.range.start < 200_000 && candidate.range.end > 200_000)
        }));
    }

    #[test]
    fn an_internal_measured_attack_blocks_the_whole_consolidated_range() {
        let basic_pitch = BasicPitchEvidenceV3 {
            frames: vec![crate::artifact::BasicPitchFrameV3 {
                time: 250_000,
                note_activation: 0.9,
                onset_activation: 0.95,
                contour_class: 42,
                contour_activation: 0.8,
            }],
            model_manifest_sha256: "a".repeat(64),
            runtime_manifest_sha256: "b".repeat(64),
        };
        let challengers = f0_consolidation_challengers(
            &fragmented_boundaries(),
            &[],
            "rmvpe",
            &stable_curve(),
            None,
            Some(&basic_pitch),
            &[],
        )
        .unwrap();
        assert!(challengers.iter().all(|candidate| {
            !(candidate.range.start < 250_000 && candidate.range.end > 250_000)
        }));
    }

    #[test]
    fn expanded_ids_retain_exact_fractional_proposal_identity() {
        let mut candidate = pitch_candidate();
        candidate.alternatives = vec![
            PitchAlternative {
                source_expert: "pitch.a".to_string(),
                center_hz: 440.2,
                cents_from_target: 0.0,
                confidence: Some(0.8),
            },
            PitchAlternative {
                source_expert: "pitch/a".to_string(),
                center_hz: 440.1,
                cents_from_target: 0.0,
                confidence: Some(0.7),
            },
            PitchAlternative {
                source_expert: "pitch/a".to_string(),
                center_hz: 440.2,
                cents_from_target: 0.0,
                confidence: Some(0.6),
            },
            PitchAlternative {
                source_expert: "pitch.a".to_string(),
                center_hz: 440.2,
                cents_from_target: 0.0,
                confidence: Some(0.5),
            },
        ];
        let expanded = expand_pitch_alternative_states(vec![candidate]).unwrap();
        assert_eq!(expanded.len(), 3, "only the exact duplicate is collapsed");
        assert!(expanded.iter().all(|candidate| candidate.target_midi == 69));
        let ids = expanded
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), expanded.len());
        crate::fusion::validate_candidate_pool(&expanded).unwrap();

        let first = expanded_state_id("candidate", 1, 440.1);
        let second = expanded_state_id("candidate", 2, 440.2);
        assert_ne!(first, second);
        assert!(first.ends_with(&format!("{:08x}", 440.1_f32.to_bits())));
        assert!(second.ends_with(&format!("{:08x}", 440.2_f32.to_bits())));

        let mut first_candidate = pitch_candidate();
        first_candidate.alternatives = vec![PitchAlternative {
            source_expert: "pitch.b".to_string(),
            center_hz: 441.0,
            cents_from_target: 0.0,
            confidence: None,
        }];
        let mut colliding_base = pitch_candidate();
        colliding_base.id = expanded_state_id("candidate", 1, 441.0);
        colliding_base.range = TimeRange::new(200_000, 300_000).unwrap();
        let expanded =
            expand_pitch_alternative_states(vec![first_candidate, colliding_base]).unwrap();
        let ids = expanded
            .iter()
            .map(|candidate| candidate.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(ids.len(), expanded.len());
        crate::fusion::validate_candidate_pool(&expanded).unwrap();
    }

    #[test]
    fn persistent_shift_neighbor_query_uses_exact_tolerance_edges() {
        let shifts = vec![(100_000, 200.0), (500_000, 300.0)];
        assert!(has_persistent_shift_near(&shifts, 100_000));
        assert!(has_persistent_shift_near(
            &shifts,
            100_000 + BOUNDARY_EVIDENCE_TOLERANCE
        ));
        assert!(!has_persistent_shift_near(
            &shifts,
            100_001 + BOUNDARY_EVIDENCE_TOLERANCE
        ));
    }

    #[test]
    fn post_expansion_graph_size_is_bounded() {
        validate_candidate_context_relation_total(MAX_CANDIDATE_CONTEXT_RELATIONS).unwrap();
        assert!(
            validate_candidate_context_relation_total(MAX_CANDIDATE_CONTEXT_RELATIONS + 1)
                .unwrap_err()
                .contains("context relations")
        );
        assert_eq!(validate_projected_state_count([2, 3]).unwrap(), 5);
        assert_eq!(MAX_CANDIDATE_EVIDENCE_RELATIONS % 1_000, 0);
        let relations_per_state = MAX_CANDIDATE_EVIDENCE_RELATIONS / 1_000;
        assert_eq!(
            validate_projected_nested_relations([(1_000, relations_per_state)]).unwrap(),
            MAX_CANDIDATE_EVIDENCE_RELATIONS
        );
        assert!(
            validate_projected_nested_relations([(1_000, relations_per_state + 1)])
                .unwrap_err()
                .contains("nested evidence relations")
        );
        validate_candidate_state_count(MAX_EXPANDED_CANDIDATES).unwrap();
        assert!(
            validate_candidate_state_count(MAX_EXPANDED_CANDIDATES + 1)
                .unwrap_err()
                .contains("bounded candidate limit")
        );
        assert!(
            validate_projected_state_count(std::iter::repeat_n(1, MAX_EXPANDED_CANDIDATES + 1))
                .unwrap_err()
                .contains("bounded state limit")
        );
        assert!(
            validate_projected_state_count([MAX_PITCH_PROPOSALS_PER_SEGMENT + 1])
                .unwrap_err()
                .contains("pitch-proposal limit")
        );
    }
}
