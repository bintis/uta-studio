#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Interval {
    pub start: usize,
    pub end: usize,
    pub label: i64,
    pub word: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alignment {
    pub mel_to_phoneme: Vec<i64>,
    pub phoneme_boundary: Vec<i64>,
    pub mel_to_word: Vec<i64>,
    pub word_boundary: Vec<i64>,
    pub phoneme_intervals: Vec<Interval>,
    pub word_intervals: Vec<Interval>,
    pub dp_last: Vec<f32>,
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
}

fn log_softmax(row: &[f32]) -> Vec<f32> {
    let maximum = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let log_sum = row
        .iter()
        .map(|value| (*value - maximum).exp())
        .sum::<f32>()
        .ln()
        + maximum;
    row.iter().map(|value| *value - log_sum).collect()
}

/// Reproduce pinned upstream `get_ph_word_bd` + `perform_viterbi_bd`.
///
/// `boundary_input` is already sigmoid(Stage-A logits), matching the upstream
/// caller. The decoder intentionally applies the second sigmoid used by the
/// pinned implementation. Correcting this requires a separate versioned policy.
pub fn align(
    prediction: &[f32],
    classes: usize,
    boundary_input: &[f32],
    labels: &[i64],
    phoneme_to_word: &[i64],
) -> Result<Alignment, String> {
    let frames = boundary_input.len();
    if frames == 0
        || classes < 4
        || prediction.len() != frames * classes
        || labels.len() != phoneme_to_word.len()
    {
        return Err("STARS Viterbi input shape is invalid".to_string());
    }
    let filtered = labels
        .iter()
        .copied()
        .zip(phoneme_to_word.iter().copied())
        .filter(|(label, _)| *label > 1)
        .collect::<Vec<_>>();
    if filtered.is_empty()
        || filtered.iter().any(|(label, _)| {
            usize::try_from(*label - 2).map_or(true, |value| value >= classes - 3)
        })
    {
        return Err("STARS Viterbi phoneme labels are invalid".to_string());
    }
    let labels = filtered.iter().map(|value| value.0).collect::<Vec<_>>();
    let words = filtered.iter().map(|value| value.1).collect::<Vec<_>>();
    let boundary_log = boundary_input
        .iter()
        .map(|value| (sigmoid(*value) + 1e-6).ln())
        .collect::<Vec<_>>();
    let no_boundary_log = boundary_input
        .iter()
        .map(|value| (1.0 - sigmoid(*value) + 1e-6).ln())
        .collect::<Vec<_>>();
    let mut silence = Vec::with_capacity(frames);
    let mut phoneme_log = Vec::with_capacity(frames);
    for row in prediction.chunks_exact(classes) {
        // Upstream drops class zero, then uses index one as silence and the
        // remaining values as phoneme logits.
        let values = log_softmax(&row[1..]);
        silence.push(values[1].max(-1_000.0));
        phoneme_log.push(
            values[2..]
                .iter()
                .map(|value| value.max(-1_000.0))
                .collect::<Vec<_>>(),
        );
    }
    let states = labels.len() * 2 + 1;
    let mut dp = vec![vec![-10_000_000.0_f32; states]; frames];
    let mut backtrace = vec![vec![0_usize; states]; frames];
    dp[0][0] = silence[0];
    dp[0][1] = phoneme_log[0][usize::try_from(labels[0] - 2).unwrap()];
    for frame in 1..frames {
        for state in 0..states {
            if state == 0 {
                backtrace[frame][state] = state;
                dp[frame][state] = dp[frame - 1][state] + silence[frame] + no_boundary_log[frame];
            } else if state == 1 {
                let stay = dp[frame - 1][state] + no_boundary_log[frame];
                let advance = dp[frame - 1][state - 1] + boundary_log[frame];
                let source = if stay > advance { state } else { state - 1 };
                backtrace[frame][state] = source;
                dp[frame][state] = dp[frame - 1][source]
                    + phoneme_log[frame][usize::try_from(labels[0] - 2).unwrap()]
                    + if source == state {
                        no_boundary_log[frame]
                    } else {
                        boundary_log[frame]
                    };
            } else if state % 2 == 0 {
                let stay = dp[frame - 1][state] + no_boundary_log[frame];
                let advance = dp[frame - 1][state - 1] + boundary_log[frame];
                let source = if stay > advance { state } else { state - 1 };
                backtrace[frame][state] = source;
                dp[frame][state] = dp[frame - 1][source]
                    + silence[frame]
                    + if source == state {
                        no_boundary_log[frame]
                    } else {
                        boundary_log[frame]
                    };
            } else {
                let from_previous = dp[frame - 1][state - 2] + boundary_log[frame];
                let from_blank = dp[frame - 1][state - 1] + boundary_log[frame];
                let stay = dp[frame - 1][state] + no_boundary_log[frame];
                let source = if from_previous >= from_blank && from_previous >= stay {
                    state - 2
                } else if stay > from_blank {
                    state
                } else {
                    state - 1
                };
                backtrace[frame][state] = source;
                dp[frame][state] = dp[frame - 1][source]
                    + phoneme_log[frame][usize::try_from(labels[state / 2] - 2).unwrap()]
                    + if source == state {
                        no_boundary_log[frame]
                    } else {
                        boundary_log[frame]
                    };
            }
        }
    }
    let mut state = if dp[frames - 1][states - 1] > dp[frames - 1][states - 2] {
        states - 1
    } else {
        states - 2
    };
    let mut path = Vec::with_capacity(frames);
    path.push(state);
    state = backtrace[frames - 1][state];
    for frame in (0..frames - 1).rev() {
        path.push(state);
        state = backtrace[frame][state];
    }
    path.reverse();

    let mut phoneme_intervals = Vec::new();
    let mut word_intervals: Vec<Interval> = Vec::new();
    let mut end = 0;
    for (index, (label, word)) in labels
        .iter()
        .copied()
        .zip(words.iter().copied())
        .enumerate()
    {
        let target = index * 2 + 1;
        let first = path
            .iter()
            .position(|value| *value == target)
            .ok_or_else(|| "STARS Viterbi path omitted a phoneme".to_string())?;
        let last = path.iter().rposition(|value| *value == target).unwrap();
        // Upstream uses 0.1 / 0.02 and therefore inserts SP only for gaps > 5.
        if first - end > 5
            && phoneme_intervals
                .last()
                .is_none_or(|value: &Interval| value.word != Some(word))
        {
            phoneme_intervals.push(Interval {
                start: end,
                end: first,
                label: 1,
                word: Some(word),
            });
            word_intervals.push(Interval {
                start: end,
                end: first,
                label: -1,
                word: None,
            });
            end = first;
        }
        phoneme_intervals.push(Interval {
            start: end,
            end: last + 1,
            label,
            word: Some(word),
        });
        if word_intervals
            .last()
            .is_none_or(|value| value.label != word)
        {
            word_intervals.push(Interval {
                start: end,
                end: last + 1,
                label: word,
                word: None,
            });
        } else if let Some(interval) = word_intervals.last_mut() {
            interval.end = last + 1;
        }
        end = last + 1;
    }
    if end != frames {
        let word = words.last().copied().unwrap_or(0) + 1;
        phoneme_intervals.push(Interval {
            start: end,
            end: frames,
            label: 1,
            word: Some(word),
        });
        word_intervals.push(Interval {
            start: end,
            end: frames,
            label: -1,
            word: None,
        });
    }
    let mut mel_to_phoneme = vec![0_i64; frames];
    for (index, interval) in phoneme_intervals.iter().enumerate() {
        mel_to_phoneme[interval.start..interval.end].fill(index as i64 + 1);
    }
    let mut mel_to_word = vec![0_i64; frames];
    for (index, interval) in word_intervals.iter().enumerate() {
        mel_to_word[interval.start..interval.end].fill(index as i64 + 1);
    }
    let boundaries = |mapping: &[i64]| {
        let mut result = vec![0_i64; mapping.len()];
        for index in 1..mapping.len() {
            result[index] = i64::from(mapping[index] - mapping[index - 1] == 1);
        }
        result
    };
    Ok(Alignment {
        phoneme_boundary: boundaries(&mel_to_phoneme),
        word_boundary: boundaries(&mel_to_word),
        mel_to_phoneme,
        mel_to_word,
        phoneme_intervals,
        word_intervals,
        dp_last: dp.pop().unwrap(),
    })
}

