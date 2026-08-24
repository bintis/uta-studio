//! Deterministic note-timing quantization applied after semantic note inference.
//!
//! The quantizer intentionally mutates only semantic note ranges. Continuous F0
//! and pitch-bend samples retain their original canonical timestamps so evidence
//! never becomes grid-snapped or reinterpreted as target-note geometry.
//!
//! `rhythm-grid-dp-v1` interprets BPM as quarter-note beats per minute and
//! anchors the selected subdivision to canonical time zero. It chooses a
//! globally non-overlapping minimum-cost path, resolves exact cost ties toward
//! the earlier range, requires one whole grid step of duration, preserves every
//! positive rest, confines notes to source bounds, and never moves an endpoint
//! across (or away from) an exact caller-owned hard boundary.

use serde::{Deserialize, Serialize};

use crate::contract::{
    CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult, MusicalContextV1,
    QuantizationGridV1,
};
use crate::fusion::{CanonicalSingingTrack, TimeRange, validate_canonical_singing_track};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuantizationReportV1 {
    pub algorithm: String,
    pub bpm: f64,
    pub grid: QuantizationGridV1,
    pub grid_step: u64,
    pub minimum_note_duration: u64,
    pub source_start: u64,
    pub source_end: u64,
    pub hard_boundary_count: usize,
    pub note_count: usize,
    pub adjusted_notes: usize,
    pub maximum_shift: u64,
}

