//! Joint native onset/offset observations, independent of the selected pitch.
//!
//! An onset alone cannot prefer its measured note end over an arbitrary tail.
//! Each complete envelope contributes once per source and owns one midpoint, so
//! splitting a candidate cannot multiply that observation's reward.

use std::collections::BTreeMap;

use super::{
    ATTACK_CONTEXT_TOLERANCE, BoundaryEvidenceKind, SegmentCandidate, TimeRange,
    acoustic_fundamental_support_for_target, sustained_pitch_support_for_target,
};

// A decoder utility, not a calibrated confidence or a measurement tolerance.
const NATIVE_ENVELOPE_UTILITY: f32 = 0.6;

pub(super) struct NativeNoteEnvelopes {
    rewards: Vec<f32>,
}

impl NativeNoteEnvelopes {
    pub(super) fn new(candidates: &[SegmentCandidate]) -> Self {
        let mut observations = BTreeMap::<&str, BTreeMap<(u64, u64), f32>>::new();
        for candidate in candidates {
            if !candidate.target.is_pitched()
                || !matches!(
                    candidate.boundary_kind,
                    BoundaryEvidenceKind::Game | BoundaryEvidenceKind::AdvancedNote
                )
            {
                continue;
            }
            let Some(midi) = candidate.boundary_fractional_midi else {
                continue;
            };
            let target_hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
            let support = sustained_pitch_support_for_target(candidate, target_hz).max(
                acoustic_fundamental_support_for_target(candidate, target_hz),
            );
            observations
                .entry(&candidate.boundary_source)
                .or_default()
                .entry((candidate.range.start, candidate.range.end))
                .and_modify(|previous| *previous = previous.max(support))
                .or_insert(support);
        }
        let sources = observations
            .into_values()
            .map(|events| {
                events
                    .into_iter()
                    .map(|((start, end), support)| (TimeRange { start, end }, support))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let rewards = candidates
            .iter()
            .map(|candidate| {
                if !candidate.target.is_pitched() {
                    return 0.0;
                }
                sources
                    .iter()
                    .map(|events| {
                        let lower = candidate
                            .range
                            .start
                            .saturating_sub(ATTACK_CONTEXT_TOLERANCE);
                        let upper = candidate
                            .range
                            .start
                            .saturating_add(ATTACK_CONTEXT_TOLERANCE);
                        let first = events.partition_point(|(range, _)| range.start < lower);
                        let end = events.partition_point(|(range, _)| range.start <= upper);
                        events[first..end]
                            .iter()
                            .map(|(range, support)| {
                                let midpoint = range.start + (range.end - range.start) / 2;
                                if midpoint < candidate.range.start
                                    || midpoint >= candidate.range.end
                                {
                                    return 0.0;
                                }
                                let onset = range.start.abs_diff(candidate.range.start);
                                let offset = range.end.abs_diff(candidate.range.end);
                                let proximity = |distance: u64| {
                                    (1.0 - distance as f32 / ATTACK_CONTEXT_TOLERANCE as f32)
                                        .max(0.0)
                                };
                                support
                                    * proximity(onset)
                                    * proximity(offset)
                                    * NATIVE_ENVELOPE_UTILITY
                            })
                            .fold(0.0_f32, f32::max)
                    })
                    .sum()
            })
            .collect();
        Self { rewards }
    }

    pub(super) fn reward(&self, index: usize) -> f32 {
        self.rewards[index]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(source: &str, start: u64, end: u64) -> SegmentCandidate {
        serde_json::from_value(serde_json::json!({
            "id": format!("{source}-{start}"),
            "range": {"start": start, "end": end},
            "target": {"kind":"pitched", "midi":69, "center_hz":440.0},
            "boundary_source": source, "boundary_kind":"advanced_note", "boundary_role":"challenger",
            "boundary_fractional_midi":69.0, "target_pitch_source":source,
            "rmvpe_center_hz":440.0, "rmvpe_voiced_ratio":1.0, "rmvpe_pitch_mad_cents":0.0
        })).unwrap()
    }

    fn derived(start: u64, end: u64) -> SegmentCandidate {
        let mut candidate = note("derived", start, end);
        candidate.boundary_kind = BoundaryEvidenceKind::F0Transition;
        candidate.boundary_fractional_midi = None;
        candidate
    }

    #[test]
    fn matching_onset_cannot_hide_a_wrong_note_end() {
        let measured = note("expert", 100_000, 500_000);
        let exact = derived(100_000, 500_000);
        let late = derived(100_000, 700_000);
        let early = derived(100_000, 300_000);
        let scores = NativeNoteEnvelopes::new(&[measured, exact, late, early]);
        assert_eq!(scores.reward(0), NATIVE_ENVELOPE_UTILITY);
        assert_eq!(scores.reward(1), scores.reward(0));
        assert_eq!(scores.reward(2), 0.0);
        assert_eq!(scores.reward(3), 0.0);
    }

    #[test]
    fn pitch_proposals_and_duplicate_observations_do_not_multiply_evidence() {
        let measured = note("expert", 0, 500_000);
        let expected = NativeNoteEnvelopes::new(std::slice::from_ref(&measured)).reward(0);
        let mut alternative = measured.clone();
        alternative.target = crate::fusion::CandidateTarget::Pitched {
            midi: 81,
            center_hz: 880.0,
        };
        let scores = NativeNoteEnvelopes::new(&[measured.clone(), alternative, measured]);
        assert_eq!(scores.reward(0), expected);
        assert_eq!(scores.reward(1), expected);
    }

    #[test]
    fn a_short_envelope_has_only_one_midpoint_owner_even_when_both_edges_are_close() {
        let measured = note("expert", 0, 40_000);
        let prefix = derived(0, 20_000);
        let suffix = derived(20_000, 40_000);
        let scores = NativeNoteEnvelopes::new(&[measured, prefix, suffix]);
        assert_eq!(scores.reward(1), 0.0);
        assert!(scores.reward(2) > 0.0);
        assert!(scores.reward(1) + scores.reward(2) <= scores.reward(0));
    }

    #[test]
    fn unsupported_pitch_and_completion_shapes_are_not_native_envelopes() {
        let mut wrong = note("expert", 0, 500_000);
        wrong.boundary_fractional_midi = Some(81.0);
        let mut unpitched = wrong.clone();
        unpitched.target = crate::fusion::CandidateTarget::Unpitched;
        let completion = derived(0, 500_000);
        let scores = NativeNoteEnvelopes::new(&[wrong, unpitched, completion]);
        assert_eq!(scores.reward(0), 0.0);
        assert_eq!(scores.reward(1), 0.0);
        assert_eq!(scores.reward(2), 0.0);
    }

    #[test]
    fn distinct_sources_add_support_but_repeated_notes_keep_separate_envelopes() {
        let first = note("first-expert", 0, 500_000);
        let second = note("second-expert", 0, 500_000);
        let next = note("first-expert", 500_000, 1_000_000);
        let scores = NativeNoteEnvelopes::new(&[first, second, next]);
        assert_eq!(scores.reward(0), NATIVE_ENVELOPE_UTILITY * 2.0);
        assert_eq!(scores.reward(2), NATIVE_ENVELOPE_UTILITY);
    }
}
