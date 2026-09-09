//! Qwen Forced Aligner timestamp post-processing.

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
