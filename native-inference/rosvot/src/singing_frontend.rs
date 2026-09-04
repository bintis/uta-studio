use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

pub const SAMPLE_RATE: usize = 24_000;
pub const FFT_SIZE: usize = 512;
pub const HOP_SIZE: usize = 128;
pub const MEL_BINS: usize = 80;
pub const ROSVOT_MEL_BINS: usize = 40;
pub const PROFILE: &str = "shared-singing-frontend-24k-v1";
pub const ANNOTATION_RMVPE_SHA256: &str =
    "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2";

#[derive(Debug, Clone, PartialEq)]
pub struct AnnotationPitch {
    pub f0_hz: Vec<f32>,
    /// One means unvoiced, matching the pinned STARS/ROSVOT helpers.
    pub uv: Vec<i64>,
    pub pitch_coarse: Vec<i64>,
}

#[derive(Debug, Clone)]
struct MelBand {
    weights: Vec<(usize, f32)>,
}

fn hz_to_slaney_mel(hz: f64) -> f64 {
    const F_MIN: f64 = 0.0;
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    const MIN_LOG_MEL: f64 = (MIN_LOG_HZ - F_MIN) / F_SP;
    let linear = (hz - F_MIN) / F_SP;
    if hz < MIN_LOG_HZ {
        linear
    } else {
        MIN_LOG_MEL + (hz / MIN_LOG_HZ).ln() / (6.4_f64.ln() / 27.0)
    }
}

fn slaney_mel_to_hz(mel: f64) -> f64 {
    const F_MIN: f64 = 0.0;
    const F_SP: f64 = 200.0 / 3.0;
    const MIN_LOG_HZ: f64 = 1_000.0;
    const MIN_LOG_MEL: f64 = (MIN_LOG_HZ - F_MIN) / F_SP;
    if mel < MIN_LOG_MEL {
        F_MIN + F_SP * mel
    } else {
        MIN_LOG_HZ * ((6.4_f64.ln() / 27.0) * (mel - MIN_LOG_MEL)).exp()
    }
}

fn mel_bands() -> Vec<MelBand> {
    let minimum = hz_to_slaney_mel(30.0);
    let maximum = hz_to_slaney_mel(12_000.0);
    let frequencies = (0..MEL_BINS + 2)
        .map(|index| {
            let fraction = index as f64 / (MEL_BINS + 1) as f64;
            slaney_mel_to_hz(minimum + fraction * (maximum - minimum))
        })
        .collect::<Vec<_>>();
    (0..MEL_BINS)
        .map(|band| {
            let lower = frequencies[band];
            let center = frequencies[band + 1];
            let upper = frequencies[band + 2];
            let normalization = 2.0 / (upper - lower);
            let weights = (0..=FFT_SIZE / 2)
                .filter_map(|bin| {
                    let frequency = SAMPLE_RATE as f64 * bin as f64 / FFT_SIZE as f64;
                    let lower_slope = (frequency - lower) / (center - lower);
                    let upper_slope = (upper - frequency) / (upper - center);
                    let weight = lower_slope.min(upper_slope).max(0.0) * normalization;
                    (weight > 0.0).then_some((bin, weight as f32))
                })
                .collect();
            MelBand { weights }
        })
        .collect()
}

fn reflected_sample(audio: &[f32], padded_index: usize, left_pad: usize) -> f32 {
    if padded_index < left_pad {
        return audio[left_pad - padded_index];
    }
    let index = padded_index - left_pad;
    if index < audio.len() {
        return audio[index];
    }
    audio[audio.len() - 2 - (index - audio.len())]
}

/// Pinned STARS/ROSVOT `MelNet`: 24 kHz, 512 Hann, hop 128, Slaney mel,
/// magnitude `sqrt(re²+im²+1e-9)`, and log10 compression.
pub fn mel_80(audio: &[f32]) -> Result<(Vec<f32>, usize), String> {
    let pad = (FFT_SIZE - HOP_SIZE) / 2;
    if audio.len() <= pad {
        return Err("shared 24 kHz mel requires more than 8 ms of audio".to_string());
    }
    if audio.iter().any(|value| !value.is_finite()) {
        return Err("shared 24 kHz mel input contains non-finite samples".to_string());
    }
    let frame_count = audio.len().div_ceil(HOP_SIZE);
    let bands = mel_bands();
    let window = (0..FFT_SIZE)
        .map(|index| {
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / FFT_SIZE as f32).cos()
        })
        .collect::<Vec<_>>();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let mut buffer = vec![Complex32::default(); FFT_SIZE];
    let mut magnitude = vec![0.0_f32; FFT_SIZE / 2 + 1];
    let mut output = vec![0.0_f32; frame_count * MEL_BINS];
    for frame in 0..frame_count {
        let start = frame * HOP_SIZE;
        for index in 0..FFT_SIZE {
            let sample = reflected_sample(audio, start + index, pad).clamp(-1.0, 1.0);
            buffer[index] = Complex32::new(sample * window[index], 0.0);
        }
        fft.process(&mut buffer);
        for (value, complex) in magnitude.iter_mut().zip(&buffer[..=FFT_SIZE / 2]) {
            *value = (complex.norm_sqr() + 1.0e-9).sqrt();
        }
        for (band_index, band) in bands.iter().enumerate() {
            let mel = band
                .weights
                .iter()
                .map(|(bin, weight)| magnitude[*bin] * weight)
                .sum::<f32>();
            output[frame * MEL_BINS + band_index] = mel.max(1.0e-5).log10();
        }
    }
    Ok((output, frame_count))
}

