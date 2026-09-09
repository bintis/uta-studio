use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

pub const SAMPLE_RATE: usize = 44_100;
pub const HOP_SAMPLES: usize = 1_024;
pub const FREQUENCY_BINS: usize = 384;
pub const INPUT_CHANNELS: usize = 6;
const MINIMUM_MIDI: f32 = 24.0;
const FFT_SIZES: [usize; 3] = [8_192, 16_384, 32_768];

/// Rust-owned approximation of JBM555's published three-scale CQT frontend.
///
/// The output is laid out as `[channel, frame, frequency]`. Channels 0-2 are
/// computed from the original mix and channels 3-5 from the prepared vocal.
pub struct Frontend {
    windows: [Vec<f32>; 3],
    fft: [Arc<dyn rustfft::Fft<f32>>; 3],
}

impl Default for Frontend {
    fn default() -> Self {
        let mut planner = FftPlanner::<f32>::new();
        Self {
            windows: FFT_SIZES.map(periodic_hann),
            fft: FFT_SIZES.map(|size| planner.plan_fft_forward(size)),
        }
    }
}

impl Frontend {
    pub fn frame_count(sample_count: usize) -> usize {
        sample_count.div_ceil(HOP_SAMPLES).max(1)
    }

    pub fn extract(&self, mix: &[f32], vocal: &[f32]) -> Result<(Vec<f32>, usize), String> {
        if mix.is_empty() || vocal.is_empty() {
            return Err("JBM555 requires non-empty mix and prepared-vocal inputs".to_string());
        }
        if mix.iter().chain(vocal).any(|sample| !sample.is_finite()) {
            return Err("JBM555 frontend input contains a non-finite sample".to_string());
        }
        let samples = mix.len().min(vocal.len());
        let frames = Self::frame_count(samples);
        let mut output = vec![0.0_f32; INPUT_CHANNELS * frames * FREQUENCY_BINS];
        self.append_signal(&mix[..samples], frames, &mut output, 0);
        self.append_signal(&vocal[..samples], frames, &mut output, 3);
        Ok((output, frames))
    }

    fn append_signal(&self, audio: &[f32], frames: usize, output: &mut [f32], base_channel: usize) {
        for scale in 0..FFT_SIZES.len() {
            let fft_size = FFT_SIZES[scale];
            let mut buffer = vec![Complex32::default(); fft_size];
            let mut scratch = vec![Complex32::default(); self.fft[scale].get_inplace_scratch_len()];
            for frame in 0..frames {
                let center = frame * HOP_SAMPLES;
                for (index, value) in buffer.iter_mut().enumerate() {
                    let source = center as isize + index as isize - (fft_size / 2) as isize;
                    value.re = reflected(audio, source) * self.windows[scale][index];
                    value.im = 0.0;
                }
                self.fft[scale].process_with_scratch(&mut buffer, &mut scratch);
                let channel_offset = (base_channel + scale) * frames * FREQUENCY_BINS;
                let frame_offset = channel_offset + frame * FREQUENCY_BINS;
                for bin in 0..FREQUENCY_BINS {
                    let midi = MINIMUM_MIDI + bin as f32 / 4.0;
                    let hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
                    let exact = hz * fft_size as f32 / SAMPLE_RATE as f32;
                    let lower = exact.floor() as usize;
                    let fraction = exact - lower as f32;
                    let left = buffer[lower.min(fft_size / 2)].norm();
                    let right = buffer[(lower + 1).min(fft_size / 2)].norm();
                    output[frame_offset + bin] = (left + (right - left) * fraction).ln_1p();
                }
            }
        }
    }
}

fn periodic_hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| 0.5 - 0.5 * (2.0 * PI * index as f32 / size as f32).cos())
        .collect()
}

fn reflected(audio: &[f32], index: isize) -> f32 {
    if audio.len() <= 1 {
        return audio.first().copied().unwrap_or(0.0);
    }
    let period = 2 * (audio.len() - 1) as isize;
    let folded = index.rem_euclid(period);
    audio[if folded < audio.len() as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontend_preserves_dual_input_channel_order() {
        let mut mix = vec![0.0_f32; HOP_SAMPLES];
        let mut vocal = vec![0.0_f32; HOP_SAMPLES];
        mix[0] = 1.0;
        vocal[1] = 0.5;
        let (features, frames) = Frontend::default().extract(&mix, &vocal).unwrap();
        assert_eq!(frames, 1);
        assert_eq!(features.len(), INPUT_CHANNELS * FREQUENCY_BINS);
        assert_ne!(
            features[..3 * FREQUENCY_BINS],
            features[3 * FREQUENCY_BINS..]
        );
        assert!(features.iter().all(|value| value.is_finite()));
    }

    #[test]
    fn frame_count_uses_ceil_hop_coverage() {
        assert_eq!(Frontend::frame_count(1), 1);
        assert_eq!(Frontend::frame_count(HOP_SAMPLES), 1);
        assert_eq!(Frontend::frame_count(HOP_SAMPLES + 1), 2);
    }
}
