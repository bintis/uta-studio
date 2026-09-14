use crate::fusion::{CanonicalSingingTrack, CanonicalWordBoundary, TimeRange};

#[derive(Debug)]
pub(super) struct LyricDisplayGroup {
    pub boundary: CanonicalWordBoundary,
    pub timing_unresolved: bool,
    pub measured: bool,
}

/// Every measured word keeps its independent time and text identity. Adjacent
/// unresolved units sharing one scope stay one display group, without borrowing
/// a measured neighbour's time. Canonical units retain their individual IDs.
pub(super) fn lyric_display_groups(track: &CanonicalSingingTrack) -> Vec<LyricDisplayGroup> {
    if track.lyric_units.is_empty() {
        return track
            .words
            .iter()
            .cloned()
            .map(|boundary| LyricDisplayGroup {
                boundary,
                timing_unresolved: false,
                measured: true,
            })
            .collect();
    }
    let mut output = Vec::<LyricDisplayGroup>::new();
    for unit in &track.lyric_units {
        let measured = unit.measured_range.is_some();
        let original = track.words.iter().find(|word| word.word_id == unit.id);
        if !measured {
            if let Some(previous) = output.last_mut().filter(|group| {
                !group.measured
                    && group.boundary.line_id == unit.line_id
                    && group.boundary.range == unit.audition_range
            }) {
                if super::vocal_chart::lyric_join_between(Some(&previous.boundary.text), &unit.text)
                    == utz::LyricJoin::Space
                {
                    previous.boundary.text.push(' ');
                }
                previous.boundary.text.push_str(&unit.text);
                continue;
            }
        }
        output.push(LyricDisplayGroup {
            boundary: CanonicalWordBoundary {
                word_id: unit.id.clone(),
                text: unit.text.clone(),
                range: unit.measured_range.unwrap_or(unit.audition_range),
                confidence: original.and_then(|word| word.confidence),
                disagreement: original.and_then(|word| word.disagreement),
                source_experts: original.map_or_else(
                    || track.transcript.source_experts.clone(),
                    |word| word.source_experts.clone(),
                ),
                line_id: unit.line_id.clone(),
            },
            timing_unresolved: !measured,
            measured,
        });
    }
    output
}

/// Limit an unresolved unit's display scope to available textual neighbours.
/// This remains an audition placement, not a measured character timestamp.
pub(super) fn lyric_placeholder_scope(
    groups: &[LyricDisplayGroup],
    index: usize,
) -> Option<TimeRange> {
    let group = &groups[index];
    if group.measured {
        return Some(group.boundary.range);
    }
    let mut scope = group.boundary.range;
    if let Some(previous) = groups[..index].iter().rev().find(|group| group.measured) {
        scope.start = scope.start.max(previous.boundary.range.end);
    }
    if let Some(next) = groups[index + 1..].iter().find(|group| group.measured) {
        scope.end = scope.end.min(next.boundary.range.start);
    }
    (scope.end > scope.start).then_some(scope)
}
