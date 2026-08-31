//! Parser for Standard LRC (line-level timestamps) and Enhanced LRC
//! (word-level timestamps) into Uta! Studio's editable transcript shape.
//!
//! Standard:  `[00:12.00]First line of lyrics`
//! Enhanced:  `[00:12.00]<00:12.00>I <00:12.30>see <00:12.60>trees`
//!
//! Word-level lines produce one token per `<mm:ss.xx>` tag with exact timing.
//! Line-level lines produce a single token spanning the whole line, so the
//! renderer highlights the line as a unit.

use serde::Serialize;

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

#[derive(Debug, Clone)]
pub struct ParsedLrc {
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
        let Some(segment) = self.segments.last_mut() else {
            return;
        };
        if source_end <= segment.end {
            return;
        }
        let previous_end = segment.end;
        segment.end = source_end;
        if let Some(word) = segment.words.last_mut()
            && (word.end - previous_end).abs() <= f64::EPSILON
        {
            word.end = source_end;
        }
    }
}

/// Intermediate per-timestamp entry before segment ends are resolved.
struct RawEntry {
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
/// Text before the first tag (rare) is discarded, matching common LRC tooling.
fn parse_word_tokens(content: &str) -> Vec<(f64, String)> {
    let mut tokens: Vec<(f64, String)> = Vec::new();
    let mut cur_ts: Option<f64> = None;
    let mut cur_text = String::new();
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

    tokens
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
            continue;
        }
        if content.trim().is_empty() {
            for ts in &timestamps {
                breaks.push((ts + offset_secs).max(0.0));
            }
            continue;
        }

        let word_tokens = if content.contains('<') {
            let tokens = parse_word_tokens(&content);
            if tokens.is_empty() {
                None
            } else {
                Some(tokens)
            }
        } else {
            None
        };

        // Plain display text (word tags stripped) for the segment label.
        let display_text = match &word_tokens {
            Some(tokens) => tokens
                .iter()
                .map(|(_, w)| w.as_str())
                .collect::<Vec<_>>()
                .join(" "),
            None => content.trim().to_string(),
        };

        for ts in timestamps {
            entries.push(RawEntry {
                start: (ts + offset_secs).max(0.0),
                text: display_text.clone(),
                word_tokens: word_tokens.clone(),
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

    Ok(ParsedLrc {
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
}
