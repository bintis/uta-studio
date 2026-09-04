use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

const SAMPLE_RATE: usize = 16_000;
const FRAME_LENGTH: usize = 400;
const FRAME_SHIFT: usize = 160;
const FFT_SIZE: usize = 512;
const MEL_BINS: usize = 80;

fn mel_scale(frequency: f32) -> f32 {
    1_127.0 * (1.0 + frequency / 700.0).ln()
}

fn inverse_mel_scale(mel: f32) -> f32 {
    700.0 * (mel / 1_127.0).exp_m1()
}

fn mel_filters() -> Vec<Vec<(usize, f32)>> {
    let low = mel_scale(20.0);
    let high = mel_scale(SAMPLE_RATE as f32 / 2.0);
    let points = (0..MEL_BINS + 2)
        .map(|index| inverse_mel_scale(low + (high - low) * index as f32 / (MEL_BINS + 1) as f32))
        .collect::<Vec<_>>();
    (0..MEL_BINS)
        .map(|band| {
            let left = points[band];
            let center = points[band + 1];
            let right = points[band + 2];
            (0..=FFT_SIZE / 2)
                .filter_map(|bin| {
                    let frequency = bin as f32 * SAMPLE_RATE as f32 / FFT_SIZE as f32;
                    let weight = if frequency > left && frequency <= center {
                        (frequency - left) / (center - left)
                    } else if frequency > center && frequency < right {
                        (right - frequency) / (right - center)
                    } else {
                        0.0
                    };
                    (weight > 0.0).then_some((bin, weight))
                })
                .collect()
        })
        .collect()
}

pub fn parse_binary_cmvn(bytes: &[u8]) -> Result<([f32; MEL_BINS], [f32; MEL_BINS]), String> {
    if bytes.len() != 15 + 2 * (MEL_BINS + 1) * 8 || &bytes[..5] != b"\0BDM " {
        return Err("FireRed CMVN is not the pinned Kaldi double matrix".to_string());
    }
    if bytes[5] != 4
        || i32::from_le_bytes(bytes[6..10].try_into().unwrap()) != 2
        || bytes[10] != 4
        || i32::from_le_bytes(bytes[11..15].try_into().unwrap()) != (MEL_BINS + 1) as i32
    {
        return Err("FireRed CMVN matrix shape is invalid".to_string());
    }
    let values = bytes[15..]
        .chunks_exact(8)
        .map(|chunk| f64::from_le_bytes(chunk.try_into().unwrap()))
        .collect::<Vec<_>>();
    let count = values[MEL_BINS];
    if !count.is_finite() || count < 1.0 {
        return Err("FireRed CMVN count is invalid".to_string());
    }
    let mut means = [0.0; MEL_BINS];
    let mut inverse_std = [0.0; MEL_BINS];
    for index in 0..MEL_BINS {
        let mean = values[index] / count;
        let variance = (values[MEL_BINS + 1 + index] / count - mean * mean).max(1.0e-20);
        means[index] = mean as f32;
        inverse_std[index] = (1.0 / variance.sqrt()) as f32;
    }
    Ok((means, inverse_std))
}

pub fn extract(audio: &[f32], cmvn: &[u8]) -> Result<(Vec<f32>, usize), String> {
    if audio.len() < FRAME_LENGTH {
        return Err("FireRed requires at least 25 ms of audio".to_string());
    }
    let (means, inverse_std) = parse_binary_cmvn(cmvn)?;
    let frames = 1 + (audio.len() - FRAME_LENGTH) / FRAME_SHIFT;
    let filters = mel_filters();
    let window = (0..FRAME_LENGTH)
        .map(|index| {
            (0.5 - 0.5
                * (2.0 * std::f32::consts::PI * index as f32 / (FRAME_LENGTH - 1) as f32).cos())
            .powf(0.85)
        })
        .collect::<Vec<_>>();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let mut buffer = vec![Complex32::default(); FFT_SIZE];
    let mut output = vec![0.0; frames * MEL_BINS];
    for frame in 0..frames {
        let start = frame * FRAME_SHIFT;
        let mean = audio[start..start + FRAME_LENGTH].iter().sum::<f32>() / FRAME_LENGTH as f32;
        let mut previous = audio[start] - mean;
        for index in 0..FRAME_LENGTH {
            let sample = audio[start + index] - mean;
            let emphasized = if index == 0 {
                sample * (1.0 - 0.97)
            } else {
                sample - 0.97 * previous
            };
            previous = sample;
            buffer[index] = Complex32::new(emphasized * window[index], 0.0);
        }
        buffer[FRAME_LENGTH..].fill(Complex32::default());
        fft.process(&mut buffer);
        for (band, weights) in filters.iter().enumerate() {
            let energy = weights
                .iter()
                .map(|(bin, weight)| buffer[*bin].norm_sqr() * weight)
                .sum::<f32>()
                .max(f32::EPSILON)
                .ln();
            output[frame * MEL_BINS + band] = (energy - means[band]) * inverse_std[band];
        }
    }
    Ok((output, frames))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kaldi_double_matrix_cmvn_is_parsed_without_a_script_runtime() {
        let mut bytes = b"\0BDM ".to_vec();
        bytes.push(4);
        bytes.extend_from_slice(&2_i32.to_le_bytes());
        bytes.push(4);
        bytes.extend_from_slice(&((MEL_BINS + 1) as i32).to_le_bytes());
        let mut values = vec![0.0_f64; 2 * (MEL_BINS + 1)];
        values[MEL_BINS] = 2.0;
        for index in 0..MEL_BINS {
            values[MEL_BINS + 1 + index] = 2.0;
        }
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        let (means, inverse_std) = parse_binary_cmvn(&bytes).unwrap();
        assert_eq!(means, [0.0; MEL_BINS]);
        assert_eq!(inverse_std, [1.0; MEL_BINS]);
    }
}