fn round_half_to_even_average(left: usize, right: usize) -> usize {
    let sum = left + right;
    let floor = sum / 2;
    if sum.is_multiple_of(2) || floor.is_multiple_of(2) {
        floor
    } else {
        floor + 1
    }
}

/// Reproduce the pinned upstream note-boundary peak regulator natively.
/// Reference-boundary correction is intentionally not included because the
/// Chinese inference path calls this stage with no reference boundaries.
pub fn regulate_boundaries(
    logits: &[f32],
    threshold: f32,
    minimum_gap: usize,
    valid_frames: usize,
) -> Result<Vec<i64>, String> {
    if logits.is_empty()
        || !threshold.is_finite()
        || !(0.0..=1.0).contains(&threshold)
        || minimum_gap == 0
        || valid_frames == 0
        || valid_frames > logits.len()
        || logits.iter().any(|value| !value.is_finite())
    {
        return Err("STARS boundary-regulator input is invalid".to_string());
    }
    let active = logits
        .iter()
        .map(|value| sigmoid(*value) > threshold)
        .collect::<Vec<_>>();
    let mut result = vec![0_i64; logits.len()];
    let mut last_boundary: Option<usize> = None;
    let mut start = None;
    for (index, is_active) in active.iter().copied().enumerate() {
        if is_active {
            if start.is_none() {
                start = Some(index);
            }
            continue;
        }
        let Some(run_start) = start.take() else {
            continue;
        };
        let mut boundary = run_start;
        if index - 1 > run_start {
            boundary = (run_start..index)
                .max_by(|left, right| logits[*left].total_cmp(&logits[*right]))
                .unwrap();
        }
        if let Some(last) = last_boundary
            && boundary - last < minimum_gap
            && last > 0
        {
            boundary = round_half_to_even_average(boundary, last);
            result[last] = 0;
        }
        result[boundary] = 1;
        last_boundary = Some(boundary);
    }
    result[0] = 0;
    result[valid_frames - 1..].fill(0);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Fixture {
        prediction: Vec<Vec<f32>>,
        effective_boundary_input: Vec<f32>,
        labels: Vec<i64>,
        ph2word: Vec<i64>,
        mel2ph: Vec<i64>,
        ph_boundary: Vec<i64>,
        mel2word: Vec<i64>,
        word_boundary: Vec<i64>,
        dp_last: Vec<f32>,
    }

    #[derive(Deserialize)]
    struct BoundaryFixture {
        logits: Vec<f32>,
        nonpadding: Vec<f32>,
        threshold: f32,
        min_gap: usize,
        output: Vec<i64>,
    }

    #[test]
    fn native_boundary_regulator_matches_pinned_upstream() {
        let fixture: BoundaryFixture =
            serde_json::from_str(include_str!("../fixtures/stars-boundary-upstream.json")).unwrap();
        let valid = fixture
            .nonpadding
            .iter()
            .take_while(|value| **value > 0.0)
            .count();
        let actual =
            regulate_boundaries(&fixture.logits, fixture.threshold, fixture.min_gap, valid)
                .unwrap();
        assert_eq!(actual, fixture.output);
    }

    #[test]
    fn native_viterbi_matches_pinned_upstream_double_sigmoid_path() {
        let fixture: Fixture =
            serde_json::from_str(include_str!("../fixtures/stars-viterbi-upstream.json")).unwrap();
        let classes = fixture.prediction[0].len();
        let prediction = fixture.prediction.into_iter().flatten().collect::<Vec<_>>();
        let actual = align(
            &prediction,
            classes,
            &fixture.effective_boundary_input,
            &fixture.labels,
            &fixture.ph2word,
        )
        .unwrap();
        assert_eq!(actual.mel_to_phoneme, fixture.mel2ph);
        assert_eq!(actual.phoneme_boundary, fixture.ph_boundary);
        assert_eq!(actual.mel_to_word, fixture.mel2word);
        assert_eq!(actual.word_boundary, fixture.word_boundary);
        for (actual, expected) in actual.dp_last.iter().zip(fixture.dp_last) {
            assert!((actual - expected).abs() < 2.0e-5, "{actual} != {expected}");
        }
    }
}
