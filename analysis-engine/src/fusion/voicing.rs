//! Explicit pitch activity in the covering candidate graph.

use serde::{Deserialize, Serialize};

/// A covering state can represent observed absence of pitched singing without
/// inventing a MIDI target. Continuous F0 remains an independent raw curve.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CandidateTarget {
    Pitched { midi: u8, center_hz: f32 },
    Unpitched,
}

impl CandidateTarget {
    pub fn as_pitched(self) -> Option<(u8, f32)> {
        match self {
            Self::Pitched { midi, center_hz } => Some((midi, center_hz)),
            Self::Unpitched => None,
        }
    }

    pub fn midi(self) -> Option<u8> {
        self.as_pitched().map(|(midi, _)| midi)
    }

    pub fn center_hz(self) -> Option<f32> {
        self.as_pitched().map(|(_, center_hz)| center_hz)
    }

    pub fn is_pitched(self) -> bool {
        matches!(self, Self::Pitched { .. })
    }
}

use crate::artifact::AcousticEvidence;

use super::{
    BOUNDARY_EVIDENCE_TOLERANCE, BoundaryAlternative, BoundaryCandidateRole, BoundaryEvidenceKind,
    BoundaryEvidenceSet, F0Point, PitchGrid, SegmentCandidate, TimeRange,
};

/// Absolute-time intervals in which both continuous pitch experts report no
/// pitch and independent DSP supplies no reliable periodic support. This is
/// source evidence, not a confidence inferred from the selected candidate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoicingCandidateEvidence {
    pub source_experts: Vec<String>,
    pub unsupported_ranges: Vec<TimeRange>,
}

impl VoicingCandidateEvidence {
    pub fn duration(&self) -> u64 {
        self.unsupported_ranges
            .iter()
            .map(|range| range.end - range.start)
            .sum()
    }

    pub(super) fn valid_for(&self, range: TimeRange) -> bool {
        !self.source_experts.is_empty()
            && self
                .source_experts
                .iter()
                .all(|source| !source.trim().is_empty())
            && self.unsupported_ranges.iter().all(|part| {
                part.start >= range.start && part.end <= range.end && part.end > part.start
            })
            && self
                .unsupported_ranges
                .windows(2)
                .all(|pair| pair[0].end <= pair[1].start)
    }
}

fn grid_end(grid: PitchGrid) -> Result<u64, String> {
    grid.hop
        .checked_mul(grid.frame_count as u64)
        .and_then(|duration| grid.start.checked_add(duration))
        .ok_or_else(|| "voicing observation grid overflows".to_string())
}

fn pitch_present(curve: &[F0Point], grid: PitchGrid, time: u64) -> bool {
    let next = curve.partition_point(|point| point.time <= time);
    next.checked_sub(1)
        .and_then(|index| curve.get(index))
        .is_some_and(|point| point.time <= time && time < point.time.saturating_add(grid.hop))
}

/// Missing experts or incomplete DSP coverage are unknown, never silence.
/// Even a weak voiced neural frame or a quiet but periodic DSP frame protects
/// a sustained vowel. The detector does not threshold absolute/relative RMS.
pub(super) fn unsupported_voicing_ranges(
    rmvpe: &[F0Point],
    rmvpe_grid: Option<PitchGrid>,
    fcpe: &[F0Point],
    fcpe_grid: Option<PitchGrid>,
    acoustic: Option<&AcousticEvidence>,
) -> Result<Vec<TimeRange>, String> {
    let (Some(rmvpe_grid), Some(fcpe_grid), Some(acoustic)) = (rmvpe_grid, fcpe_grid, acoustic)
    else {
        return Ok(Vec::new());
    };
    let Some(first_acoustic) = acoustic.frames.first() else {
        return Ok(Vec::new());
    };
    let Some(last_acoustic) = acoustic.frames.last() else {
        return Ok(Vec::new());
    };
    let start = rmvpe_grid
        .start
        .max(fcpe_grid.start)
        .max(first_acoustic.start);
    let end = grid_end(rmvpe_grid)?
        .min(grid_end(fcpe_grid)?)
        .min(last_acoustic.start.saturating_add(acoustic.hop));
    if end <= start {
        return Ok(Vec::new());
    }
    let mut edges = vec![start, end];
    for grid in [rmvpe_grid, fcpe_grid] {
        for index in 0..=grid.frame_count {
            let time = grid
                .start
                .saturating_add(grid.hop.saturating_mul(index as u64));
            if time > start && time < end {
                edges.push(time);
            }
        }
    }
    for frame in &acoustic.frames {
        for time in [frame.start, frame.start.saturating_add(acoustic.hop)] {
            if time > start && time < end {
                edges.push(time);
            }
        }
    }
    edges.sort_unstable();
    edges.dedup();
    let mut ranges: Vec<TimeRange> = Vec::new();
    for pair in edges.windows(2) {
        let time = pair[0];
        if pitch_present(rmvpe, rmvpe_grid, time) || pitch_present(fcpe, fcpe_grid, time) {
            continue;
        }
        let next = acoustic.frames.partition_point(|frame| frame.start <= time);
        let Some(frame) = next
            .checked_sub(1)
            .and_then(|index| acoustic.frames.get(index))
        else {
            continue;
        };
        if time >= frame.start.saturating_add(acoustic.hop)
            || (frame.periodicity >= 0.6 && frame.snr_db >= 10.0)
        {
            continue;
        }
        if let Some(previous) = ranges.last_mut()
            && previous.end == time
        {
            previous.end = pair[1];
        } else {
            ranges.push(TimeRange {
                start: time,
                end: pair[1],
            });
        }
    }
    ranges.retain(|range| range.end - range.start >= BOUNDARY_EVIDENCE_TOLERANCE);
    Ok(ranges)
}

