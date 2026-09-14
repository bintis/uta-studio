//! Audio-anchored alignment scheduling. Text length never selects audio time.

use super::aligner::{AlignedWord, Alignment};
use super::frontend::SAMPLE_RATE;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioScope {
    pub start_sample: usize,
    pub end_sample: usize,
}

#[derive(Clone, Debug)]
pub struct AlignmentWindowTrace {
    pub start_micros: u64,
    pub end_micros: u64,
    pub first_word: usize,
    pub word_count: usize,
    pub anchored: bool,
    pub raw_timestamp_ms: Vec<u64>,
    pub corrected_timestamp_ms: Vec<u64>,
    pub timing_issues: Vec<Option<String>>,
}

fn seconds(samples: usize) -> f64 {
    samples as f64 / SAMPLE_RATE as f64
}

fn millis(samples: usize) -> u64 {
    (samples as u128 * 1_000 / SAMPLE_RATE as u128) as u64
}

fn unresolved(word: &str, scope: AudioScope, reason: &str) -> AlignedWord {
    AlignedWord {
        text: word.to_string(),
        start_seconds: seconds(scope.start_sample),
        end_seconds: seconds(scope.end_sample),
        timing_issue: Some(reason.to_string()),
    }
}

/// Keep independent classifier peaks and the ordered acoustic hypothesis.
/// A collapsed hypothesis remains unresolved; positive intervals come from
/// classifier evidence, never interpolation or a minimum-duration rule.
pub(super) fn resolve_local_timing(alignment: &mut Alignment, sample_count: usize, period_ms: u64) {
    let source_ms = millis(sample_count);
    let scope = AudioScope {
        start_sample: 0,
        end_sample: sample_count,
    };
    let mut previous_end = 0;
    for (index, word) in alignment.words.iter_mut().enumerate() {
        let start = alignment.corrected_timestamp_ms[index * 2];
        let end = alignment.corrected_timestamp_ms[index * 2 + 1];
        let reason = if !word.text.chars().any(char::is_alphanumeric) {
            Some("nonlexical_unit")
        } else if end <= start {
            Some("collapsed_timestamp")
        } else if start >= source_ms || end > source_ms.saturating_add(period_ms) {
            Some("outside_audio_window")
        } else if start < previous_end {
            Some("nonmonotonic_timestamp")
        } else {
            None
        };
        if let Some(reason) = reason {
            *word = unresolved(&word.text, scope, reason);
        } else {
            word.start_seconds = start as f64 / 1_000.0;
            word.end_seconds = end.min(source_ms) as f64 / 1_000.0;
            word.timing_issue = None;
            previous_end = end.min(source_ms);
        }
    }
}

fn windows(scope: AudioScope, maximum: usize, margin: usize) -> Vec<AudioScope> {
    let mut result = Vec::new();
    let mut start = scope.start_sample;
    loop {
        let end = start.saturating_add(maximum).min(scope.end_sample);
        result.push(AudioScope {
            start_sample: start,
            end_sample: end,
        });
        if end == scope.end_sample {
            break;
        }
        start += maximum - margin * 2;
    }
    result
}

