use crate::fusion::{CanonicalLyricUnit, CanonicalSingingTrack, CanonicalWordBoundary, TimeRange};

#[derive(Debug)]
pub(super) struct LyricDisplayGroup {
    pub boundary: CanonicalWordBoundary,
    pub timing_unresolved: bool,
    pub measured: bool,
}

/// A word with no measured timestamp stays visible beside a measured neighbour,
/// in original text order. Grouping text never makes its neighbour's timestamp
/// an independent measurement for the missing word.
pub(super) fn lyric_display_groups(track: &CanonicalSingingTrack) -> Vec<LyricDisplayGroup> {
    if track.lyric_units.is_empty() {
        return track.words.iter().cloned().map(|boundary| LyricDisplayGroup {
            boundary,
            timing_unresolved: false,
            measured: true,
        }).collect();
    }
    let mut output = Vec::new();
    let mut start = 0;
    while start < track.lyric_units.len() {
        let line = &track.lyric_units[start].line_id;
        let end = track.lyric_units[start..].iter()
            .position(|unit| &unit.line_id != line)
            .map_or(track.lyric_units.len(), |offset| start + offset);
        append_line_groups(track, &track.lyric_units[start..end], &mut output);
        start = end;
    }
    output
}

fn append_line_groups(
    track: &CanonicalSingingTrack,
    units: &[CanonicalLyricUnit],
    output: &mut Vec<LyricDisplayGroup>,
) {
    let line_start = output.len();
    let mut pending = String::new();
    let mut unresolved_prefix = false;
    for unit in units {
        if let Some(word) = unit.measured_range.and_then(|_| track.words.iter()
            .find(|word| word.word_id == unit.id)) {
            let mut boundary = word.clone();
            append_text(&mut pending, &unit.text);
            boundary.text = std::mem::take(&mut pending);
            output.push(LyricDisplayGroup {
                boundary,
                timing_unresolved: std::mem::take(&mut unresolved_prefix),
                measured: true,
            });
        } else if output.len() > line_start {
            let previous = output.last_mut().expect("this line has a measured word");
            append_text(&mut previous.boundary.text, &unit.text);
            previous.timing_unresolved = true;
        } else {
            append_text(&mut pending, &unit.text);
            unresolved_prefix = true;
        }
    }
    if output.len() > line_start || pending.is_empty() {
        return;
    }
    let first = &units[0];
    let line = first.line_id.as_deref().and_then(|id| track.transcript.tokens.iter()
        .find(|line| line.id.as_deref() == Some(id)));
    let scope = line.and_then(|line| line.range).unwrap_or_else(|| TimeRange {
        start: units.iter().map(|unit| unit.audition_range.start).min().unwrap_or(0),
        end: units.iter().map(|unit| unit.audition_range.end).max().unwrap_or(0),
    });
    output.push(LyricDisplayGroup {
        boundary: CanonicalWordBoundary {
            word_id: first.id.clone(),
            text: line.map_or(pending, |line| line.text.clone()),
            range: scope,
            confidence: None,
            disagreement: None,
            source_experts: track.transcript.source_experts.clone(),
            line_id: first.line_id.clone(),
        },
        timing_unresolved: true,
        measured: false,
    });
}

fn append_text(text: &mut String, next: &str) {
    if !text.is_empty()
        && super::vocal_chart::lyric_join_between(Some(text), next) == utz::LyricJoin::Space
    {
        text.push(' ');
    }
    text.push_str(next);
}

/// Limit an unresolved line's display scope to the available textual neighbours.
/// This is an audition placement, not a measured timestamp for its characters.
pub(super) fn lyric_placeholder_scope(groups: &[LyricDisplayGroup], index: usize) -> Option<TimeRange> {
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