impl QuantizationReportV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.algorithm != crate::fingerprint::QUANTIZATION_VERSION
            || !self.bpm.is_finite()
            || self.bpm <= 0.0
            || self.bpm > 1_000.0
            || self.grid_step == 0
            || grid_step(self.bpm, self.grid)? != self.grid_step
            || self.minimum_note_duration != self.grid_step
            || self.source_end <= self.source_start
            || self.adjusted_notes > self.note_count
            || self.maximum_shift > self.grid_step
        {
            return Err(invalid_output("quantization report is invalid"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy)]
struct TimingCandidate {
    range: TimeRange,
    local_cost: f64,
}

#[derive(Debug, Clone, Copy)]
struct DynamicState {
    cost: f64,
    previous: Option<usize>,
}

pub fn quantize_singing_track(
    track: &mut CanonicalSingingTrack,
    context: &MusicalContextV1,
    source_range: TimeRange,
    hard_boundaries: &[TimeRange],
) -> EngineResult<QuantizationReportV1> {
    validate_canonical_singing_track(track).map_err(invalid_output)?;
    if source_range.end <= source_range.start {
        return Err(invalid_output("quantization source bounds are invalid"));
    }
    let bpm = context.bpm.ok_or_else(missing_context)?;
    let grid = context.quantization_grid.ok_or_else(missing_context)?;
    if !bpm.is_finite() || bpm <= 0.0 {
        return Err(missing_context());
    }
    let step = grid_step(bpm, grid)?;
    let original_ranges = track
        .notes
        .iter()
        .map(|note| note.range)
        .collect::<Vec<_>>();
    if original_ranges
        .iter()
        .any(|range| range.start < source_range.start || range.end > source_range.end)
    {
        return Err(invalid_output(
            "candidate note timing escapes the authorized source timeline",
        ));
    }
    let mut hard_edges = hard_boundaries
        .iter()
        .flat_map(|range| [range.start, range.end])
        .collect::<Vec<_>>();
    hard_edges.sort_unstable();
    hard_edges.dedup();
    if original_ranges.is_empty() {
        let report = QuantizationReportV1 {
            algorithm: crate::fingerprint::QUANTIZATION_VERSION.to_string(),
            bpm,
            grid,
            grid_step: step,
            minimum_note_duration: step,
            source_start: source_range.start,
            source_end: source_range.end,
            hard_boundary_count: hard_edges.len(),
            note_count: 0,
            adjusted_notes: 0,
            maximum_shift: 0,
        };
        report.validate()?;
        return Ok(report);
    }

    let candidates = track
        .notes
        .iter()
        .map(|note| {
            timing_candidates(
                note.range,
                note.confidence,
                note.uncertain,
                step,
                source_range,
                &hard_edges,
            )
        })
        .collect::<EngineResult<Vec<_>>>()?;

    let mut states = candidates
        .iter()
        .map(|items| {
            items
                .iter()
                .map(|candidate| DynamicState {
                    cost: candidate.local_cost,
                    previous: None,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    for note_index in 1..candidates.len() {
        let original_gap = original_ranges[note_index]
            .start
            .saturating_sub(original_ranges[note_index - 1].end);
        for current_index in 0..candidates[note_index].len() {
            let current = candidates[note_index][current_index];
            let mut best = None;
            for previous_index in 0..candidates[note_index - 1].len() {
                let previous = candidates[note_index - 1][previous_index];
                if previous.range.end > current.range.start {
                    continue;
                }
                let quantized_gap = current.range.start - previous.range.end;
                if original_gap > 0 && quantized_gap == 0 {
                    continue;
                }
                let transition = normalized_distance(quantized_gap, original_gap, step) * 0.25;
                let cost =
                    states[note_index - 1][previous_index].cost + current.local_cost + transition;
                if best.is_none_or(|(best_cost, _)| cost < best_cost) {
                    best = Some((cost, previous_index));
                }
            }
            let Some((cost, previous)) = best else {
                states[note_index][current_index].cost = f64::INFINITY;
                continue;
            };
            states[note_index][current_index] = DynamicState {
                cost,
                previous: Some(previous),
            };
        }
    }

    let last_index = states.len() - 1;
    let mut selected = states[last_index]
        .iter()
        .enumerate()
        .filter(|(_, state)| state.cost.is_finite())
        .min_by(|(left_index, left), (right_index, right)| {
            left.cost.total_cmp(&right.cost).then_with(|| {
                let left = candidates[last_index][*left_index].range;
                let right = candidates[last_index][*right_index].range;
                (left.start, left.end).cmp(&(right.start, right.end))
            })
        })
        .map(|(index, _)| index)
        .ok_or_else(|| {
            invalid_output("quantization could not produce non-overlapping note geometry")
        })?;
    let mut selected_ranges = vec![TimeRange { start: 0, end: 1 }; candidates.len()];
    for note_index in (0..candidates.len()).rev() {
        selected_ranges[note_index] = candidates[note_index][selected].range;
        if note_index > 0 {
            selected = states[note_index][selected].previous.ok_or_else(|| {
                invalid_output("quantization backtracking lost a timing predecessor")
            })?;
        }
    }

    let mut adjusted_notes = 0;
    let mut maximum_shift = 0;
    for ((note, original), quantized) in track
        .notes
        .iter_mut()
        .zip(&original_ranges)
        .zip(selected_ranges)
    {
        if *original != quantized {
            adjusted_notes += 1;
        }
        maximum_shift = maximum_shift
            .max(original.start.abs_diff(quantized.start))
            .max(original.end.abs_diff(quantized.end));
        note.range = quantized;
    }
    validate_canonical_singing_track(track).map_err(invalid_output)?;

    let report = QuantizationReportV1 {
        algorithm: crate::fingerprint::QUANTIZATION_VERSION.to_string(),
        bpm,
        grid,
        grid_step: step,
        minimum_note_duration: step,
        source_start: source_range.start,
        source_end: source_range.end,
        hard_boundary_count: hard_edges.len(),
        note_count: original_ranges.len(),
        adjusted_notes,
        maximum_shift,
    };
    report.validate()?;
    Ok(report)
}

fn grid_step(bpm: f64, grid: QuantizationGridV1) -> EngineResult<u64> {
    let units_per_beat = f64::from(CANONICAL_TIMEBASE) * 60.0 / bpm;
    let units = units_per_beat / f64::from(grid.steps_per_beat());
    if !units.is_finite() || units < 1.0 || units > u64::MAX as f64 {
        return Err(missing_context());
    }
    Ok(units.round() as u64)
}

fn timing_candidates(
    original: TimeRange,
    confidence: Option<f32>,
    uncertain: bool,
    step: u64,
    source_range: TimeRange,
    hard_edges: &[u64],
) -> EngineResult<Vec<TimingCandidate>> {
    let starts = grid_neighbors(original.start, step)?;
    let ends = grid_neighbors(original.end, step)?;
    let confidence = f64::from(confidence.unwrap_or(0.5));
    let evidence_weight = (0.75 + confidence * 1.5) * if uncertain { 0.75 } else { 1.0 };
    let original_duration = original.end - original.start;
    let mut candidates = Vec::new();
    for start in starts {
        for end in &ends {
            if end.saturating_sub(start) < step
                || start < source_range.start
                || *end > source_range.end
                || start.abs_diff(original.start) > step
                || end.abs_diff(original.end) > step
                || hard_edges.iter().any(|edge| {
                    crosses_hard_edge(original.start, start, *edge)
                        || crosses_hard_edge(original.end, *end, *edge)
                })
            {
                continue;
            }
            let range = TimeRange { start, end: *end };
            let movement = normalized_distance(start, original.start, step)
                + normalized_distance(*end, original.end, step);
            let duration = normalized_distance(*end - start, original_duration, step);
            candidates.push(TimingCandidate {
                range,
                local_cost: movement * evidence_weight + duration * 0.5,
            });
        }
    }
    candidates.sort_by(|left, right| {
        left.local_cost
            .total_cmp(&right.local_cost)
            .then_with(|| left.range.start.cmp(&right.range.start))
            .then_with(|| left.range.end.cmp(&right.range.end))
    });
    candidates.dedup_by_key(|candidate| (candidate.range.start, candidate.range.end));
    if candidates.is_empty() {
        return Err(invalid_output(
            "quantization grid cannot satisfy a note's word boundary and duration constraints",
        ));
    }
    Ok(candidates)
}

fn crosses_hard_edge(original: u64, candidate: u64, edge: u64) -> bool {
    if original == edge {
        candidate != edge
    } else {
        (original < edge) != (candidate < edge)
    }
}

fn grid_neighbors(value: u64, step: u64) -> EngineResult<Vec<u64>> {
    let lower_index = value / step;
    let rounded_index = value
        .checked_add(step / 2)
        .ok_or_else(|| invalid_output("quantization grid arithmetic overflowed"))?
        / step;
    let mut indices = vec![lower_index, rounded_index, rounded_index.saturating_add(1)];
    if lower_index > 0 {
        indices.push(lower_index - 1);
    }
    indices.sort_unstable();
    indices.dedup();
    let mut values = indices
        .into_iter()
        .map(|index| {
            index
                .checked_mul(step)
                .ok_or_else(|| invalid_output("quantization grid arithmetic overflowed"))
        })
        .collect::<EngineResult<Vec<_>>>()?;
    values.sort_unstable();
    values.dedup();
    Ok(values)
}

fn normalized_distance(left: u64, right: u64, step: u64) -> f64 {
    left.abs_diff(right) as f64 / step as f64
}

fn missing_context() -> EngineError {
    EngineError::new(
        EngineErrorCode::MissingRequiredInput,
        "rhythm quantization requires explicit finite BPM and quantization grid",
    )
    .with_capability("rhythm.quantize")
}

fn invalid_output(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
        .with_capability("rhythm.quantize")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{ContextAuthority, TimeSignatureV1};
    use crate::fusion::{
        CanonicalLyrics, CanonicalNote, CanonicalNoteEvidence, CanonicalWordBoundary, F0Point,
        HarmonyMetadata, LyricsAuthority, PitchBendPoint, TechniqueScores, TranscriptTokenEvidence,
    };

    fn context() -> MusicalContextV1 {
        MusicalContextV1 {
            bpm: Some(120.0),
            key: None,
            time_signature: Some(TimeSignatureV1 { beats: 4, unit: 4 }),
            quantization_grid: Some(QuantizationGridV1::Sixteenth),
            authority: ContextAuthority::Hint,
        }
    }

    fn note(id: &str, start: u64, end: u64, word_id: &str) -> CanonicalNote {
        CanonicalNote {
            id: id.to_string(),
            range: TimeRange { start, end },
            midi_note: 69,
            center_pitch_hz: 440.0,
            center_offset_cents: 0.0,
            confidence: Some(0.8),
            uncertain: false,
            alternatives: Vec::new(),
            f0_curve: vec![F0Point {
                time: start + 10_000,
                hz: 440.0,
                confidence: Some(0.9),
            }],
            pitch_bend: vec![PitchBendPoint {
                time: start + 10_000,
                cents: 0.0,
            }],
            techniques: TechniqueScores::default(),
            word_id: Some(word_id.to_string()),
            evidence: CanonicalNoteEvidence {
                source_experts: vec!["game".to_string()],
                game_fractional_midi: 69.0,
                game_boundary_decision_threshold: 0.2,
                game_presence_decision_threshold: 0.2,
                rmvpe_center_hz: Some(440.0),
                rmvpe_confidence: Some(0.9),
                rmvpe_cents_difference: Some(0.0),
                rmvpe_voiced_ratio: Some(1.0),
                rmvpe_pitch_mad_cents: Some(0.0),
                fcpe_center_hz: None,
                fcpe_observed_ratio: None,
                fcpe_pitch_mad_cents: None,
                fcpe_cents_from_rmvpe: None,
                fcpe_supports_rmvpe: None,
                acoustic: None,
            },
        }
    }

    fn track() -> CanonicalSingingTrack {
        CanonicalSingingTrack {
            schema_version: 1,
            transcript: CanonicalLyrics {
                text: "sing now".to_string(),
                language: Some("en".to_string()),
                authority: LyricsAuthority::Generated,
                tokens: vec![TranscriptTokenEvidence {
                    id: Some("word-1".to_string()),
                    text: "sing".to_string(),
                    range: None,
                    confidence: Some(0.9),
                }],
                confidence: Some(0.9),
                source_experts: vec!["qwen".to_string()],
                alternatives: Vec::new(),
            },
            words: vec![
                CanonicalWordBoundary {
                    word_id: "word-1".to_string(),
                    text: "sing".to_string(),
                    range: TimeRange {
                        start: 0,
                        end: 500_000,
                    },
                    confidence: Some(0.9),
                    disagreement: None,
                    source_experts: vec!["align".to_string()],
                },
                CanonicalWordBoundary {
                    word_id: "word-2".to_string(),
                    text: "now".to_string(),
                    range: TimeRange {
                        start: 500_000,
                        end: 1_000_000,
                    },
                    confidence: Some(0.9),
                    disagreement: None,
                    source_experts: vec!["align".to_string()],
                },
            ],
            notes: vec![
                note("note-1", 113_000, 368_000, "word-1"),
                note("note-2", 512_000, 887_000, "word-2"),
            ],
            f0_curve: vec![
                F0Point {
                    time: 123_000,
                    hz: 440.0,
                    confidence: Some(0.9),
                },
                F0Point {
                    time: 522_000,
                    hz: 493.88,
                    confidence: Some(0.9),
                },
            ],
            harmony_metadata: HarmonyMetadata::default(),
            provenance: Vec::new(),
        }
    }

    #[test]
    fn quantization_is_deterministic_and_preserves_continuous_timestamps() {
        let mut first = track();
        let mut second = first.clone();
        let note_f0 = first.notes[0].f0_curve.clone();
        let pitch_bend = first.notes[0].pitch_bend.clone();
        let global_f0 = first.f0_curve.clone();

        let bounds = TimeRange::new(0, 1_000_000).unwrap();
        let first_report = quantize_singing_track(&mut first, &context(), bounds, &[]).unwrap();
        let second_report = quantize_singing_track(&mut second, &context(), bounds, &[]).unwrap();

        assert_eq!(first, second);
        assert_eq!(first_report, second_report);
        assert_eq!(first.notes[0].f0_curve, note_f0);
        assert_eq!(first.notes[0].pitch_bend, pitch_bend);
        assert_eq!(first.f0_curve, global_f0);
        assert!(
            first
                .notes
                .iter()
                .all(|note| note.range.start % first_report.grid_step == 0
                    && note.range.end % first_report.grid_step == 0)
        );
        assert!(
            first
                .notes
                .windows(2)
                .all(|pair| pair[0].range.end <= pair[1].range.start)
        );
    }

    #[test]
    fn missing_bpm_fails_without_mutating_track() {
        let mut candidate = track();
        let original = candidate.clone();
        let mut context = context();
        context.bpm = None;
        let error = quantize_singing_track(
            &mut candidate,
            &context,
            TimeRange::new(0, 1_000_000).unwrap(),
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::MissingRequiredInput);
        assert_eq!(candidate, original);
    }

    #[test]
    fn hard_edges_source_bounds_minimum_duration_and_positive_rests_are_preserved() {
        let mut candidate = track();
        candidate.notes[0].range = TimeRange::new(62_500, 312_500).unwrap();
        candidate.notes[1].range = TimeRange::new(500_001, 999_000).unwrap();
        let first_f0 = candidate.notes[0].f0_curve.clone();
        let hard = [TimeRange::new(0, 500_000).unwrap()];
        let report = quantize_singing_track(
            &mut candidate,
            &context(),
            TimeRange::new(0, 1_000_000).unwrap(),
            &hard,
        )
        .unwrap();
        assert_eq!(report.grid_step, 125_000);
        assert_eq!(report.minimum_note_duration, report.grid_step);
        assert_eq!(report.hard_boundary_count, 2);
        // Exact half-grid ties choose the earlier valid range.
        assert_eq!(candidate.notes[0].range.start, 0);
        assert_eq!(candidate.notes[1].range.start, 500_000);
        assert_eq!(candidate.notes[1].range.end, 1_000_000);
        assert!(candidate.notes[0].range.end < candidate.notes[1].range.start);
        assert!(candidate.notes.iter().all(|note| {
            note.range.end - note.range.start >= report.minimum_note_duration
                && note.range.start >= report.source_start
                && note.range.end <= report.source_end
        }));
        assert_eq!(candidate.notes[0].f0_curve, first_f0);
    }

    #[test]
    fn exact_non_grid_hard_boundary_fails_without_mutation() {
        let mut candidate = track();
        candidate.notes[0].range.start = 113_000;
        let original = candidate.clone();
        let hard = [TimeRange::new(113_000, 500_000).unwrap()];
        let error = quantize_singing_track(
            &mut candidate,
            &context(),
            TimeRange::new(0, 1_000_000).unwrap(),
            &hard,
        )
        .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::OutputValidationFailed);
        assert_eq!(candidate, original);
    }

    #[test]
    fn source_escape_fails_without_mutation() {
        let mut candidate = track();
        candidate.notes[1].range.end = 1_000_001;
        let original = candidate.clone();
        let error = quantize_singing_track(
            &mut candidate,
            &context(),
            TimeRange::new(0, 1_000_000).unwrap(),
            &[],
        )
        .unwrap_err();
        assert_eq!(error.code, EngineErrorCode::OutputValidationFailed);
        assert_eq!(candidate, original);
    }
}
