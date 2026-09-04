//! RMVPE's 16 kHz / 128-bin HTK-mel log-magnitude frontend. Ported
//! term-for-term from `native-inference/rmvpe/src/mel.h` (itself ported from
//! this repository's pinned `mel.rs` reference): 1024-point periodic Hann,
//! hop 160, reflect padding of `FFT_SIZE/2` each side, HTK mel scale,
//! magnitude (not power) spectrum, natural-log floor `1e-5`.

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT_SIZE: usize = 1_024;
pub const HOP_SIZE: usize = 160;
pub const MEL_BINS: usize = 128;

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
    let minimum = hz_to_htk_mel(30.0);
    let maximum = hz_to_htk_mel(8_000.0);
    let frequencies = (0..MEL_BINS + 2)
        .map(|index| {
            let fraction = index as f32 / (MEL_BINS + 1) as f32;
            htk_mel_to_hz(minimum + fraction * (maximum - minimum))
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
                    let frequency = SAMPLE_RATE as f32 * bin as f32 / FFT_SIZE as f32;
                    let weight = if frequency >= lower && frequency <= center {
                        (frequency - lower) / (center - lower)
                    } else if frequency > center && frequency <= upper {
                        (upper - frequency) / (upper - center)
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

fn reflected_sample(audio: &[f32], padded_index: i64) -> f32 {
    let pad = (FFT_SIZE / 2) as i64;
    if padded_index < pad {
        return audio[(pad - padded_index) as usize];
    }
    let index = padded_index - pad;
    if (index as usize) < audio.len() {
        return audio[index as usize];
    }
    let offset = index - audio.len() as i64;
    audio[audio.len() - 2 - offset as usize]
}

/// Frame-major log-mel, shape `(frame_count, 128)`, `frame_count = audio_len /
/// HOP_SIZE + 1`. Matches `mel.h::LogMelSpectrogram` exactly.
pub fn log_mel_spectrogram(audio: &[f32]) -> Result<(Vec<f32>, usize), String> {
    if audio.len() <= FFT_SIZE {
        return Err("RMVPE requires more than 64 ms of decoded audio".to_string());
    }
    if audio.iter().any(|value| !value.is_finite()) {
        return Err("RMVPE mel input contains non-finite samples".to_string());
    }
    let frame_count = audio.len() / HOP_SIZE + 1;
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
        let start = (frame * HOP_SIZE) as i64;
        for index in 0..FFT_SIZE {
            let sample = reflected_sample(audio, start + index as i64);
            buffer[index] = Complex32::new(sample * window[index], 0.0);
        }
        fft.process(&mut buffer);
        for (value, complex) in magnitude.iter_mut().zip(&buffer[..=FFT_SIZE / 2]) {
            *value = complex.norm();
        }
        for (band_index, band) in bands.iter().enumerate() {
            let energy = band
                .weights
                .iter()
                .map(|(bin, weight)| magnitude[*bin] * weight)
                .sum::<f32>();
            output[frame * MEL_BINS + band_index] = energy.max(1.0e-5).ln();
        }
    }
    Ok((output, frame_count))
}

/// Rearranges frame-major mel `(frames, 128)` into channel-major `[128,
/// window_frames]`, zero-padding (not log-floor-padding) beyond the real
/// frame count. Matches `mel.h::ToChannelMajorWindow` exactly.
pub fn to_channel_major_window(
    frame_major: &[f32],
    frames: usize,
    start: usize,
    window_frames: usize,
) -> Vec<f32> {
    let mut channel_major = vec![0.0_f32; MEL_BINS * window_frames];
    let copied_frames = frames.saturating_sub(start).min(window_frames);
    for frame in 0..copied_frames {
        for channel in 0..MEL_BINS {
            channel_major[channel * window_frames + frame] =
                frame_major[(start + frame) * MEL_BINS + channel];
        }
    }
    channel_major
}