/// Scoped words are aligned only against their actual ASR/caller audio range.
/// Unscoped long lyrics use a forward acoustic search: a silent window consumes
/// no text, and a window advances the text cursor only after measured words.
/// Missing words remain explicit unresolved items, not synthetic 1 ms events.
pub(super) fn align_scoped(
    samples: &[f32],
    words: &[String],
    scopes: &[Option<AudioScope>],
    maximum: usize,
    period_ms: u64,
    mut infer: impl FnMut(&[f32], &[String]) -> Result<Alignment, String>,
    report: &mut dyn FnMut(u64, u64),
) -> Result<Alignment, String> {
    if samples.is_empty() || maximum == 0 || period_ms == 0 || scopes.len() != words.len() {
        return Err("Qwen alignment scope geometry is invalid".to_string());
    }
    let whole = AudioScope {
        start_sample: 0,
        end_sample: samples.len(),
    };
    for scope in scopes.iter().flatten() {
        if scope.start_sample >= scope.end_sample || scope.end_sample > samples.len() {
            return Err("Qwen alignment audio scope is outside the source".to_string());
        }
    }
    let margin = SAMPLE_RATE.min(maximum / 4);
    let mut groups = Vec::new();
    let mut first = 0;
    while first < words.len() {
        let mut end = first + 1;
        while end < words.len() && scopes[end] == scopes[first] {
            end += 1;
        }
        let scope = scopes[first].unwrap_or_else(|| AudioScope {
            // Include neighboring coarse scopes rather than guessing a point
            // inside them. Their measured words enforce monotonicity below.
            start_sample: scopes[..first]
                .iter()
                .rev()
                .flatten()
                .next()
                .map_or(0, |scope| scope.start_sample),
            end_sample: scopes[end..]
                .iter()
                .flatten()
                .next()
                .map_or(samples.len(), |scope| scope.end_sample),
        });
        if scope.start_sample >= scope.end_sample {
            return Err("Qwen alignment neighboring scopes are reversed".to_string());
        }
        groups.push((
            first,
            end,
            scopes[first].is_some(),
            windows(scope, maximum, margin),
        ));
        first = end;
    }
    let total = groups
        .iter()
        .map(|(_, _, _, windows)| windows.len() as u64)
        .sum();
    let mut completed = 0;
    report(completed, total);
    let mut result = Alignment {
        words: words
            .iter()
            .map(|word| unresolved(word, whole, "no_acoustic_anchor"))
            .collect(),
        raw_classes: vec![0; words.len() * 2],
        raw_timestamp_ms: vec![0; words.len() * 2],
        corrected_timestamp_ms: vec![0; words.len() * 2],
        windows: Vec::new(),
        prompt_tokens: 0,
        encoder_seconds: 0.0,
        decoder_seconds: 0.0,
    };
    let mut attempted_scopes = vec![whole; words.len()];
    for (first, end, anchored, group_windows) in groups {
        let single = group_windows.len() == 1;
        let mut cursor = first;
        let last_window = group_windows.len() - 1;
        for (window_index, scope) in group_windows.into_iter().enumerate() {
            if cursor < end {
                // One positive word interval needs at least one timestamp bin.
                // This bounds lexical lookahead, not song length or acceptance.
                let lookahead = usize::try_from(millis(maximum) / period_ms)
                    .unwrap_or(usize::MAX)
                    .max(1);
                let stop = if single {
                    end
                } else {
                    cursor.saturating_add(lookahead).min(end)
                };
                let local = infer(
                    &samples[scope.start_sample..scope.end_sample],
                    &words[cursor..stop],
                )?;
                if local.words.len() != stop - cursor
                    || local.raw_classes.len() != local.words.len() * 2
                    || local.raw_timestamp_ms.len() != local.words.len() * 2
                    || local.corrected_timestamp_ms.len() != local.words.len() * 2
                {
                    return Err("Qwen local alignment changed the word/evidence shape".to_string());
                }
                let offset_ms = millis(scope.start_sample);
                let offset_seconds = seconds(scope.start_sample);
                let core_start = if window_index == 0 {
                    0.0
                } else {
                    seconds(margin)
                };
                let core_end = seconds(scope.end_sample - scope.start_sample)
                    - if window_index == last_window {
                        0.0
                    } else {
                        seconds(margin)
                    };
                let mut consumed = cursor;
                for (index, measured) in local.words.iter().enumerate() {
                    let target = cursor + index;
                    attempted_scopes[target] = scope;
                    result.raw_classes[target * 2..target * 2 + 2]
                        .copy_from_slice(&local.raw_classes[index * 2..index * 2 + 2]);
                    for edge in 0..2 {
                        result.raw_timestamp_ms[target * 2 + edge] = local.raw_timestamp_ms
                            [index * 2 + edge]
                            .checked_add(offset_ms)
                            .ok_or("Qwen raw timestamp overflows")?;
                        result.corrected_timestamp_ms[target * 2 + edge] = local
                            .corrected_timestamp_ms[index * 2 + edge]
                            .checked_add(offset_ms)
                            .ok_or("Qwen corrected timestamp overflows")?;
                    }
                    let midpoint = (measured.start_seconds + measured.end_seconds) * 0.5;
                    if measured.timing_issue.is_none()
                        && (single || (midpoint >= core_start && midpoint < core_end))
                    {
                        let mut measured = measured.clone();
                        measured.start_seconds += offset_seconds;
                        measured.end_seconds += offset_seconds;
                        result.words[target] = measured;
                        consumed = target + 1;
                    } else {
                        result.words[target] = unresolved(
                            &words[target],
                            scope,
                            measured
                                .timing_issue
                                .as_deref()
                                .unwrap_or("window_edge_pending"),
                        );
                    }
                }
                result.windows.push(AlignmentWindowTrace {
                    start_micros: (scope.start_sample as u128 * 1_000_000 / SAMPLE_RATE as u128)
                        as u64,
                    end_micros: (scope.end_sample as u128 * 1_000_000 / SAMPLE_RATE as u128) as u64,
                    first_word: cursor,
                    word_count: stop - cursor,
                    anchored,
                    raw_timestamp_ms: local.raw_timestamp_ms,
                    corrected_timestamp_ms: local.corrected_timestamp_ms,
                    timing_issues: local
                        .words
                        .iter()
                        .map(|word| word.timing_issue.clone())
                        .collect(),
                });
                result.prompt_tokens = result
                    .prompt_tokens
                    .checked_add(local.prompt_tokens)
                    .ok_or("Qwen alignment prompt count overflows")?;
                result.encoder_seconds += local.encoder_seconds;
                result.decoder_seconds += local.decoder_seconds;
                cursor = if single { end } else { consumed };
            }
            completed += 1;
            report(completed, total);
        }
    }
    let mut previous_end = 0.0;
    for (word, scope) in result.words.iter_mut().zip(attempted_scopes) {
        if word.timing_issue.is_none() {
            if word.start_seconds < previous_end {
                *word = unresolved(&word.text, scope, "overlapping_anchor_alignment");
            } else {
                previous_end = word.end_seconds;
            }
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prediction(words: &[String], ranges: &[(u64, u64)], sample_count: usize) -> Alignment {
        let timestamps = ranges
            .iter()
            .flat_map(|&(start, end)| [start, end])
            .collect::<Vec<_>>();
        let mut alignment = Alignment {
            words: words
                .iter()
                .map(|word| AlignedWord {
                    text: word.clone(),
                    start_seconds: 0.0,
                    end_seconds: 0.0,
                    timing_issue: None,
                })
                .collect(),
            raw_classes: timestamps.iter().map(|time| (*time / 80) as u32).collect(),
            raw_timestamp_ms: timestamps.clone(),
            corrected_timestamp_ms: timestamps,
            windows: Vec::new(),
            prompt_tokens: words.len() * 3,
            encoder_seconds: 0.0,
            decoder_seconds: 0.0,
        };
        resolve_local_timing(&mut alignment, sample_count, 80);
        alignment
    }

    #[test]
    fn collapsed_ranges_are_scopes_not_fabricated_millisecond_words() {
        let words = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        let alignment = prediction(&words, &[(80, 240), (240, 240), (320, 480)], SAMPLE_RATE);
        assert!(alignment.words[0].timing_issue.is_none());
        assert_eq!(
            alignment.words[1].timing_issue.as_deref(),
            Some("collapsed_timestamp")
        );
        assert_eq!(alignment.words[1].end_seconds, 1.0);
        assert!(alignment.words[2].timing_issue.is_none());
        assert_eq!(&alignment.raw_timestamp_ms[2..4], &[240, 240]);
    }

    #[test]
    fn ordered_acoustic_alternative_can_resolve_reversed_independent_peaks() {
        let words = vec!["first".to_string(), "second".to_string()];
        let mut alignment = prediction(&words, &[(80, 160), (160, 240)], SAMPLE_RATE);
        alignment.raw_timestamp_ms = vec![80, 240, 80, 240];
        resolve_local_timing(&mut alignment, SAMPLE_RATE, 80);
        assert!(
            alignment
                .words
                .iter()
                .all(|word| word.timing_issue.is_none())
        );
        assert_eq!(alignment.words[0].end_seconds, 0.16);
        assert_eq!(alignment.words[1].start_seconds, 0.16);
        assert_eq!(alignment.raw_timestamp_ms, [80, 240, 80, 240]);
    }

    #[test]
    fn anchors_skip_intro_and_interlude_without_text_time_proportions() {
        let samples = (0..SAMPLE_RATE * 6)
            .map(|sample| sample as f32)
            .collect::<Vec<_>>();
        let words = vec!["first".to_string(), "again".to_string()];
        let scopes = [
            Some(AudioScope {
                start_sample: SAMPLE_RATE * 2,
                end_sample: SAMPLE_RATE * 3,
            }),
            Some(AudioScope {
                start_sample: SAMPLE_RATE * 5,
                end_sample: SAMPLE_RATE * 6,
            }),
        ];
        let mut starts = Vec::new();
        let result = align_scoped(
            &samples,
            &words,
            &scopes,
            SAMPLE_RATE,
            80,
            |audio, words| {
                starts.push(audio[0] as usize);
                Ok(prediction(words, &[(80, 320)], audio.len()))
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(starts, [SAMPLE_RATE * 2, SAMPLE_RATE * 5]);
        assert_eq!(result.words[0].start_seconds, 2.08);
        assert_eq!(result.words[1].start_seconds, 5.08);
        assert!(result.windows.iter().all(|window| window.anchored));
    }

    #[test]
    fn unanchored_search_does_not_consume_lyrics_in_silent_windows() {
        let samples = (0..SAMPLE_RATE * 3)
            .map(|sample| sample as f32)
            .collect::<Vec<_>>();
        let words = vec!["sing".to_string()];
        let result = align_scoped(
            &samples,
            &words,
            &[None],
            SAMPLE_RATE,
            80,
            |audio, words| {
                let offset = millis(audio[0] as usize);
                let range = if offset <= 2080 && offset + millis(audio.len()) >= 2320 {
                    (2080 - offset, 2320 - offset)
                } else {
                    (0, 0)
                };
                Ok(prediction(words, &[range], audio.len()))
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert!(result.words[0].timing_issue.is_none());
        assert_eq!(result.words[0].start_seconds, 2.08);
        assert!(result.windows.len() > 1);
        assert_eq!(result.words.len(), 1);
    }

    #[test]
    fn out_of_window_and_reversed_ranges_are_explicitly_unresolved() {
        let words = vec!["a".to_string(), "b".to_string()];
        let result = prediction(&words, &[(2000, 2400), (400, 320)], SAMPLE_RATE);
        assert!(result.words.iter().all(|word| word.timing_issue.is_some()));
    }
}
