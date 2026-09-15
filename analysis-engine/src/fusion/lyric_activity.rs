//! Musical note onset is not necessarily the first periodic vowel frame.
use super::{BoundaryConstraintKind, BoundaryEvidenceKind, SegmentCandidate};

const LEXICAL_ONSET_CONTEXT: u64 = 60_000;

/// Return a native note's consonantal-prefix duration only when its measured
/// lexical onset corroborates it and its observed pitched body is longer than
/// that prefix. A lyric alone cannot create a pitched event in silence. This
/// is boundary-local evidence, not a modification of either raw pitch curve.
pub(super) fn native_lexical_prefix_duration(candidate: &SegmentCandidate) -> u64 {
    if !candidate.target.is_pitched()
        || candidate.boundary_fractional_midi.is_none()
        || !matches!(candidate.boundary_kind, BoundaryEvidenceKind::Game | BoundaryEvidenceKind::AdvancedNote)
    {
        return 0;
    }
    let Some(prefix) = candidate.voicing_evidence.as_ref()
        .and_then(|evidence| evidence.unsupported_ranges.first())
        .filter(|range| range.start == candidate.range.start && range.end < candidate.range.end)
    else {
        return 0;
    };
    let duration = prefix.end - prefix.start;
    let body = candidate.continuous_pitch_observed_duration.unwrap_or(0);
    if body <= duration || candidate.rmvpe_voiced_ratio.is_none_or(|ratio| ratio <= 0.0)
        || candidate.fcpe_observed_ratio.is_none_or(|ratio| ratio <= 0.0)
    {
        return 0;
    }
    let lexical = candidate.boundary_constraints.iter().any(|constraint| {
        constraint.kind == BoundaryConstraintKind::WordStart
            && constraint.time.abs_diff(candidate.range.start) < LEXICAL_ONSET_CONTEXT
            && constraint.time < prefix.end
            && constraint.time.abs_diff(candidate.range.start) <= constraint.time.abs_diff(candidate.range.end)
    });
    if lexical { duration } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate() -> SegmentCandidate {
        serde_json::from_value(serde_json::json!({
            "id": "measured-note", "range": {"start": 100000, "end": 600000},
            "target": {"kind": "pitched", "midi": 60, "center_hz": 261.62555},
            "boundary_source": "note-expert", "boundary_kind": "advanced_note",
            "boundary_role": "challenger", "boundary_fractional_midi": 60.0,
            "target_pitch_source": "note-expert", "continuous_pitch_observed_duration": 400000,
            "rmvpe_voiced_ratio": 0.8, "fcpe_observed_ratio": 0.8,
            "voicing_evidence": {"source_experts": ["rmvpe", "fcpe", "acoustic_dsp"],
                "unsupported_ranges": [{"start": 100000, "end": 200000}]},
            "boundary_constraints": [{"source_expert": "forced_alignment", "kind": "word_start", "time": 100000, "depends_on": []}]
        })).unwrap()
    }

    #[test]
    fn corroborated_consonant_preserves_original_activity_evidence() {
        let note = candidate();
        let original = note.clone();
        assert_eq!(native_lexical_prefix_duration(&note), 100_000);
        assert_eq!(note, original);
    }

    #[test]
    fn word_ends_internal_gaps_and_unobserved_pitch_are_not_prefixes() {
        let mut note = candidate();
        note.boundary_constraints[0].kind = BoundaryConstraintKind::WordEnd;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
        note = candidate();
        note.voicing_evidence.as_mut().unwrap().unsupported_ranges[0].start = 300_000;
        note.voicing_evidence.as_mut().unwrap().unsupported_ranges[0].end = 400_000;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
        note = candidate();
        note.continuous_pitch_observed_duration = Some(50_000);
        assert_eq!(native_lexical_prefix_duration(&note), 0);
        note = candidate();
        note.fcpe_observed_ratio = None;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
    }

    #[test]
    fn lexical_only_candidates_cannot_gain_a_native_pitch_event() {
        let mut note = candidate();
        note.boundary_kind = BoundaryEvidenceKind::Alignment;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
        note = candidate();
        note.boundary_fractional_midi = None;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
        note = candidate();
        note.boundary_constraints[0].time = 500_000;
        assert_eq!(native_lexical_prefix_duration(&note), 0);
    }
}
