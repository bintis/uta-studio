//! Whisper-compatible Slaney log-mel frontend used by Qwen3-ASR and the
//! Qwen3 Forced Aligner.

use rustfft::{Fft, FftPlanner, num_complex::Complex32};
use std::sync::Arc;

pub const SAMPLE_RATE: usize = 16_000;
pub const FFT: usize = 400;
pub const HOP: usize = 160;
pub const FREQUENCIES: usize = FFT / 2 + 1;

#[derive(Debug, Clone)]
pub struct Mel {
    pub bins: usize,
    pub frames: usize,
    /// GGML/Whisper order: `[mel_bin, time]`, with time contiguous.
    pub data: Vec<f32>,
}

pub struct Frontend {
    bins: usize,
    filters: Vec<f32>,
    window: Vec<f32>,
    fft: Arc<dyn Fft<f32>>,
}

impl Frontend {
    pub fn whisper(bins: usize) -> Self {
        Self {
            bins,
            filters: slaney_filters(bins),
            window: (0..FFT)
                .map(|index| {
                    (0.5 - 0.5 * (std::f64::consts::TAU * index as f64 / FFT as f64).cos()) as f32
                })
                .collect(),
            fft: FftPlanner::new().plan_fft_forward(FFT),
        }
    }

    pub fn filters(&self) -> &[f32] {
        &self.filters
    }

    pub fn compute(&self, samples: &[f32]) -> Mel {
        // Whisper center=true computes floor(samples/hop)+1 STFT frames and
        // discards the final frame before dynamic-range reduction.
        let frames = samples.len() / HOP;
        if frames == 0 || self.bins == 0 {
            return Mel {
                bins: self.bins,
                frames: 0,
                data: Vec::new(),
            };
        }

        let mut frame_major = vec![0.0_f32; frames * self.bins];
        let mut fft_input = vec![Complex32::default(); FFT];
        let mut scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
        let mut power = [0.0_f32; FREQUENCIES];
        for (frame, output) in frame_major.chunks_mut(self.bins).enumerate() {
            for (index, value) in fft_input.iter_mut().enumerate() {
                let at = frame as isize * HOP as isize + index as isize - (FFT / 2) as isize;
                *value = Complex32::new(reflect_sample(samples, at) * self.window[index], 0.0);
            }
            self.fft.process_with_scratch(&mut fft_input, &mut scratch);
            for (value, complex) in power.iter_mut().zip(&fft_input) {
                *value = complex.re * complex.re + complex.im * complex.im;
            }
            for (bin, value) in output.iter_mut().enumerate() {
                let filter = &self.filters[bin * FREQUENCIES..(bin + 1) * FREQUENCIES];
                let sum: f64 = filter
                    .iter()
                    .zip(&power)
                    .map(|(&weight, &power)| f64::from(weight) * f64::from(power))
                    .sum();
                *value = sum.max(1e-10).log10() as f32;
            }
        }

        let floor = frame_major
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max)
            - 8.0;
        let mut data = vec![0.0; frame_major.len()];
        for (bin, output) in data.chunks_mut(frames).enumerate() {
            for (frame, value) in output.iter_mut().enumerate() {
                *value = (frame_major[frame * self.bins + bin].max(floor) + 4.0) * 0.25;
            }
        }
        Mel {
            bins: self.bins,
            frames,
            data,
        }
    }
}

fn reflect_sample(samples: &[f32], index: isize) -> f32 {
    match samples.len() {
        0 => 0.0,
        1 => samples[0],
        count => {
            let period = 2 * (count as isize - 1);
            let at = index.rem_euclid(period) as usize;
            samples[if at < count { at } else { period as usize - at }]
        }
    }
}

