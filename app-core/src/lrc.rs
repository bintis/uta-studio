//! Parser for Standard LRC (line-level timestamps) and Enhanced LRC
//! (word-level timestamps) into Uta! Studio's editable transcript shape.
//!
//! Standard:  `[00:12.00]First line of lyrics`
//! Enhanced:  `[00:12.00]<00:12.00>I <00:12.30>see <00:12.60>trees`
//!
//! Word-level lines produce one token per `<mm:ss.xx>` tag with exact timing.
//! Line-level lines produce a single token spanning the whole line, so the
//! renderer highlights the line as a unit.

use serde::{Deserialize, Serialize};

/// Fallback duration (seconds) for the final segment, whose end cannot be
/// derived from a following line.
const LAST_SEGMENT_SECS: f64 = 4.0;

#[derive(Debug, Clone, Serialize)]
pub struct LrcWord {
    pub word: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct LrcSegment {
    pub text: String,
    pub start: f64,
    pub end: f64,
    pub words: Vec<LrcWord>,
}

/// A supplied line window, not a measured word or note interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LrcInputRange {
    pub start: f64,
    pub end: f64,
}

/// Complete canonical input, including body lines with no supplied timing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LrcInputLine {
    pub text: String,
    pub range: Option<LrcInputRange>,
}

#[derive(Debug, Clone)]
pub struct ParsedLrc {
    /// Pure timed LRC is expanded in timestamp order. Mixed input retains
    /// source order so untimed lines stay beside their original neighbours.
    pub input_lines: Vec<LrcInputLine>,
    /// Only timed lines can be projected onto the display timeline.
    pub segments: Vec<LrcSegment>,
    final_end_inferred: bool,
}

impl ParsedLrc {
    /// Analysis knows the source duration, so an open-ended final LRC line can
    /// retain sung material past the renderer's four-second display fallback.
    /// An explicit trailing empty timestamp is not inferred and is preserved.
    pub fn extend_inferred_final_end(&mut self, source_end: f64) {
        if !self.final_end_inferred || !source_end.is_finite() {
            return;
        }
        let Some(final_start) = self.segments.last().map(|segment| segment.start) else {
            return;
        };
        for segment in self
            .segments
            .iter_mut()
            .rev()
            .take_while(|segment| segment.start == final_start)
        {
            if source_end <= segment.end {
                continue;
            }
            let previous_range = LrcInputRange {
                start: segment.start,
                end: segment.end,
            };
            segment.end = source_end;
            if let Some(word) = segment.words.last_mut()
                && (word.end - previous_range.end).abs() <= f64::EPSILON
            {
                word.end = source_end;
            }
            for line in &mut self.input_lines {
                if line.range == Some(previous_range) {
                    line.range = Some(LrcInputRange {
                        start: final_start,
                        end: source_end,
                    });
                }
            }
        }
    }
}

/// Intermediate per-timestamp entry before segment ends are resolved.
struct RawEntry {
    input_index: usize,
    start: f64,
    text: String,
    /// `Some` when the line was enhanced (word tokens parsed from `<...>` tags).
    word_tokens: Option<Vec<(f64, String)>>,
}

/// Parse a fractional part expressed as decimal digits (e.g. `"45"` -> 0.45).
fn parse_fraction(frac: &str) -> Option<f64> {
    if frac.is_empty() {
        return Some(0.0);
    }
    if !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let n: f64 = frac.parse().ok()?;
    let denom = 10f64.powi(frac.len() as i32);
    Some(n / denom)
}

/// Parse a timestamp body like `mm:ss.xx`, `mm:ss`, `mm:ss.xxx` or `mm:ss:xx`
/// into seconds.
fn parse_timestamp(body: &str) -> Option<f64> {
    let body = body.trim();
    let first_colon = body.find(':')?;
    let minutes: f64 = body[..first_colon].trim().parse().ok()?;
    let rest = &body[first_colon + 1..];

    let seconds = if let Some(second_colon) = rest.find(':') {
        // `mm:ss:xx` colon-decimal form.
        let ss: f64 = rest[..second_colon].trim().parse().ok()?;
        ss + parse_fraction(rest[second_colon + 1..].trim())?
    } else {
        rest.trim().parse::<f64>().ok()?
    };

    Some(minutes * 60.0 + seconds)
}