pub(super) fn voicing_evidence(
    range: TimeRange,
    unsupported: &[TimeRange],
) -> Option<VoicingCandidateEvidence> {
    let first = unsupported.partition_point(|part| part.end <= range.start);
    let ranges = unsupported[first..]
        .iter()
        .take_while(|part| part.start < range.end)
        .map(|part| TimeRange {
            start: part.start.max(range.start),
            end: part.end.min(range.end),
        })
        .collect::<Vec<_>>();
    (!ranges.is_empty()).then(|| VoicingCandidateEvidence {
        source_experts: vec!["rmvpe".into(), "fcpe".into(), "acoustic_dsp".into()],
        unsupported_ranges: ranges,
    })
}

/// Keep original duration candidates verbatim, and add covering pieces at
/// observed activity boundaries. Short recovery pieces are ordinary competing
/// states; a nearby native note can instead extend to the observed recovery
/// and absorb the piece, paying one semantic note-state cost.
pub(super) fn voicing_boundary_challengers(
    boundaries: &BoundaryEvidenceSet,
    alternatives: &[BoundaryAlternative],
    unsupported: &[TimeRange],
) -> Vec<BoundaryAlternative> {
    let mut output = Vec::new();
    for alternative in boundaries
        .segments
        .iter()
        .filter(|segment| {
            boundaries.kind != BoundaryEvidenceKind::Game || segment.fractional_midi.is_some()
        })
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
        .chain(alternatives.iter().cloned())
    {
        let mut cursor = alternative.range.start;
        let first = unsupported.partition_point(|part| part.end <= cursor);
        let overlapping = unsupported[first..]
            .iter()
            .take_while(|part| part.start < alternative.range.end)
            .collect::<Vec<_>>();
        if !overlapping.is_empty() {
            for gap in overlapping {
                if gap.start > cursor {
                    output.push(derived_boundary(
                        &alternative,
                        TimeRange {
                            start: cursor,
                            end: gap.start.min(alternative.range.end),
                        },
                    ));
                }
                cursor = cursor.max(gap.end);
                if cursor >= alternative.range.end {
                    break;
                }
            }
            if cursor < alternative.range.end {
                output.push(derived_boundary(
                    &alternative,
                    TimeRange {
                        start: cursor,
                        end: alternative.range.end,
                    },
                ));
            }
        }
        // Timing disagreement at a recovery is another duration proposal. It
        // does not create a new native onset observation or move caller text.
        let first = unsupported.partition_point(|gap| {
            gap.end.saturating_add(BOUNDARY_EVIDENCE_TOLERANCE) < alternative.range.start
        });
        for gap in unsupported[first..].iter().take_while(|gap| {
            gap.end
                <= alternative
                    .range
                    .start
                    .saturating_add(BOUNDARY_EVIDENCE_TOLERANCE)
        }) {
            if gap.end != alternative.range.start && gap.end < alternative.range.end {
                output.push(derived_boundary(
                    &alternative,
                    TimeRange {
                        start: gap.end,
                        end: alternative.range.end,
                    },
                ));
            }
        }
    }
    output
}

