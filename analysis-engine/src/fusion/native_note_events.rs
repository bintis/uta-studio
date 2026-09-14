//! Source-local note-expert onset observations for duration-path selection.
//!
//! A native note boundary is an observation even when its producer has no
//! calibrated confidence field. Without this term, every extra note pays the
//! state prior while a longer rival can erase repeated notes for free.
//! The utility is attenuated by independent pitch observations and timing,
//! and never treats duplicate pitch proposals as independent expert votes.

use std::collections::BTreeMap;

use super::{
    ATTACK_CONTEXT_TOLERANCE, BoundaryEvidenceKind, SegmentCandidate,
    acoustic_fundamental_support_for_target, belongs_to_start, sustained_pitch_support_for_target,
};

/// A decoder utility scale chosen on the documented calibration recordings;
/// this is neither a source probability nor an acceptance threshold.
const NATIVE_EVENT_UTILITY: f32 = 0.6;

#[derive(Clone, Copy)]
struct OnsetObservation {
    time: u64,
    support: f32,
}

#[derive(Clone, Copy)]
struct OnsetCredit {
    source: usize,
    time: u64,
    reward: f32,
}

pub(super) struct NativeNoteEvents {
    credits: Vec<Vec<OnsetCredit>>,
}

impl NativeNoteEvents {
    pub(super) fn new(candidates: &[SegmentCandidate]) -> Self {
        let mut observations = BTreeMap::<&str, BTreeMap<u64, f32>>::new();
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
                // Completion fragments have geometry, but no native pitched
                // note observation. Their starts must not create extra votes.
                continue;
            };
            let target_hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
            let support = sustained_pitch_support_for_target(candidate, target_hz).max(
                acoustic_fundamental_support_for_target(candidate, target_hz),
            );
            observations
                .entry(&candidate.boundary_source)
                .or_default()
                .entry(candidate.range.start)
                .and_modify(|prior| *prior = prior.max(support))
                .or_insert(support);
        }
        let observations = observations
            .into_values()
            .map(|source| {
                source
                    .into_iter()
                    .map(|(time, support)| OnsetObservation { time, support })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let credits = candidates
            .iter()
            .map(|candidate| {
                if !candidate.target.is_pitched() {
                    return Vec::new();
                }
                observations
                    .iter()
                    .enumerate()
                    .filter_map(|(source, events)| {
                        let next =
                            events.partition_point(|event| event.time < candidate.range.start);
                        let nearest = events[next.saturating_sub(1)..(next + 1).min(events.len())]
                            .iter()
                            .min_by_key(|event| {
                                (event.time.abs_diff(candidate.range.start), event.time)
                            })?;
                        let distance = nearest.time.abs_diff(candidate.range.start);
                        if distance >= ATTACK_CONTEXT_TOLERANCE
                            || !belongs_to_start(candidate, nearest.time)
                        {
                            return None;
                        }
                        let localization = 1.0 - distance as f32 / ATTACK_CONTEXT_TOLERANCE as f32;
                        Some(OnsetCredit {
                            source,
                            time: nearest.time,
                            reward: NATIVE_EVENT_UTILITY * nearest.support * localization,
                        })
                    })
                    .collect()
            })
            .collect();
        Self { credits }
    }

    pub(super) fn reward(&self, index: usize) -> f32 {
        self.credits[index].iter().map(|event| event.reward).sum()
    }

    /// Nearby states can refer to the same physical expert onset. Retain the
    /// larger local contribution rather than paying that observation twice.
    pub(super) fn repeated_reward(&self, previous: usize, next: usize) -> f32 {
        self.credits[previous]
            .iter()
            .filter_map(|prior| {
                self.credits[next]
                    .iter()
                    .find(|event| event.source == prior.source && event.time == prior.time)
                    .map(|event| event.reward.min(prior.reward))
            })
            .sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(source: &str, start: u64, end: u64) -> SegmentCandidate {
        serde_json::from_value(serde_json::json!({
            "id": format!("{source}-{start}"),
            "range": { "start": start, "end": end },
            "target": { "kind": "pitched", "midi": 69, "center_hz": 440.0 },
            "boundary_source": source,
            "boundary_kind": "advanced_note",
            "boundary_role": "challenger",
            "boundary_fractional_midi": 69.0,
            "target_pitch_source": source,
            "rmvpe_center_hz": 440.0,
            "rmvpe_voiced_ratio": 1.0,
            "rmvpe_pitch_mad_cents": 0.0
        }))
        .unwrap()
    }

    #[test]
    fn expert_identity_and_duplicate_pitch_proposals_do_not_change_onset_support() {
        let original = note("singer-expert", 0, 500_000);
        let original_reward = NativeNoteEvents::new(std::slice::from_ref(&original)).reward(0);
        let mut renamed = original.clone();
        renamed.boundary_source = "other-expert".to_string();
        renamed.target_pitch_source = "other-expert".to_string();
        assert_eq!(
            NativeNoteEvents::new(std::slice::from_ref(&renamed)).reward(0),
            original_reward
        );
        let mut alternative = original.clone();
        alternative.id = "pitch-alternative".to_string();
        alternative.target = crate::fusion::CandidateTarget::Pitched {
            midi: 81,
            center_hz: 880.0,
        };
        let index = NativeNoteEvents::new(&[original.clone(), alternative, original]);
        assert_eq!(index.reward(0), original_reward);
        assert_eq!(index.reward(1), original_reward);
    }

    #[test]
    fn disagreement_with_independent_pitch_attenuates_the_native_event() {
        let supported = note("singer-expert", 0, 500_000);
        let mut contradicted = supported.clone();
        contradicted.boundary_fractional_midi = Some(81.0);
        assert!(NativeNoteEvents::new(&[supported]).reward(0) > 0.0);
        assert_eq!(NativeNoteEvents::new(&[contradicted]).reward(0), 0.0);
    }

    #[test]
    fn completion_fragments_and_derived_splits_are_not_native_observations() {
        let mut completion = note("singer-expert", 0, 500_000);
        completion.boundary_fractional_midi = None;
        let mut derived = completion.clone();
        derived.boundary_fractional_midi = Some(69.0);
        derived.boundary_kind = BoundaryEvidenceKind::BasicPitchOnset;
        let index = NativeNoteEvents::new(&[completion, derived]);
        assert_eq!(index.reward(0), 0.0);
        assert_eq!(index.reward(1), 0.0);
    }

    #[test]
    fn nearby_candidates_share_one_event_without_multiplying_its_reward() {
        let measured = note("singer-expert", 0, 500_000);
        let mut prefix = note("derived", 0, 30_000);
        let mut suffix = note("derived", 30_000, 500_000);
        prefix.boundary_kind = BoundaryEvidenceKind::F0Transition;
        suffix.boundary_kind = BoundaryEvidenceKind::F0Transition;
        let index = NativeNoteEvents::new(&[measured, prefix, suffix]);
        assert!(index.reward(1) > index.reward(2));
        assert!(index.reward(2) > 0.0);
        assert_eq!(
            index.reward(1) + index.reward(2) - index.repeated_reward(1, 2),
            index.reward(1)
        );
    }

    #[test]
    fn a_different_measured_reattack_remains_a_separate_observation() {
        let before = note("singer-expert", 0, 500_000);
        let after = note("singer-expert", 500_000, 1_000_000);
        let index = NativeNoteEvents::new(&[before, after]);
        assert!(index.reward(0) > 0.0);
        assert!(index.reward(1) > 0.0);
        assert_eq!(index.repeated_reward(0, 1), 0.0);
    }

    #[test]
    fn an_unpitched_state_cannot_supply_or_receive_a_native_onset_vote() {
        let pitched = note("singer-expert", 0, 500_000);
        let expected = NativeNoteEvents::new(std::slice::from_ref(&pitched)).reward(0);
        let mut rest = pitched.clone();
        rest.id = "unpitched".into();
        rest.target = crate::fusion::CandidateTarget::Unpitched;
        rest.boundary_kind = BoundaryEvidenceKind::Voicing;
        rest.boundary_fractional_midi = None;
        let index = NativeNoteEvents::new(&[pitched, rest]);
        assert_eq!(index.reward(0), expected);
        assert_eq!(index.reward(1), 0.0);
    }
}