/// Split enhanced-line content on `<mm:ss.xx>` tags into `(start, text)` tokens.
/// A prefix uses the supplied line start, just like square-bracket word timing.
fn parse_word_tokens(content: &str, line_start: f64) -> Vec<(f64, String)> {
    let mut tokens: Vec<(f64, String)> = Vec::new();
    let mut cur_ts = Some(line_start);
    let mut cur_text = String::new();
    let mut saw_inline_timestamp = false;
    let mut i = 0;

    while i < content.len() {
        if content[i..].starts_with('<')
            && let Some(close_rel) = content[i..].find('>')
        {
            let inner = &content[i + 1..i + close_rel];
            if let Some(ts) = parse_timestamp(inner) {
                if let Some(prev_ts) = cur_ts {
                    let text = cur_text.trim().to_string();
                    if !text.is_empty() {
                        tokens.push((prev_ts, text));
                    }
                }
                cur_text.clear();
                cur_ts = Some(ts);
                saw_inline_timestamp = true;
                i += close_rel + 1;
                continue;
            }
        }
        let ch = content[i..].chars().next().unwrap();
        cur_text.push(ch);
        i += ch.len_utf8();
    }

    if let Some(ts) = cur_ts {
        let text = cur_text.trim().to_string();
        if !text.is_empty() {
            tokens.push((ts, text));
        }
    }

    if saw_inline_timestamp {
        tokens
    } else {
        Vec::new()
    }
}

/// Some providers (notably QQ Music fallbacks) encode per-character timing as
/// square-bracket timestamps after the line timestamp, for example
/// `[00:08.86]穢[00:08.94]れ[00:09.02]な`. Treat those inner tags as enhanced
/// timing instead of leaving them inside the lyric text sent to alignment.
fn parse_square_word_tokens(content: &str, line_start: f64) -> Vec<(f64, String)> {
    let mut tokens = Vec::new();
    let mut current_start = line_start;
    let mut current_text = String::new();
    let mut saw_inline_timestamp = false;
    let mut i = 0;

    while i < content.len() {
        if content[i..].starts_with('[')
            && let Some(close_rel) = content[i..].find(']')
        {
            let inner = &content[i + 1..i + close_rel];
            if let Some(timestamp) = parse_timestamp(inner) {
                let text = current_text.trim().to_string();
                if !text.is_empty() {
                    tokens.push((current_start, text));
                }
                current_text.clear();
                current_start = timestamp;
                saw_inline_timestamp = true;
                i += close_rel + 1;
                continue;
            }
        }
        let ch = content[i..].chars().next().unwrap();
        current_text.push(ch);
        i += ch.len_utf8();
    }

    let text = current_text.trim().to_string();
    if !text.is_empty() {
        tokens.push((current_start, text));
    }
    if saw_inline_timestamp {
        tokens
    } else {
        Vec::new()
    }
}

fn timed_tokens_display_text(tokens: &[(f64, String)]) -> String {
    let mut output = String::new();
    for (_, text) in tokens {
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let needs_space =
            output
                .chars()
                .last()
                .zip(text.chars().next())
                .is_some_and(|(left, right)| {
                    left.is_ascii_alphanumeric() && right.is_ascii_alphanumeric()
                });
        if needs_space {
            output.push(' ');
        }
        output.push_str(text);
    }
    output
}