fn derived_boundary(original: &BoundaryAlternative, range: TimeRange) -> BoundaryAlternative {
    BoundaryAlternative {
        source_expert: original.source_expert.clone(),
        range,
        kind: BoundaryEvidenceKind::Voicing,
        fractional_midi: original.fractional_midi,
        source_local_score: None,
        source_local_pitch_score: original.source_local_pitch_score,
        calibrated_boundary_confidence: None,
        calibrated_pitch_confidence: original.calibrated_pitch_confidence,
        hard: false,
    }
}

/// Rest candidates stay within existing primary coverage. Whole intervals and
/// pieces at existing edges let the same exact-cover validator respect hard
/// boundaries without ever dropping coverage or rewriting a selected state.
pub(super) fn append_unpitched_candidates(
    candidates: &mut Vec<SegmentCandidate>,
    unsupported: &[TimeRange],
) -> Result<(), String> {
    let Some(template) = candidates.first().cloned() else {
        return Ok(());
    };
    let mut ranges = candidates
        .iter()
        .filter(|candidate| candidate.boundary_role == BoundaryCandidateRole::Primary)
        .map(|candidate| candidate.range)
        .collect::<Vec<_>>();
    if ranges.is_empty() {
        ranges.extend(candidates.iter().map(|candidate| candidate.range));
    }
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut components: Vec<TimeRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = components.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            components.push(range);
        }
    }
    let mut edges = candidates
        .iter()
        .flat_map(|candidate| [candidate.range.start, candidate.range.end])
        .collect::<Vec<_>>();
    edges.sort_unstable();
    edges.dedup();
    let mut rest_ranges = std::collections::BTreeSet::new();
    for component in components {
        let first = unsupported.partition_point(|gap| gap.end <= component.start);
        for gap in unsupported[first..]
            .iter()
            .take_while(|gap| gap.start < component.end)
        {
            let start = gap.start.max(component.start);
            let end = gap.end.min(component.end);
            rest_ranges.insert((start, end));
            let first_edge = edges.partition_point(|time| *time <= start);
            let mut cursor = start;
            for edge in edges[first_edge..]
                .iter()
                .copied()
                .take_while(|time| *time < end)
            {
                rest_ranges.insert((cursor, edge));
                cursor = edge;
            }
            rest_ranges.insert((cursor, end));
        }
    }
    if candidates
        .len()
        .checked_add(rest_ranges.len())
        .is_none_or(|count| count > super::candidate_states::MAX_EXPANDED_CANDIDATES)
    {
        return Err("activity candidate graph exceeds the bounded candidate limit".into());
    }
    for (index, (start, end)) in rest_ranges.into_iter().enumerate() {
        let range = TimeRange { start, end };
        let mut candidate = template.clone();
        candidate.id = format!("unpitched-segment-{index}");
        candidate.range = range;
        candidate.target = CandidateTarget::Unpitched;
        candidate.boundary_source = "continuous_voicing".into();
        candidate.boundary_kind = BoundaryEvidenceKind::Voicing;
        candidate.boundary_role = BoundaryCandidateRole::Challenger;
        candidate.boundary_fractional_midi = None;
        candidate.boundary_decision_parameter = None;
        candidate.presence_decision_parameter = None;
        candidate.boundary_hard = false;
        candidate.boundary_support = None;
        candidate.boundary_calibrated_confidence = None;
        candidate.target_pitch_source = "unpitched".into();
        candidate.target_pitch_source_local_score = None;
        candidate.target_pitch_calibrated_confidence = None;
        candidate.continuous_pitch_error_integral = None;
        candidate.continuous_pitch_observed_duration = None;
        candidate.rmvpe_center_hz = None;
        candidate.rmvpe_confidence = None;
        candidate.rmvpe_cents_difference = None;
        candidate.rmvpe_voiced_ratio = None;
        candidate.rmvpe_pitch_mad_cents = None;
        candidate.fcpe_center_hz = None;
        candidate.fcpe_observed_ratio = None;
        candidate.fcpe_pitch_mad_cents = None;
        candidate.fcpe_cents_from_rmvpe = None;
        candidate.fcpe_supports_rmvpe = None;
        candidate.acoustic = None;
        candidate.basic_pitch = None;
        candidate.boundary_alternatives.clear();
        candidate.boundary_constraints.clear();
        candidate.technique_evidence.clear();
        candidate.techniques = Default::default();
        candidate.word_id = None;
        candidate.alternatives.clear();
        candidate.voicing_evidence = voicing_evidence(range, unsupported);
        candidates.push(candidate);
    }
    Ok(())
}

#[cfg(test)]
#[path = "voicing_tests.rs"]
mod tests;
