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
        if !measured
            && let Some(previous) = output.last_mut().filter(|group| {
                !group.measured
                    && group.boundary.line_id == unit.line_id
                    && group.boundary.range == unit.audition_range
            })
        {
            if super::vocal_chart::lyric_join_between(Some(&previous.boundary.text), &unit.text)
                == utz::LyricJoin::Space
            {
                previous.boundary.text.push(' ');
            }
            previous.boundary.text.push_str(&unit.text);
            continue;
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

/// Project unresolved search scopes in transcript order, bounded by measured
/// neighbours. If those neighbours leave no room, retain an unresolved point
/// marker at their boundary; never fall back to the beginning of the line or
/// manufacture a positive-duration word. Measured intervals remain unchanged.
/// Both passes are linear, including long runs of unresolved units.
pub(super) fn lyric_display_scopes(groups: &[LyricDisplayGroup]) -> Vec<TimeRange> {
    let mut next_starts = vec![None; groups.len()];
    let mut next_start = None;
    for (index, group) in groups.iter().enumerate().rev() {
        next_starts[index] = next_start;
        if group.measured {
            next_start = Some(group.boundary.range.start);
        }
    }
    let mut previous_start = 0;
    let mut previous_end = None;
    groups
        .iter()
        .zip(next_starts)
        .map(|(group, next_start)| {
            let range = if group.measured {
                previous_end = Some(group.boundary.range.end);
                group.boundary.range
            } else {
                let upper = next_start.unwrap_or(u64::MAX);
                let lower = previous_end.unwrap_or(0).max(previous_start).min(upper);
                let start = group.boundary.range.start.clamp(lower, upper);
                let end = group.boundary.range.end.clamp(start, upper);
                TimeRange { start, end }
            };
            previous_start = range.start;
            range
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(text: &str, start: u64, end: u64, measured: bool) -> LyricDisplayGroup {
        LyricDisplayGroup {
            boundary: CanonicalWordBoundary {
                word_id: text.into(),
                text: text.into(),
                range: TimeRange { start, end },
                confidence: None,
                disagreement: None,
                source_experts: Vec::new(),
                line_id: None,
            },
            timing_unresolved: !measured,
            measured,
        }
    }

    #[test]
    fn collapsed_missing_word_is_a_point_not_the_entire_line() {
        let groups = [
            group("空", 41_310_000, 41_550_000, true),
            group("を", 42_110_000, 42_670_000, true),
            group("覆う", 41_310_000, 48_180_000, false),
            group("黒い", 42_670_000, 43_870_000, true),
        ];
        let ranges = lyric_display_scopes(&groups);
        assert_eq!(
            ranges[2],
            TimeRange {
                start: 42_670_000,
                end: 42_670_000
            }
        );
        assert!(ranges.windows(2).all(|pair| pair[0].start <= pair[1].start));
        for (group, range) in groups.iter().zip(ranges) {
            if group.measured {
                assert_eq!(range, group.boundary.range);
            }
        }
        assert_eq!(groups[2].boundary.range.start, 41_310_000);
        assert!(groups[2].timing_unresolved);
    }

    #[test]
    fn missing_words_keep_available_gaps_without_evenly_dividing_them() {
        let groups = [
            group("一", 100, 200, true),
            group("二", 0, 1000, false),
            group("三", 400, 500, true),
        ];
        assert_eq!(
            lyric_display_scopes(&groups)[1],
            TimeRange {
                start: 200,
                end: 400
            }
        );
    }

    #[test]
    fn reversed_search_scopes_do_not_reverse_unresolved_text() {
        let groups = [
            group("first", 100, 200, true),
            group("missing", 800, 900, false),
            group("also missing", 0, 100, false),
            group("last", 900, 1000, true),
        ];
        let ranges = lyric_display_scopes(&groups);
        assert_eq!(
            ranges[2],
            TimeRange {
                start: 800,
                end: 800
            }
        );
        assert!(ranges.windows(2).all(|pair| pair[0].start <= pair[1].start));
    }

    #[test]
    fn leading_trailing_and_overlapping_anchor_scopes_never_invent_duration() {
        let groups = [
            group("before", 300, 600, false),
            group("first", 100, 400, true),
            group("between", 0, 1000, false),
            group("second", 300, 500, true),
            group("after", 0, 100, false),
        ];
        let ranges = lyric_display_scopes(&groups);
        assert_eq!(
            ranges[0],
            TimeRange {
                start: 100,
                end: 100
            }
        );
        assert_eq!(
            ranges[2],
            TimeRange {
                start: 300,
                end: 300
            }
        );
        assert_eq!(
            ranges[4],
            TimeRange {
                start: 500,
                end: 500
            }
        );
        assert!(ranges.windows(2).all(|pair| pair[0].start <= pair[1].start));
    }
}