pub fn rosvot_mel_prefix(mel: &[f32], frames: usize) -> Result<Vec<f32>, String> {
    if mel.len() != frames * MEL_BINS {
        return Err("shared mel shape is invalid".to_string());
    }
    let mut result = Vec::with_capacity(frames * ROSVOT_MEL_BINS);
    for row in mel.chunks_exact(MEL_BINS) {
        result.extend_from_slice(&row[..ROSVOT_MEL_BINS]);
    }
    Ok(result)
}

fn resample_curve(
    points: &[f32],
    target_step: f64,
    target_length: usize,
) -> Result<Vec<f32>, String> {
    if points.is_empty() || target_length == 0 || !target_step.is_finite() || target_step <= 0.0 {
        return Err("annotation RMVPE resampling input is invalid".to_string());
    }
    let maximum = (points.len() - 1) as f64 * 0.01;
    let generated = (maximum / target_step).ceil() as usize;
    let mut result = Vec::with_capacity(target_length);
    for index in 0..generated.min(target_length) {
        let position = index as f64 * target_step / 0.01;
        let left = (position.floor() as usize).min(points.len() - 1);
        let right = (left + 1).min(points.len() - 1);
        let fraction = (position - left as f64) as f32;
        result.push(points[left] + (points[right] - points[left]) * fraction);
    }
    let fill = result.last().copied().unwrap_or(points[0]);
    result.resize(target_length, fill);
    Ok(result)
}

fn interpolate_unvoiced_log_f0(f0: &[f32], uv: &[bool]) -> Vec<f32> {
    let voiced = uv
        .iter()
        .enumerate()
        .filter_map(|(index, unvoiced)| (!*unvoiced).then_some(index))
        .collect::<Vec<_>>();
    if voiced.is_empty() {
        return vec![0.0; f0.len()];
    }
    let mut result = f0
        .iter()
        .zip(uv)
        .map(|(value, unvoiced)| {
            if *unvoiced {
                0.0
            } else {
                (value + 1.0e-8).log2()
            }
        })
        .collect::<Vec<_>>();
    for index in 0..result.len() {
        if !uv[index] {
            continue;
        }
        let right_position = voiced.partition_point(|value| *value < index);
        let right = voiced.get(right_position).copied();
        let left = right_position
            .checked_sub(1)
            .and_then(|value| voiced.get(value))
            .copied();
        result[index] = match (left, right) {
            (Some(left), Some(right)) if right != left => {
                let fraction = (index - left) as f32 / (right - left) as f32;
                result[left] + (result[right] - result[left]) * fraction
            }
            (Some(left), _) => result[left],
            (_, Some(right)) => result[right],
            _ => 0.0,
        };
    }
    result
}

fn f0_to_coarse(f0: f32) -> i64 {
    let mel_min = 1_127.0_f32 * (1.0 + 50.0_f32 / 700.0).ln();
    let mel_max = 1_127.0_f32 * (1.0 + 900.0_f32 / 700.0).ln();
    let mel = 1_127.0_f32 * (1.0 + f0 / 700.0).ln();
    let scaled = if mel > 0.0 {
        (mel - mel_min) * 254.0 / (mel_max - mel_min) + 1.0
    } else {
        1.0
    };
    scaled.clamp(1.0, 255.0).round_ties_even() as i64
}