/// Slaney mel-frequency scale with Slaney area normalization.
pub fn slaney_filters(bins: usize) -> Vec<f32> {
    let hz_to_mel = |hz: f64| {
        if hz < 1000.0 {
            hz / (200.0 / 3.0)
        } else {
            15.0 + (hz / 1000.0).ln() / (6.4_f64.ln() / 27.0)
        }
    };
    let mel_to_hz = |mel: f64| {
        if mel < 15.0 {
            mel * (200.0 / 3.0)
        } else {
            1000.0 * ((mel - 15.0) * (6.4_f64.ln() / 27.0)).exp()
        }
    };
    let max_mel = hz_to_mel(SAMPLE_RATE as f64 / 2.0);
    let points: Vec<f64> = (0..bins + 2)
        .map(|index| mel_to_hz(max_mel * index as f64 / (bins + 1) as f64))
        .collect();
    let mut filters = vec![0.0; bins * FREQUENCIES];
    for bin in 0..bins {
        let (left, center, right) = (points[bin], points[bin + 1], points[bin + 2]);
        let scale = 2.0 / (right - left);
        for frequency in 0..FREQUENCIES {
            let hz = SAMPLE_RATE as f64 * frequency as f64 / FFT as f64;
            let weight = ((hz - left) / (center - left))
                .min((right - hz) / (right - center))
                .max(0.0);
            filters[bin * FREQUENCIES + frequency] = (weight * scale) as f32;
        }
    }
    filters
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signal(count: usize) -> Vec<f32> {
        let mut state = 0x42a9_u32;
        (0..count)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f64 / u32::MAX as f64 * 0.4 - 0.2) as f32
            })
            .collect()
    }

    /// Independent O(N²) double-precision DFT, not the production FFT.
    fn reference(frontend: &Frontend, samples: &[f32]) -> Vec<f64> {
        let frames = samples.len() / HOP;
        let mut result = vec![0.0; frames * frontend.bins];
        for frame in 0..frames {
            let mut power = vec![0.0_f64; FREQUENCIES];
            for (frequency, value) in power.iter_mut().enumerate() {
                let (mut real, mut imaginary) = (0.0, 0.0);
                for index in 0..FFT {
                    let at = frame as isize * HOP as isize + index as isize - (FFT / 2) as isize;
                    let sample =
                        f64::from(reflect_sample(samples, at)) * f64::from(frontend.window[index]);
                    let angle = -std::f64::consts::TAU * (frequency * index) as f64 / FFT as f64;
                    real += sample * angle.cos();
                    imaginary += sample * angle.sin();
                }
                *value = real * real + imaginary * imaginary;
            }
            for bin in 0..frontend.bins {
                let sum: f64 = power
                    .iter()
                    .enumerate()
                    .map(|(frequency, power)| {
                        power * f64::from(frontend.filters[bin * FREQUENCIES + frequency])
                    })
                    .sum();
                result[bin * frames + frame] = sum.max(1e-10).log10();
            }
        }
        let floor = result.iter().copied().fold(f64::NEG_INFINITY, f64::max) - 8.0;
        for value in &mut result {
            *value = (value.max(floor) + 4.0) * 0.25;
        }
        result
    }

    #[test]
    fn fft_mel_matches_independent_dft_reference() {
        let frontend = Frontend::whisper(128);
        let samples = signal(640);
        let actual = frontend.compute(&samples);
        let expected = reference(&frontend, &samples);
        assert_eq!(
            (actual.bins, actual.frames, actual.data.len()),
            (128, 4, 512)
        );
        let max_delta = actual
            .data
            .iter()
            .zip(&expected)
            .map(|(actual, expected)| (f64::from(*actual) - expected).abs())
            .fold(0.0_f64, f64::max);
        assert!(max_delta < 2e-5, "FFT/DFT mel max delta {max_delta}");
    }

    #[test]
    fn silence_and_final_stft_frame_semantics() {
        let frontend = Frontend::whisper(128);
        for count in [160, 320, 481, 8000] {
            let mel = frontend.compute(&vec![0.0; count]);
            assert_eq!(mel.frames, count / HOP);
            assert!(mel.data.iter().all(|&value| value == -1.5));
        }
        assert_eq!(frontend.compute(&[]).frames, 0);
        assert!(frontend.compute(&[1.0]).data.is_empty());
    }

    #[test]
    fn reflection_does_not_repeat_boundary_sample() {
        let samples = [0.0, 1.0, 2.0, 3.0];
        assert_eq!(
            (-4..8)
                .map(|index| reflect_sample(&samples, index))
                .collect::<Vec<_>>(),
            [2.0, 3.0, 2.0, 1.0, 0.0, 1.0, 2.0, 3.0, 2.0, 1.0, 0.0, 1.0]
        );
    }

    #[test]
    fn power_scaling_and_normalization_are_per_utterance() {
        let frontend = Frontend::whisper(128);
        let samples = signal(800);
        let doubled: Vec<_> = samples.iter().map(|value| value * 2.0).collect();
        let first = frontend.compute(&samples);
        let second = frontend.compute(&doubled);
        let shift = 4.0_f32.log10() / 4.0;
        for (&first, &second) in first.data.iter().zip(&second.data) {
            assert!(((second - first) - shift).abs() < 2e-6);
        }
    }
}