/// Extract leading `[...]` tags from a line, returning the parsed line-level
/// timestamps, the remaining content, and any `[offset:...]` in milliseconds.
fn split_line(line: &str) -> (Vec<f64>, String, Option<f64>) {
    let mut timestamps = Vec::new();
    let mut offset_ms = None;
    let mut rest = line.trim();

    while rest.starts_with('[') {
        let Some(close) = rest.find(']') else {
            break;
        };
        let inner = &rest[1..close];
        let after = rest[close + 1..].trim_start();

        // Timestamp tags begin with a digit; metadata tags (ar/ti/offset/...)
        // begin with a letter.
        if inner.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            if let Some(ts) = parse_timestamp(inner) {
                timestamps.push(ts);
            }
        } else if let Some(value) = inner.strip_prefix("offset:").or_else(|| {
            inner
                .split_once(':')
                .filter(|(k, _)| k.eq_ignore_ascii_case("offset"))
                .map(|(_, v)| v)
        }) {
            offset_ms = value.trim().parse::<f64>().ok();
        }

        rest = after;
    }

    (timestamps, rest.to_string(), offset_ms)
}

/// Parse LRC / Enhanced LRC text into ordered segments. Returns an error when
/// no timestamped lyric lines are found.
pub fn parse_lrc(text: &str) -> Result<ParsedLrc, String> {
    let mut entries: Vec<RawEntry> = Vec::new();
    let mut input_lines = Vec::new();
    let mut has_untimed_body = false;
    // Timestamps on empty lines (e.g. a trailing `[mm:ss.xx]`) don't produce a
    // segment; they mark where the previous line's highlight should stop.
    let mut breaks: Vec<f64> = Vec::new();
    let mut offset_secs = 0.0;

    for raw_line in text.lines() {
        let (timestamps, content, offset_ms) = split_line(raw_line);
        if let Some(ms) = offset_ms {
            offset_secs = ms / 1000.0;
        }
        if timestamps.is_empty() {
            // Keep untagged body text, but do not turn metadata, section tags,
            // or malformed leading tags that were filtered before into lyrics.
            if !raw_line.trim_start().starts_with('[') && !content.trim().is_empty() {
                input_lines.push(LrcInputLine {
                    text: content.trim().to_string(),
                    range: None,
                });
                has_untimed_body = true;
            }
            continue;
        }
        if content.trim().is_empty() {
            for ts in &timestamps {
                breaks.push((ts + offset_secs).max(0.0));
            }
            continue;
        }

        for ts in timestamps {
            let word_tokens = if content.contains('<') {
                let tokens = parse_word_tokens(&content, ts);
                (!tokens.is_empty()).then_some(tokens)
            } else if content.contains('[') {
                let tokens = parse_square_word_tokens(&content, ts);
                (!tokens.is_empty()).then_some(tokens)
            } else {
                None
            };
            let display_text = word_tokens
                .as_deref()
                .map(timed_tokens_display_text)
                .filter(|text| !text.is_empty())
                .unwrap_or_else(|| content.trim().to_string());
            let input_index = input_lines.len();
            input_lines.push(LrcInputLine {
                text: display_text.clone(),
                range: None,
            });
            entries.push(RawEntry {
                input_index,
                start: (ts + offset_secs).max(0.0),
                text: display_text,
                word_tokens,
            });
        }
    }

    if entries.is_empty() {
        return Err("No timestamped lyric lines found in the provided LRC".to_string());
    }

    entries.sort_by(|a, b| {
        a.start
            .partial_cmp(&b.start)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // A segment's highlight ends at the earliest boundary after its start: the
    // next line's start or an empty-timestamp marker, whichever comes first.
    let mut boundaries: Vec<f64> = entries.iter().map(|e| e.start).collect();
    boundaries.extend(breaks.iter().copied());
    boundaries.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut segments = Vec::with_capacity(entries.len());
    let mut final_end_inferred = false;

    for entry in &entries {
        let seg_start = entry.start;
        let known_end = boundaries.iter().copied().find(|&b| b > seg_start + 1e-6);
        let seg_end = known_end.unwrap_or_else(|| {
            final_end_inferred = true;
            seg_start + LAST_SEGMENT_SECS
        });
        input_lines[entry.input_index].range = Some(LrcInputRange {
            start: seg_start,
            end: seg_end,
        });

        let words = match &entry.word_tokens {
            Some(tokens) => {
                let mut words = Vec::with_capacity(tokens.len());
                for (i, (start, word)) in tokens.iter().enumerate() {
                    let start = (start + offset_secs).max(0.0);
                    let end = tokens
                        .get(i + 1)
                        .map(|(next, _)| (next + offset_secs).max(start))
                        .unwrap_or(seg_end.max(start));
                    words.push(LrcWord {
                        word: word.clone(),
                        start,
                        end,
                    });
                }
                words
            }
            None => vec![LrcWord {
                word: entry.text.clone(),
                start: seg_start,
                end: seg_end,
            }],
        };

        segments.push(LrcSegment {
            text: entry.text.clone(),
            start: seg_start,
            end: seg_end,
            words,
        });
    }

    if !has_untimed_body {
        input_lines = entries
            .iter()
            .map(|entry| input_lines[entry.input_index].clone())
            .collect();
    }

    Ok(ParsedLrc {
        input_lines,
        segments,
        final_end_inferred,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_duration_extends_only_an_inferred_final_line_end() {
        let mut inferred = parse_lrc("[00:10.00]last line").unwrap();
        inferred.extend_inferred_final_end(22.0);
        assert_eq!(inferred.segments[0].end, 22.0);
        assert_eq!(inferred.segments[0].words[0].end, 22.0);

        let mut explicit = parse_lrc("[00:10.00]last line\n[00:14.50]").unwrap();
        explicit.extend_inferred_final_end(22.0);
        assert_eq!(explicit.segments[0].end, 14.5);
        assert_eq!(explicit.segments[0].words[0].end, 14.5);
    }

    #[test]
    fn square_bracket_character_timing_does_not_leak_into_alignment_text() {
        let parsed =
            parse_lrc("[00:08.86]穢[00:08.94]れ[00:09.02]な[00:09.10]き\n[00:10.00]次の行")
                .unwrap();
        let first = &parsed.segments[0];
        assert_eq!(first.text, "穢れなき");
        assert_eq!(
            first
                .words
                .iter()
                .map(|word| word.word.as_str())
                .collect::<Vec<_>>(),
            ["穢", "れ", "な", "き"]
        );
        assert!((first.words[0].start - 8.86).abs() < 0.000_001);
        assert!((first.words[1].start - 8.94).abs() < 0.000_001);
        assert!(!first.text.contains("[00:"));
    }

    #[test]
    fn enhanced_cjk_tokens_render_without_invented_spaces() {
        let parsed = parse_lrc("[00:01.00]<00:01.00>霞<00:01.25>む\n[00:02.00]景色").unwrap();
        assert_eq!(parsed.segments[0].text, "霞む");
        assert_eq!(parsed.segments[0].words.len(), 2);
    }

    #[test]
    fn repeated_leading_line_timestamps_remain_duplicate_line_entries() {
        let parsed = parse_lrc("[00:01.00][00:02.00]chorus\n[00:03.00]next").unwrap();
        assert_eq!(parsed.segments[0].text, "chorus");
        assert_eq!(parsed.segments[1].text, "chorus");
        assert_eq!(parsed.segments[0].words.len(), 1);
        assert_eq!(parsed.segments[1].words.len(), 1);
    }

    #[test]
    fn enhanced_prefix_uses_the_supplied_line_start_without_losing_text() {
        let parsed = parse_lrc("[00:10.00]Hello <00:11.00>world\n[00:12.00]").unwrap();
        let segment = &parsed.segments[0];
        assert_eq!(segment.text, "Hello world");
        assert_eq!(parsed.input_lines[0].text, "Hello world");
        assert_eq!(segment.words.len(), 2);
        assert_eq!(segment.words[0].word, "Hello");
        assert_eq!((segment.words[0].start, segment.words[0].end), (10.0, 11.0));
        assert_eq!(segment.words[1].word, "world");
        assert_eq!((segment.words[1].start, segment.words[1].end), (11.0, 12.0));
    }

    #[test]
    fn enhanced_cjk_prefix_keeps_text_and_applies_the_existing_offset() {
        let parsed = parse_lrc("[offset:250]\n[00:10.00]霞<00:11.00>む\n[00:12.00]景色").unwrap();
        let segment = &parsed.segments[0];
        assert_eq!(segment.text, "霞む");
        assert_eq!(segment.words[0].word, "霞");
        assert_eq!(
            (segment.words[0].start, segment.words[0].end),
            (10.25, 11.25)
        );
        assert_eq!(segment.words[1].start, 11.25);
    }

    #[test]
    fn mixed_body_lines_keep_source_order_without_acquiring_timestamps() {
        let parsed = parse_lrc(concat!(
            "opening\n",
            "[00:20.00]late\n",
            "bridge\n",
            "[00:10.00][00:30.00]repeat\n",
            "repeat\n",
            "[00:40.00]\n",
            "outro",
        ))
        .unwrap();
        assert_eq!(
            parsed
                .input_lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            [
                "opening", "late", "bridge", "repeat", "repeat", "repeat", "outro"
            ]
        );
        assert_eq!(
            parsed
                .input_lines
                .iter()
                .map(|line| line.range.map(|range| range.start))
                .collect::<Vec<_>>(),
            [None, Some(20.0), None, Some(10.0), Some(30.0), None, None]
        );
        assert_eq!(
            parsed
                .segments
                .iter()
                .map(|segment| (segment.start, segment.end))
                .collect::<Vec<_>>(),
            [(10.0, 20.0), (20.0, 30.0), (30.0, 40.0)]
        );
    }

    #[test]
    fn pure_timed_input_expands_repeats_in_timestamp_order() {
        let parsed = parse_lrc("[00:30.00][00:10.00]chorus\n[00:20.00]verse\n[00:40.00]").unwrap();
        assert_eq!(
            parsed
                .input_lines
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            ["chorus", "verse", "chorus"]
        );
        for (line, segment) in parsed.input_lines.iter().zip(&parsed.segments) {
            assert_eq!(line.text, segment.text);
            assert_eq!(
                line.range,
                Some(LrcInputRange {
                    start: segment.start,
                    end: segment.end,
                })
            );
        }
    }

    #[test]
    fn untimed_body_support_does_not_promote_filtered_tags_to_lyrics() {
        let parsed = parse_lrc(concat!(
            "[ar:Artist]\n[ti:Title]\n[by:Editor]\n[Offset:250]\n",
            "[unknown:tag]ignored suffix\n[Verse 1]\n[00:invalid]ignored\n",
            "\n[00:10.00]sung\n  body  \n[00:12.00]\n",
        ))
        .unwrap();
        assert_eq!(
            parsed.input_lines,
            [
                LrcInputLine {
                    text: "sung".to_string(),
                    range: Some(LrcInputRange {
                        start: 10.25,
                        end: 12.25
                    }),
                },
                LrcInputLine {
                    text: "body".to_string(),
                    range: None
                },
            ]
        );
        assert!(parse_lrc("[ar:Artist]\n[ti:Title]\n[offset:0]").is_err());
    }

    #[test]
    fn extending_final_display_scope_never_times_untimed_input() {
        let mut parsed = parse_lrc("intro\n[00:10.00]last sung line\noutro").unwrap();
        parsed.extend_inferred_final_end(22.0);
        assert_eq!(parsed.input_lines[0].range, None);
        assert_eq!(
            parsed.input_lines[1].range,
            Some(LrcInputRange {
                start: 10.0,
                end: 22.0
            })
        );
        assert_eq!(parsed.input_lines[2].range, None);
        assert_eq!(parsed.segments[0].end, 22.0);

        let mut explicit = parse_lrc("[00:10.00]last\n[00:14.50]\noutro").unwrap();
        explicit.extend_inferred_final_end(22.0);
        assert_eq!(explicit.input_lines[0].range.unwrap().end, 14.5);
        assert_eq!(explicit.input_lines[1].range, None);
    }

    #[test]
    fn simultaneous_final_lines_keep_consistent_display_and_input_scopes() {
        let mut parsed = parse_lrc("[00:10.00]first\n[00:10.00]second").unwrap();
        parsed.extend_inferred_final_end(22.0);
        assert!(parsed.segments.iter().all(|segment| segment.end == 22.0));
        assert!(
            parsed
                .input_lines
                .iter()
                .all(|line| line.range.unwrap().end == 22.0)
        );
    }
}