/// Convert raw 10 ms salience-decoded F0 from the exact annotation RMVPE into
/// the shared 24 kHz frame generation consumed by both STARS and ROSVOT.
pub fn annotation_pitch(raw_f0: &[f32], target_frames: usize) -> Result<AnnotationPitch, String> {
    if raw_f0.len() < 2
        || target_frames == 0
        || raw_f0
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err("annotation RMVPE F0 input is invalid".to_string());
    }
    let mut cleaned = raw_f0.to_vec();
    for value in &mut cleaned {
        if *value < 50.0 || *value > 900.0 {
            *value = 0.0;
        }
    }
    const MIN_GAP: usize = 6;
    for index in 0..cleaned.len().saturating_sub(MIN_GAP + 1) {
        if cleaned[index] == 0.0
            && cleaned[index + MIN_GAP + 1] == 0.0
            && cleaned[index..index + MIN_GAP + 2].iter().sum::<f32>() > 0.0
        {
            cleaned[index..index + MIN_GAP + 2].fill(0.0);
        }
    }
    let uv_source = cleaned
        .iter()
        .map(|value| f32::from(*value == 0.0))
        .collect::<Vec<_>>();
    let mut f0 = resample_curve(
        &cleaned,
        HOP_SIZE as f64 / SAMPLE_RATE as f64,
        target_frames,
    )?;
    let uv_values = resample_curve(
        &uv_source,
        HOP_SIZE as f64 / SAMPLE_RATE as f64,
        target_frames,
    )?;
    let uv_bool = uv_values
        .iter()
        .map(|value| *value > 0.5)
        .collect::<Vec<_>>();
    for (value, unvoiced) in f0.iter_mut().zip(&uv_bool) {
        if *unvoiced {
            *value = 0.0;
        }
    }
    let normalized = interpolate_unvoiced_log_f0(&f0, &uv_bool);
    let f0_hz = normalized
        .iter()
        .zip(&uv_bool)
        .map(|(value, unvoiced)| {
            if *unvoiced {
                0.0
            } else {
                2.0_f32.powf(*value).clamp(50.0, 900.0)
            }
        })
        .collect::<Vec<_>>();
    let uv = uv_bool
        .iter()
        .map(|value| i64::from(*value))
        .collect::<Vec<_>>();
    let pitch_coarse = f0_hz.iter().map(|value| f0_to_coarse(*value)).collect();
    Ok(AnnotationPitch {
        f0_hz,
        uv,
        pitch_coarse,
    })
}

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    use super::*;

    #[derive(Deserialize)]
    struct Fixture {
        profile: String,
        audio: Vec<f32>,
        mel_frames: usize,
        mel_bins: usize,
        mel: Vec<Vec<f32>>,
        raw_rmvpe_f0: Vec<f32>,
        annotation_target_frames: usize,
        annotation_f0: Vec<f32>,
        annotation_uv: Vec<i64>,
        annotation_pitch_coarse: Vec<i64>,
    }

    fn fixture() -> Fixture {
        serde_json::from_str(include_str!(
            "../fixtures/shared-singing-frontend-upstream.json"
        ))
        .unwrap()
    }

    #[test]
    fn shared_mel_matches_the_identical_stars_and_rosvot_upstream_frontend() {
        let expected = fixture();
        assert_eq!(expected.profile, PROFILE);
        assert_eq!(expected.mel_bins, MEL_BINS);
        let (actual, frames) = mel_80(&expected.audio).unwrap();
        assert_eq!(frames, expected.mel_frames);
        let expected = expected.mel.into_iter().flatten().collect::<Vec<_>>();
        assert_eq!(actual.len(), expected.len());
        let maximum = actual
            .iter()
            .zip(expected)
            .enumerate()
            .map(|(index, (left, right))| (index, *left, right, (*left - right).abs()))
            .max_by(|left, right| left.3.total_cmp(&right.3))
            .unwrap();
        assert!(
            maximum.3 < 5.0e-4,
            "maximum mel error {} at {}: {} != {}",
            maximum.3,
            maximum.0,
            maximum.1,
            maximum.2
        );
    }

    #[test]
    fn exact_annotation_rmvpe_adapter_matches_upstream_helpers() {
        let expected = fixture();
        let actual =
            annotation_pitch(&expected.raw_rmvpe_f0, expected.annotation_target_frames).unwrap();
        assert_eq!(actual.uv, expected.annotation_uv);
        assert_eq!(actual.pitch_coarse, expected.annotation_pitch_coarse);
        for (actual, expected) in actual.f0_hz.iter().zip(expected.annotation_f0) {
            assert!((actual - expected).abs() < 2.0e-4, "{actual} != {expected}");
        }
    }

    #[test]
    fn rosvot_consumes_the_first_forty_bins_of_the_shared_generation() {
        let mel = (0..MEL_BINS * 3)
            .map(|value| value as f32)
            .collect::<Vec<_>>();
        let actual = rosvot_mel_prefix(&mel, 3).unwrap();
        assert_eq!(actual.len(), ROSVOT_MEL_BINS * 3);
        assert_eq!(&actual[..ROSVOT_MEL_BINS], &mel[..ROSVOT_MEL_BINS]);
        assert_eq!(actual[ROSVOT_MEL_BINS], mel[MEL_BINS]);
    }
}
