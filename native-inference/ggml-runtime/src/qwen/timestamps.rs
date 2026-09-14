//! Qwen Forced Aligner timestamp post-processing.

/// Maximum-likelihood nondecreasing timestamp sequence from the complete head.
/// Row normalization constants do not affect the optimum, so summing logits is
/// equivalent to summing log probabilities. Equal timestamps remain legal:
/// no positive word duration or regularly spaced timestamps are fabricated.
/// Complexity is O(rows * classes), with one backpointer per classifier cell.
/// Ties prefer the later class, matching the independent classifier's argmax.
/// The original independent peaks remain separate diagnostic evidence.
pub fn ordered_timestamp_ms(
    logits: &[f32],
    classes: usize,
    period_ms: u64,
) -> Result<Vec<u64>, String> {
    if classes == 0 || period_ms == 0 || !logits.len().is_multiple_of(classes) {
        return Err("Qwen timestamp classifier geometry is invalid".into());
    }
    if logits.iter().any(|value| !value.is_finite()) {
        return Err("Qwen timestamp classifier contains nonfinite logits".into());
    }
    if logits.is_empty() {
        return Ok(Vec::new());
    }
    let rows = logits.len() / classes;
    let mut parent = vec![0_usize; logits.len()];
    let mut previous = logits[..classes]
        .iter()
        .map(|value| f64::from(*value))
        .collect::<Vec<_>>();
    let mut current = vec![0.0_f64; classes];
    for row in 1..rows {
        let mut best = 0;
        for class in 0..classes {
            if previous[class] >= previous[best] {
                best = class;
            }
            current[class] = previous[best] + f64::from(logits[row * classes + class]);
            parent[row * classes + class] = best;
        }
        std::mem::swap(&mut previous, &mut current);
    }
    let mut best = previous
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .expect("nonempty classifier")
        .0;
    let mut result = vec![0; rows];
    for row in (0..rows).rev() {
        result[row] = u64::try_from(best)
            .ok()
            .and_then(|class| class.checked_mul(period_ms))
            .ok_or("Qwen ordered timestamp overflows")?;
        best = parent[row * classes + best];
    }
    Ok(result)
}

/// Official longest-nondecreasing-subsequence tie-breaking and anomaly
/// correction in milliseconds. Raw classifier predictions remain separately
/// available; interpolation can produce positions between the 80 ms classes.
pub fn correct_timestamp_ms(data: &[u64]) -> Vec<u64> {
    let count = data.len();
    if count == 0 {
        return Vec::new();
    }
    let mut length = vec![1_usize; count];
    let mut parent = vec![None; count];
    let mut best = 0;
    for index in 1..count {
        for earlier in 0..index {
            if data[earlier] <= data[index] && length[earlier] + 1 > length[index] {
                length[index] = length[earlier] + 1;
                parent[index] = Some(earlier);
            }
        }
        if length[index] > length[best] {
            best = index;
        }
    }
    let mut normal = vec![false; count];
    let mut cursor = Some(best);
    while let Some(index) = cursor {
        normal[index] = true;
        cursor = parent[index];
    }

    let mut result = data.to_vec();
    let mut index = 0;
    while index < count {
        if normal[index] {
            index += 1;
            continue;
        }
        let mut end = index;
        while end < count && !normal[end] {
            end += 1;
        }
        let left = index.checked_sub(1).map(|at| result[at]);
        let right = (end < count).then(|| result[end]);
        for at in index..end {
            result[at] = match (left, right) {
                (None, Some(right)) => right,
                (Some(left), None) => left,
                (Some(left), Some(right)) if end - index <= 2 => {
                    if at - index + 1 <= end - at {
                        left
                    } else {
                        right
                    }
                }
                (Some(left), Some(right)) => {
                    (left as f64
                        + (right - left) as f64 / (end - index + 1) as f64
                            * (at - index + 1) as f64) as u64
                }
                (None, None) => data[at],
            };
        }
        index = end;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_decoding_uses_secondary_acoustic_peaks_instead_of_interpolation() {
        let logits = [
            0.0, 8.0, 0.0, 0.0, 0.0, 0.0, 7.0, 8.0, 0.0, 9.0, 8.0, 0.0, 0.0, 0.0, 0.0, 9.0,
        ];
        assert_eq!(
            ordered_timestamp_ms(&logits, 4, 80).unwrap(),
            [80, 160, 160, 240]
        );
    }

    #[test]
    fn ordered_decoding_does_not_force_words_into_silence_or_break_ties_into_durations() {
        assert_eq!(
            ordered_timestamp_ms(&[4.0, 0.0, 0.0, 4.0, 0.0, 0.0], 3, 80).unwrap(),
            [0, 0]
        );
        assert_eq!(ordered_timestamp_ms(&[0.0; 12], 3, 80).unwrap(), [160; 4]);
        assert_eq!(ordered_timestamp_ms(&[], 3, 80).unwrap(), Vec::<u64>::new());
    }

    #[test]
    fn ordered_decoding_rejects_invalid_shapes_and_nonfinite_evidence() {
        assert!(ordered_timestamp_ms(&[1.0], 0, 80).is_err());
        assert!(ordered_timestamp_ms(&[1.0], 2, 80).is_err());
        assert!(ordered_timestamp_ms(&[1.0], 1, 0).is_err());
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(ordered_timestamp_ms(&[value], 1, 80).is_err());
        }
        assert!(ordered_timestamp_ms(&[0.0, 1.0, 2.0], 3, u64::MAX).is_err());
    }

    #[test]
    fn ordered_decoding_matches_exhaustive_global_optima() {
        for seed in 0..64 {
            let logits = (0..12)
                .map(|index| ((index * 17 + seed * 13 + index * seed) % 11) as f32 - 5.0)
                .collect::<Vec<_>>();
            let decoded = ordered_timestamp_ms(&logits, 3, 1).unwrap();
            assert!(decoded.windows(2).all(|pair| pair[0] <= pair[1]));
            let actual = decoded
                .iter()
                .enumerate()
                .map(|(row, class)| logits[row * 3 + *class as usize])
                .sum::<f32>();
            let mut best = f32::NEG_INFINITY;
            for first in 0..3 {
                for second in first..3 {
                    for third in second..3 {
                        for fourth in third..3 {
                            best = best.max(
                                logits[first]
                                    + logits[3 + second]
                                    + logits[6 + third]
                                    + logits[9 + fourth],
                            );
                        }
                    }
                }
            }
            assert_eq!(actual, best, "seed {seed}");
        }
    }

    #[test]
    fn correction_keeps_first_lnds_ties_and_endpoint_runs() {
        assert_eq!(correct_timestamp_ms(&[]), Vec::<u64>::new());
        assert_eq!(
            correct_timestamp_ms(&[80, 320, 160, 240]),
            [80, 80, 160, 240]
        );
        assert_eq!(correct_timestamp_ms(&[320, 240, 160, 80]), [320; 4]);
        assert_eq!(
            correct_timestamp_ms(&[0, 800, 720, 400, 480]),
            [0, 0, 400, 400, 480]
        );
    }

    #[test]
    fn correction_interpolates_milliseconds_not_truncated_classes() {
        assert_eq!(
            correct_timestamp_ms(&[0, 800, 720, 640, 400, 480, 560]),
            [0, 100, 200, 300, 400, 480, 560]
        );
        assert_eq!(correct_timestamp_ms(&[0, 0, 80, 240]), [0, 0, 80, 240]);
    }
}
