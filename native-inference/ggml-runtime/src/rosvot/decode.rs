use super::model::PITCH_CLASSES;

pub const NOTE_START: usize = 30;
pub const NOTE_END: usize = 85;

#[derive(Debug, Clone, PartialEq)]
pub struct AggregatedNotes {
    /// Note-major `[notes, hidden]` attention-weighted features.
    pub features: Vec<f32>,
    pub count: usize,
    pub frame_to_note: Vec<usize>,
}

fn sigmoid(value: f32) -> f32 {
    1.0 / (1.0 + (-value).exp())
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

/// Regulates frame boundary logits while preserving transcript word starts.
pub fn regulate_boundaries(
    logits: &[f32],
    threshold: f32,
    minimum_gap: usize,
    reference: &[i32],
    reference_minimum_gap: usize,
    valid_frames: usize,
) -> Result<Vec<i32>, String> {
    if logits.is_empty()
        || reference.len() != logits.len()
        || valid_frames == 0
        || valid_frames > logits.len()
        || minimum_gap == 0
        || !threshold.is_finite()
        || !(0.0..=1.0).contains(&threshold)
        || logits.iter().any(|value| !value.is_finite())
        || reference.iter().any(|value| !matches!(value, 0 | 1))
    {
        return Err("ROSVOT boundary regulator input is invalid".to_string());
    }
    let probabilities = logits
        .iter()
        .map(|value| sigmoid(*value))
        .collect::<Vec<_>>();
    let mut result = vec![0_i32; logits.len()];
    let mut last_boundary: isize = -1;
    let mut start: isize = -1;
    for index in 0..logits.len() {
        if probabilities[index] > threshold {
            if start < 0 {
                start = index as isize;
            }
        } else if start >= 0 {
            let run_start = start as usize;
            let mut boundary = if index - 1 > run_start {
                (run_start..index)
                    .max_by(|left, right| probabilities[*left].total_cmp(&probabilities[*right]))
                    .unwrap()
            } else {
                run_start
            };
            if boundary as isize - last_boundary < minimum_gap as isize && last_boundary > 0 {
                let previous = last_boundary as usize;
                boundary = round_half_to_even_average(boundary, previous);
                result[previous] = 0;
            }
            result[boundary] = 1;
            last_boundary = boundary as isize;
            start = -1;
        }
    }

    if reference_minimum_gap > 0 {
        for index in 0..reference.len() {
            if reference[index] != 1 {
                continue;
            }
            let start = index.saturating_sub(reference_minimum_gap);
            let end = (index + reference_minimum_gap).min(result.len());
            let count = result[start..end].iter().sum::<i32>();
            if count == 0 {
                result[index] = 1;
            } else if count == 1 && result[index] != 1 {
                result[start..end].copy_from_slice(&reference[start..end]);
            } else if count > 1 {
                for distance in 1..=reference_minimum_gap {
                    let left = index.saturating_sub(distance);
                    if result[left] == 1 && reference[left] != 1 {
                        result[left] = 0;
                        break;
                    }
                    let right = (index + distance).min(result.len() - 1);
                    if result[right] == 1 && reference[right] != 1 {
                        result[right] = 0;
                        break;
                    }
                }
                result[index] = 1;
            }
        }
        if reference
            .iter()
            .enumerate()
            .any(|(index, value)| *value == 1 && result[index] != 1)
        {
            return Err("ROSVOT regulation failed to preserve transcript boundaries".to_string());
        }
    }
    result[0] = 0;
    result[valid_frames - 1..].fill(0);
    Ok(result)
}

/// Aggregates frame features into dynamic notes using the model's attention.
pub fn aggregate_notes(
    weighted_features: &[f32],
    attention: &[f32],
    boundaries: &[i32],
    hidden: usize,
    valid_frames: usize,
) -> Result<AggregatedNotes, String> {
    if hidden == 0
        || valid_frames == 0
        || valid_frames > attention.len()
        || boundaries.len() != attention.len()
        || weighted_features.len() != attention.len() * hidden
        || attention
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        || weighted_features.iter().any(|value| !value.is_finite())
        || boundaries.iter().any(|value| !matches!(value, 0 | 1))
    {
        return Err("ROSVOT note aggregation input is invalid".to_string());
    }
    let mut frame_to_note = Vec::with_capacity(valid_frames);
    let mut note = 0_usize;
    for boundary in &boundaries[..valid_frames] {
        note += usize::try_from(*boundary).unwrap();
        frame_to_note.push(note);
    }
    let count = note + 1;
    let mut denominator = vec![0.0_f32; count];
    let mut features = vec![0.0_f32; count * hidden];
    for frame in 0..valid_frames {
        let note = frame_to_note[frame];
        denominator[note] += attention[frame];
        for channel in 0..hidden {
            features[note * hidden + channel] += weighted_features[frame * hidden + channel];
        }
    }
    for note in 0..count {
        let scale = denominator[note] + 1.0e-5;
        for channel in 0..hidden {
            features[note * hidden + channel] /= scale;
        }
    }
    Ok(AggregatedNotes {
        features,
        count,
        frame_to_note,
    })
}

pub fn decode_pitch(logits: &[f32], notes: usize) -> Result<Vec<Option<u8>>, String> {
    if notes == 0
        || logits.len() != notes * PITCH_CLASSES
        || logits.iter().any(|value| !value.is_finite())
    {
        return Err("ROSVOT pitch logits are invalid".to_string());
    }
    Ok(logits
        .chunks_exact(PITCH_CLASSES)
        .map(|row| {
            let class = row
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .unwrap()
                .0;
            (NOTE_START..=NOTE_END)
                .contains(&class)
                .then_some(class as u8)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pitch_decoder_keeps_only_the_upstream_midi_range() {
        let mut logits = vec![-10.0; 2 * PITCH_CLASSES];
        logits[60] = 10.0;
        logits[PITCH_CLASSES + 10] = 10.0;
        assert_eq!(decode_pitch(&logits, 2).unwrap(), vec![Some(60), None]);
    }

    #[test]
    fn transcript_boundaries_are_preserved() {
        let mut reference = vec![0; 16];
        reference[7] = 1;
        let mut logits = vec![-8.0; 16];
        logits[5] = 8.0;
        let actual = regulate_boundaries(&logits, 0.85, 4, &reference, 2, 16).unwrap();
        assert_eq!(actual[7], 1);
        assert_eq!(actual[5], 0);
    }

    #[test]
    fn note_aggregation_uses_frame_attention() {
        let weighted = vec![2.0, 4.0, 3.0, 6.0, 10.0, 20.0];
        let attention = vec![1.0, 2.0, 5.0];
        let boundaries = vec![0, 0, 1];
        let actual = aggregate_notes(&weighted, &attention, &boundaries, 2, 3).unwrap();
        assert_eq!(actual.count, 2);
        assert_eq!(actual.frame_to_note, [0, 0, 1]);
        assert!((actual.features[0] - 5.0 / 3.00001).abs() < 1.0e-5);
        assert!((actual.features[2] - 10.0 / 5.00001).abs() < 1.0e-5);
    }
}
