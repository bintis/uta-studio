#[derive(Debug, Clone, PartialEq)]
pub struct GaussianBlurredResult {
    pub values: Vec<f32>,
    pub presence: Vec<u8>,
}

pub fn boundaries_to_regions(boundaries: &[u8], mask: Option<&[u8]>) -> Result<Vec<i32>, String> {
    validate_optional_length("mask", mask, boundaries.len())?;
    let mut regions = vec![0_i32; boundaries.len()];
    let mut running = 1_i32;
    for (index, boundary) in boundaries.iter().enumerate() {
        if *boundary != 0 {
            running = running
                .checked_add(1)
                .ok_or_else(|| "GAME region id overflowed i32".to_string())?;
        }
        regions[index] = if mask.is_none_or(|values| values[index] != 0) {
            running
        } else {
            0
        };
    }
    Ok(regions)
}

pub fn decode_soft_boundaries(
    probabilities: &[f32],
    barriers: Option<&[u8]>,
    mask: Option<&[u8]>,
    threshold: f32,
    radius: usize,
) -> Result<Vec<u8>, String> {
    validate_optional_length("barriers", barriers, probabilities.len())?;
    validate_optional_length("mask", mask, probabilities.len())?;
    if !threshold.is_finite() || probabilities.iter().any(|value| !value.is_finite()) {
        return Err("GAME boundary probabilities and threshold must be finite".to_string());
    }
    if probabilities.is_empty() {
        return Ok(Vec::new());
    }

    let values = probabilities
        .iter()
        .enumerate()
        .map(|(index, value)| {
            if mask.is_some_and(|values| values[index] == 0)
                || barriers.is_some_and(|values| values[index] != 0)
            {
                f32::INFINITY
            } else {
                *value
            }
        })
        .collect::<Vec<_>>();
    let mut decoded = vec![0_u8; probabilities.len()];
    for index in 0..probabilities.len() {
        let start = index.saturating_sub(radius);
        let end = (index + radius).min(probabilities.len() - 1);
        let mut argmax = start;
        for candidate in start + 1..=end {
            if values[candidate] > values[argmax] {
                argmax = candidate;
            }
        }
        let barrier = barriers.is_some_and(|values| values[index] != 0);
        if (argmax == index || barrier) && (barrier || probabilities[index] >= threshold) {
            decoded[index] = 1;
        }
        if mask.is_some_and(|values| values[index] == 0) {
            decoded[index] = 0;
        }
    }
    Ok(decoded)
}

pub fn decode_gaussian_blurred_probs(
    probabilities: &[f32],
    rows: usize,
    bins: usize,
    minimum: f32,
    maximum: f32,
    deviation: f32,
    threshold: f32,
) -> Result<GaussianBlurredResult, String> {
    let expected = rows
        .checked_mul(bins)
        .ok_or_else(|| "GAME pitch probability shape overflow".to_string())?;
    if probabilities.len() != expected {
        return Err(format!(
            "GAME pitch probability length mismatch: expected {expected}, got {}",
            probabilities.len()
        ));
    }
    if bins == 0 {
        return Err("GAME pitch decoding requires at least one bin".to_string());
    }
    if rows > 0 && bins < 2 {
        return Err("GAME pitch decoding requires at least two bins".to_string());
    }
    if [minimum, maximum, deviation, threshold]
        .iter()
        .any(|value| !value.is_finite())
        || maximum <= minimum
        || deviation < 0.0
        || probabilities.iter().any(|value| !value.is_finite())
    {
        return Err("GAME pitch decoding parameters are invalid".to_string());
    }
    if rows == 0 {
        return Ok(GaussianBlurredResult {
            values: Vec::new(),
            presence: Vec::new(),
        });
    }

    let step = (maximum - minimum) / (bins - 1) as f32;
    let width = (deviation / step).ceil() as usize;
    let mut values = Vec::with_capacity(rows);
    let mut presence = Vec::with_capacity(rows);
    for row in probabilities.chunks_exact(bins) {
        let mut argmax = 0;
        for index in 1..bins {
            if row[index] > row[argmax] {
                argmax = index;
            }
        }
        let start = argmax.saturating_sub(width);
        let end = (argmax + width + 1).min(bins);
        let mut weight_sum = 0.0_f32;
        let mut value_sum = 0.0_f32;
        for index in start..end {
            weight_sum += row[index];
            value_sum += row[index] * (minimum + step * index as f32);
        }
        values.push(value_sum / (weight_sum + 1.0e-8));
        presence.push(u8::from(row[argmax] >= threshold));
    }
    Ok(GaussianBlurredResult { values, presence })
}

fn validate_optional_length(
    label: &str,
    values: Option<&[u8]>,
    expected: usize,
) -> Result<(), String> {
    if let Some(values) = values
        && values.len() != expected
    {
        return Err(format!(
            "GAME {label} length mismatch: expected {expected}, got {}",
            values.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_keep_counting_across_masked_frames() {
        assert_eq!(
            boundaries_to_regions(&[0, 1, 1, 0], Some(&[1, 1, 0, 1])).unwrap(),
            vec![1, 2, 0, 3]
        );
    }

    #[test]
    fn boundary_decode_preserves_leftmost_ties_and_barriers() {
        assert_eq!(
            decode_soft_boundaries(
                &[0.4, 0.4, 0.1, 0.7, 0.6],
                Some(&[0, 0, 1, 0, 0]),
                Some(&[1, 1, 0, 1, 1]),
                0.3,
                1,
            )
            .unwrap(),
            vec![1, 0, 0, 0, 0]
        );
    }

    #[test]
    fn gaussian_decode_matches_game_weighting() {
        let decoded = decode_gaussian_blurred_probs(
            &[
                0.0, 0.2, 0.6, 0.3, 0.0, //
                0.4, 0.3, 0.2, 0.1, 0.0,
            ],
            2,
            5,
            60.0,
            64.0,
            1.0,
            0.5,
        )
        .unwrap();
        assert!((decoded.values[0] - 62.09091).abs() < 1.0e-4);
        assert!((decoded.values[1] - 60.42857).abs() < 1.0e-4);
        assert_eq!(decoded.presence, vec![1, 0]);
    }

    #[test]
    fn non_finite_probabilities_fail_closed() {
        assert!(
            decode_soft_boundaries(&[f32::NAN], None, None, 0.2, 2)
                .unwrap_err()
                .contains("finite")
        );
    }
}
