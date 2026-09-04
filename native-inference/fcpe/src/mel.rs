//! Log-mel spectrogram frontend for FCPE (16 kHz, 1024 FFT, 160 hop, 128 mel).
//!
//! This is a faithful port of the mel spectrogram used by the OpenVINO FCPE
//! worker (openvino-worker/src/mel.rs), with parameters matching the ONNX
//! model's built-in STFT+mel pipeline.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT_SIZE: usize = 1_024;
pub const HOP_SIZE: usize = 160;
pub const MEL_BINS: usize = 128;

#[derive(Debug, Clone)]
struct MelBand {
    weights: Vec<(usize, f32)>,
}

fn hz_to_htk_mel(hz: f32) -> f32 {
    2_595.0 * (1.0 + hz / 700.0).log10()
}

fn htk_mel_to_hz(mel: f32) -> f32 {
    700.0 * (10.0_f32.powf(mel / 2_595.0) - 1.0)
}

fn mel_bands() -> Vec<MelBand> {
    let min_mel = hz_to_htk_mel(30.0);
    let max_mel = hz_to_htk_mel(8_000.0);
    let mel_points = (0..MEL_BINS + 2)
        .map(|index| {
            let fraction = index as f32 / (MEL_BINS + 1) as f32;
            htk_mel_to_hz(min_mel + fraction * (max_mel - min_mel))
        })
        .collect::<Vec<_>>();
    let fft_frequencies = (0..=FFT_SIZE / 2)
        .map(|bin| SAMPLE_RATE as f32 * bin as f32 / FFT_SIZE as f32)
        .collect::<Vec<_>>();
    (0..MEL_BINS)
        .map(|band| {
            let lower = mel_points[band];
            let center = mel_points[band + 1];
            let upper = mel_points[band + 2];
            let normalization = 2.0 / (upper - lower);
            let weights = fft_frequencies
                .iter()
                .enumerate()
                .filter_map(|(bin, frequency)| {
                    let weight = if *frequency >= lower && *frequency <= center {
                        (*frequency - lower) / (center - lower)
                    } else if *frequency > center && *frequency <= upper {
                        (upper - *frequency) / (upper - center)
                    } else {
                        0.0
                    } * normalization;
                    (weight > 0.0).then_some((bin, weight))
                })
                .collect();
            MelBand { weights }
        })
        .collect()
}

fn reflected_sample(audio: &[f32], padded_index: usize) -> f32 {
    let pad = FFT_SIZE / 2;
    if padded_index < pad {
        return audio[pad - padded_index];
    }
    let audio_index = padded_index - pad;
    if audio_index < audio.len() {
        return audio[audio_index];
    }
    let offset = audio_index - audio.len();
    audio[audio.len() - 2 - offset]
}

/// Returns frame-major log-mel data with shape `(frames, 128)`.
pub fn log_mel_spectrogram(
    audio: &[f32],
    mut progress: impl FnMut(f32),
) -> Result<(Vec<f32>, usize), String> {
    if audio.len() <= FFT_SIZE {
        return Err("FCPE requires more than 64 ms of decoded audio".to_string());
    }
    let frame_count = audio.len() / HOP_SIZE + 1;
    let padded_length = audio.len() + FFT_SIZE;
    let last_frame_end = (frame_count - 1) * HOP_SIZE + FFT_SIZE;
    if last_frame_end > padded_length {
        return Err("internal STFT frame calculation exceeded reflected padding".to_string());
    }
    let bands = mel_bands();
    let window = (0..FFT_SIZE)
        .map(|index| {
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / FFT_SIZE as f32).cos()
        })
        .collect::<Vec<_>>();
    let fft = FftPlanner::<f32>::new().plan_fft_forward(FFT_SIZE);
    let mut buffer = vec![Complex32::default(); FFT_SIZE];
    let mut magnitudes = vec![0.0_f32; FFT_SIZE / 2 + 1];
    let mut output = vec![0.0_f32; frame_count * MEL_BINS];

    for frame in 0..frame_count {
        let start = frame * HOP_SIZE;
        for index in 0..FFT_SIZE {
            buffer[index] =
                Complex32::new(reflected_sample(audio, start + index) * window[index], 0.0);
        }
        fft.process(&mut buffer);
        for (magnitude, value) in magnitudes.iter_mut().zip(&buffer[..=FFT_SIZE / 2]) {
            *magnitude = value.norm();
        }
        for (band_index, band) in bands.iter().enumerate() {
            let energy = band
                .weights
                .iter()
                .map(|(bin, weight)| magnitudes[*bin] * weight)
                .sum::<f32>();
            output[frame * MEL_BINS + band_index] = energy.max(1.0e-5).ln();
        }
        if frame % 250 == 0 {
            progress(frame as f32 / frame_count as f32);
        }
    }
    progress(1.0);
    Ok((output, frame_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_has_the_expected_frame_count_and_finite_log_floor() {
        let audio = vec![0.0; SAMPLE_RATE];
        let (mel, frames) = log_mel_spectrogram(&audio, |_| {}).unwrap();
        assert_eq!(frames, 101);
        assert_eq!(mel.len(), frames * MEL_BINS);
        assert!(mel.iter().all(|value| value.is_finite()));
        assert!(
            mel.iter()
                .all(|value| (*value - 1.0e-5_f32.ln()).abs() < 1.0e-5)
        );
    }
}
