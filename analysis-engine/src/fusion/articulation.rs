//! Associate measured lexical attacks with existing native note events.
//!
//! A consonant can begin before periodic pitch. Its measured word onset may
//! refine a nearby note attack, but an extra word alone must not create a new
//! note. Original model envelopes remain available as competing candidates.
use std::collections::BTreeMap;

use super::{
    BoundaryAlternative, BoundaryEvidenceKind, BoundaryEvidenceSet, CanonicalWordBoundary,
    TimeRange,
};

// Cross-modal association horizon, not an evaluation tolerance or note-length
// restriction. Association is mutual nearest, so dense notes do not acquire
// several attacks from one word, or silently jump over their neighbors.
const ASSOCIATION_HORIZON: u64 = 200_000;
const SHARED_EDGE_TOLERANCE: u64 = 20_000;

fn nearest(times: &[u64], time: u64) -> Option<u64> {
    let next = times.partition_point(|candidate| *candidate < time);
    times[next.saturating_sub(1)..(next + 1).min(times.len())]
        .iter()
        .copied()
        .min_by_key(|candidate| (candidate.abs_diff(time), *candidate))
}

pub(super) fn onset_anchors(native: &[u64], words: &[u64]) -> BTreeMap<u64, u64> {
    let mut native = native.to_vec();
    native.sort_unstable();
    native.dedup();
    let mut words = words.to_vec();
    words.sort_unstable();
    words.dedup();
    native
        .iter()
        .filter_map(|time| {
            let word = nearest(&words, *time)?;
            (word.abs_diff(*time) <= ASSOCIATION_HORIZON && nearest(&native, word) == Some(*time))
                .then_some((*time, word))
        })
        .collect()
}

pub(super) fn boundary_challengers(
    boundaries: &BoundaryEvidenceSet,
    alternatives: &[BoundaryAlternative],
    words: &[CanonicalWordBoundary],
) -> Vec<BoundaryAlternative> {
    let word_starts = words
        .iter()
        .map(|word| word.range.start)
        .collect::<Vec<_>>();
    let mut sources = BTreeMap::<String, Vec<BoundaryAlternative>>::new();
    let primary = boundaries
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
        });
    for boundary in primary.chain(alternatives.iter().cloned()) {
        if matches!(
            boundary.kind,
            BoundaryEvidenceKind::Game | BoundaryEvidenceKind::AdvancedNote
        ) && boundary.fractional_midi.is_some()
            && !boundary.hard
        {
            sources
                .entry(boundary.source_expert.clone())
                .or_default()
                .push(boundary);
        }
    }
    let mut result = Vec::new();
    for notes in sources.into_values() {
        let mut onsets = notes
            .iter()
            .map(|note| note.range.start)
            .collect::<Vec<_>>();
        onsets.sort_unstable();
        onsets.dedup();
        let anchors = onset_anchors(&onsets, &word_starts);
        for note in notes {
            let start = anchors
                .get(&note.range.start)
                .copied()
                .unwrap_or(note.range.start);
            let end = nearest(&onsets, note.range.end)
                .filter(|time| {
                    *time > note.range.start
                        && time.abs_diff(note.range.end) <= SHARED_EDGE_TOLERANCE
                })
                .and_then(|time| anchors.get(&time))
                .copied()
                .unwrap_or(note.range.end);
            if end <= start || (start == note.range.start && end == note.range.end) {
                continue;
            }
            result.push(BoundaryAlternative {
                source_expert: format!("{}.articulation", note.source_expert),
                range: TimeRange { start, end },
                kind: BoundaryEvidenceKind::Alignment,
                fractional_midi: note.fractional_midi,
                // The duration is now jointly proposed, not an independent
                // calibrated prediction from the original note model.
                source_local_score: None,
                source_local_pitch_score: None,
                calibrated_boundary_confidence: None,
                calibrated_pitch_confidence: None,
                hard: false,
            });
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_without_a_native_attack_does_not_create_an_event() {
        assert_eq!(
            onset_anchors(&[100_000, 900_000], &[50_000, 350_000, 850_000]),
            BTreeMap::from([(100_000, 50_000), (900_000, 850_000)])
        );
    }

    #[test]
    fn matching_is_unique_ordered_and_insensitive_to_duplicate_pitch_proposals() {
        let native = [300_000, 100_000, 100_000, 200_000];
        let words = [180_000, 180_000, 280_000];
        let anchors = onset_anchors(&native, &words);
        assert_eq!(
            anchors,
            BTreeMap::from([(200_000, 180_000), (300_000, 280_000)])
        );
        assert_eq!(onset_anchors(&[], &words), BTreeMap::new());
        assert_eq!(onset_anchors(&native, &[]), BTreeMap::new());
    }

    #[test]
    fn distant_words_do_not_drag_notes_across_silence() {
        assert!(onset_anchors(&[1_000_000], &[0, 2_000_000]).is_empty());
        assert_eq!(
            onset_anchors(&[u64::MAX - 10], &[u64::MAX - 20]),
            BTreeMap::from([(u64::MAX - 10, u64::MAX - 20)])
        );
    }

    #[test]
    fn paired_envelopes_keep_shared_edges_and_original_evidence_unchanged() {
        let boundaries = BoundaryEvidenceSet {
            source_expert: "game".into(),
            kind: BoundaryEvidenceKind::Game,
            segments: vec![
                super::super::BoundarySegmentEvidence {
                    range: TimeRange {
                        start: 150_000,
                        end: 500_000,
                    },
                    fractional_midi: Some(60.0),
                    boundary_decision_parameter: None,
                    presence_decision_parameter: None,
                },
                super::super::BoundarySegmentEvidence {
                    range: TimeRange {
                        start: 500_000,
                        end: 900_000,
                    },
                    fractional_midi: Some(62.0),
                    boundary_decision_parameter: None,
                    presence_decision_parameter: None,
                },
            ],
            model_hash: None,
            runtime_identity: None,
        };
        let words = [100_000, 450_000].map(|start| CanonicalWordBoundary {
            word_id: start.to_string(),
            text: "a".into(),
            range: TimeRange {
                start,
                end: start + 100_000,
            },
            confidence: None,
            disagreement: None,
            source_experts: vec!["aligner".into()],
            line_id: None,
        });
        let before = boundaries.clone();
        let proposals = boundary_challengers(&boundaries, &[], &words);
        assert_eq!(proposals.len(), 2);
        assert_eq!(
            proposals[0].range,
            TimeRange {
                start: 100_000,
                end: 450_000
            }
        );
        assert_eq!(
            proposals[1].range,
            TimeRange {
                start: 450_000,
                end: 900_000
            }
        );
        assert_eq!(boundaries, before);
        assert!(
            proposals
                .iter()
                .all(|proposal| proposal.kind == BoundaryEvidenceKind::Alignment && !proposal.hard)
        );
    }
}
