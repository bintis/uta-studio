//! Source-local note-expert onset observations for duration-path selection.
//!
//! A native note boundary is an observation even when its producer has no
//! calibrated confidence field. Without this term, every extra note pays the
//! state prior while a longer rival can erase repeated notes for free.
//! The utility is attenuated by independent pitch observations and timing,
//! and never treats duplicate pitch proposals as independent expert votes.

use std::collections::BTreeMap;

use super::{
    ATTACK_CONTEXT_TOLERANCE, BoundaryEvidenceKind, SegmentCandidate, TimeRange,
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

/// Candidate evidence is clipped to each duration. Reunite those pieces
/// before locating a recovery, so an internal source edge is never mistaken
/// for the end of an observed unsupported interval.
fn unsupported_ranges(candidates: &[SegmentCandidate]) -> Vec<TimeRange> {
    let mut ranges = candidates
        .iter()
        .flat_map(|candidate| candidate.voicing_evidence.iter())
        .flat_map(|evidence| evidence.unsupported_ranges.iter().copied())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|range| (range.start, range.end));
    let mut united: Vec<TimeRange> = Vec::new();
    for range in ranges {
        if let Some(previous) = united.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            united.push(range);
        }
    }
    united
}

fn unsupported_at(ranges: &[TimeRange], time: u64) -> Option<TimeRange> {
    let next = ranges.partition_point(|range| range.end <= time);
    ranges
        .get(next)
        .copied()
        .filter(|range| range.start <= time)
}

pub(super) struct NativeNoteEvents {
    credits: Vec<Vec<OnsetCredit>>,
}

impl NativeNoteEvents {
    pub(super) fn new(candidates: &[SegmentCandidate]) -> Self {
        let unsupported = unsupported_ranges(candidates);
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
            // Keep the native range on the candidate. For onset credit only,
            // an observation inside an unsupported interval refers to its
            // recovery. Several same-source observations at that recovery
            // remain one vote, even when their original starts differ.
            let onset = unsupported_at(&unsupported, candidate.range.start)
                .map_or(candidate.range.start, |range| range.end);
            observations
                .entry(&candidate.boundary_source)
                .or_default()
                .entry(onset)
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
                // A pitched candidate remains eligible in the graph, but
                // an onset inside explicitly unsupported time earns no
                // native event credit. A recovery proposal can receive the
                // original observation instead.
                if !candidate.target.is_pitched()
                    || unsupported_at(&unsupported, candidate.range.start).is_some()
                {
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

    fn rest(id: &str, start: u64, end: u64) -> SegmentCandidate {
        serde_json::from_value(serde_json::json!({
            "id": id,
            "range": { "start": start, "end": end },
            "target": { "kind": "unpitched" },
            "boundary_source": "continuous_voicing",
            "boundary_kind": "voicing",
            "boundary_role": "challenger",
            "target_pitch_source": "unpitched",
            "voicing_evidence": {
                "source_experts": ["rmvpe", "fcpe", "acoustic_dsp"],
                "unsupported_ranges": [{ "start": start, "end": end }]
            }
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

    #[test]
    fn a_partial_rest_cannot_repeat_an_onset_or_reward_an_unsupported_start() {
        let before = note("earlier-expert", 380_000, 400_000);
        let mut early = note("following-expert", 420_000, 1_000_000);
        early.voicing_evidence = rest("unsupported-prefix", 420_000, 460_000).voicing_evidence;
        let mut recovery = early.clone();
        recovery.id = "supported-recovery".into();
        recovery.range.start = 460_000;
        recovery.boundary_kind = BoundaryEvidenceKind::Voicing;
        recovery.voicing_evidence = None;
        let pool = vec![
            before,
            early,
            recovery,
            rest("whole-rest", 400_000, 460_000),
            rest("partial-rest", 400_000, 420_000),
        ];
        let index = NativeNoteEvents::new(&pool);
        assert!(index.reward(0) > 0.0);
        assert_eq!(index.reward(1), 0.0);
        assert!(index.reward(2) > 0.0);
        assert_eq!(index.repeated_reward(0, 2), 0.0);
        let selected = crate::fusion::decode_candidate_graph(&pool).unwrap();
        crate::fusion::validate_candidate_path(&pool, &selected).unwrap();
        let pitched = selected
            .iter()
            .filter(|candidate| candidate.target.is_pitched())
            .collect::<Vec<_>>();
        assert_eq!(pitched.len(), 2);
        assert_eq!(pitched[0].range, pool[0].range);
        assert_eq!(pitched[1].id, "supported-recovery");
        assert_eq!(pitched[1].range.start, 460_000);
        assert_eq!(
            selected
                .iter()
                .filter(|candidate| !candidate.target.is_pitched())
                .map(|candidate| candidate.range.end - candidate.range.start)
                .sum::<u64>(),
            60_000
        );
        assert_eq!(
            pool[1].range.start, 420_000,
            "the native observation stays intact"
        );
    }

    #[test]
    fn same_source_onsets_inside_one_gap_share_one_recovery_observation() {
        let mut first = note("singer-expert", 420_000, 1_000_000);
        first.voicing_evidence = rest("first-prefix", 420_000, 460_000).voicing_evidence;
        let mut second = note("singer-expert", 430_000, 1_000_000);
        second.voicing_evidence = rest("second-prefix", 430_000, 460_000).voicing_evidence;
        let mut recovery = note("derived", 460_000, 1_000_000);
        recovery.boundary_kind = BoundaryEvidenceKind::Voicing;
        let mut nearby = recovery.clone();
        nearby.id = "near-recovery".into();
        nearby.range.start = 480_000;
        let pool = vec![
            first,
            second,
            recovery,
            nearby,
            rest("rest-before-edge", 400_000, 420_000),
            rest("rest-after-edge", 420_000, 460_000),
        ];
        let index = NativeNoteEvents::new(&pool);
        assert_eq!(
            unsupported_ranges(&pool),
            [TimeRange {
                start: 400_000,
                end: 460_000
            }]
        );
        assert_eq!(index.reward(0), 0.0);
        assert_eq!(index.reward(1), 0.0);
        assert_eq!(index.reward(2), NATIVE_EVENT_UTILITY);
        assert!(index.reward(3) > 0.0);
        assert!(
            (index.reward(2) + index.reward(3)
                - index.repeated_reward(2, 3)
                - NATIVE_EVENT_UTILITY)
                .abs()
                < 0.000001
        );
    }
}
