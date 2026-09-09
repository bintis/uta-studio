use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

#[derive(Clone)]
pub(crate) struct Spectrogram {
    pub data: Vec<f32>,
    pub n_freq: usize,
    pub n_frames: usize,
}

impl Spectrogram {
    fn zeros(n_freq: usize, n_frames: usize) -> Self {
        Self {
            data: vec![0.0; n_freq * n_frames * 2],
            n_freq,
            n_frames,
        }
    }

    #[inline]
    pub fn get(&self, frequency: usize, frame: usize) -> (f32, f32) {
        let base = (frequency * self.n_frames + frame) * 2;
        (self.data[base], self.data[base + 1])
    }

    #[inline]
    pub fn set(&mut self, frequency: usize, frame: usize, real: f32, imaginary: f32) {
        let base = (frequency * self.n_frames + frame) * 2;
        self.data[base] = real;
        self.data[base + 1] = imaginary;
    }

    #[inline]
    pub fn add(&mut self, frequency: usize, frame: usize, real: f32, imaginary: f32) {
        let base = (frequency * self.n_frames + frame) * 2;
        self.data[base] += real;
        self.data[base + 1] += imaginary;
    }
}

fn hann_window(window_length: usize, fft_size: usize) -> Vec<f32> {
    let mut window = vec![0.0_f32; window_length];
    for (index, value) in window.iter_mut().enumerate() {
        *value =
            0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / window_length as f32).cos();
    }
    if window_length == fft_size {
        return window;
    }
    let mut padded = vec![0.0_f32; fft_size];
    let left = (fft_size - window_length) / 2;
    padded[left..left + window_length].copy_from_slice(&window);
    padded
}

/// Clamped reflection used by the validated GGML RoFormer implementation.
fn reflect_pad(audio: &[f32], amount: usize) -> Vec<f32> {
    let length = audio.len();
    let mut padded = vec![0.0_f32; amount * 2 + length];
    for index in 0..amount {
        let source = (amount - index).min(length.saturating_sub(1));
        padded[index] = audio[source];
    }
    padded[amount..amount + length].copy_from_slice(audio);
    for index in 0..amount {
        let source = (length as isize - 2 - index as isize).max(0) as usize;
        padded[amount + length + index] = audio[source.min(length.saturating_sub(1))];
    }
    padded
}

/// Frequency-major, frame-minor, interleaved-complex STFT.
pub(crate) fn compute_stft(
    audio: &[f32],
    fft_size: usize,
    hop_length: usize,
    window_length: usize,
) -> Spectrogram {
    let window = hann_window(window_length, fft_size);
    let padded = reflect_pad(audio, fft_size / 2);
    let frequency_count = fft_size / 2 + 1;
    let frame_count = if padded.len() >= fft_size {
        1 + (padded.len() - fft_size) / hop_length
    } else {
        0
    };
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(fft_size);
    let mut scratch = vec![Complex32::default(); fft.get_inplace_scratch_len()];
    let mut result = Spectrogram::zeros(frequency_count, frame_count);
    let mut buffer = vec![Complex32::default(); fft_size];
    for frame in 0..frame_count {
        let start = frame * hop_length;
        for index in 0..fft_size {
            buffer[index] = Complex32::new(padded[start + index] * window[index], 0.0);
        }
        fft.process_with_scratch(&mut buffer, &mut scratch);
        for (frequency, value) in buffer.iter().take(frequency_count).enumerate() {
            result.set(frequency, frame, value.re, value.im);
        }
    }
    result
}

pub(crate) fn compute_istft(
    spectrogram: &Spectrogram,
    fft_size: usize,
    hop_length: usize,
    window_length: usize,
    output_length: usize,
) -> Vec<f32> {
    let window = hann_window(window_length, fft_size);
    let frame_count = spectrogram.n_frames;
    let frequency_count = spectrogram.n_freq;
    let buffer_length = fft_size + hop_length * frame_count.saturating_sub(1) + fft_size;
    let mut accumulated = vec![0.0_f32; buffer_length];
    let mut window_sum = vec![0.0_f32; buffer_length];
    let mut planner = FftPlanner::<f32>::new();
    let inverse = planner.plan_fft_inverse(fft_size);
    let mut scratch = vec![Complex32::default(); inverse.get_inplace_scratch_len()];
    let scale = 1.0 / fft_size as f32;
    let mut buffer = vec![Complex32::default(); fft_size];
    for frame in 0..frame_count {
        for frequency in 0..frequency_count {
            let (real, imaginary) = spectrogram.get(frequency, frame);
            buffer[frequency] = Complex32::new(real, imaginary);
        }
        for frequency in frequency_count..fft_size {
            let mirror = buffer[fft_size - frequency];
            buffer[frequency] = Complex32::new(mirror.re, -mirror.im);
        }
        inverse.process_with_scratch(&mut buffer, &mut scratch);
        let start = frame * hop_length;
        for index in 0..fft_size {
            accumulated[start + index] += buffer[index].re * scale * window[index];
            window_sum[start + index] += window[index] * window[index];
        }
    }
    for (value, denominator) in accumulated.iter_mut().zip(&window_sum) {
        if *denominator > 1.0e-8 {
            *value /= denominator;
        }
    }
    let mut output = vec![0.0_f32; output_length];
    let center = fft_size / 2;
    for (index, value) in output.iter_mut().enumerate() {
        if let Some(source) = accumulated.get(center + index) {
            *value = *source;
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_reconstructs_tone() {
        let fft_size = 2048;
        let hop = 441;
        let length = 44_100;
        let audio = (0..length)
            .map(|index| (2.0 * std::f32::consts::PI * 440.0 * index as f32 / 44_100.0).sin())
            .collect::<Vec<_>>();
        let spectrum = compute_stft(&audio, fft_size, hop, fft_size);
        let reconstructed = compute_istft(&spectrum, fft_size, hop, fft_size, length);
        let maximum = audio
            .iter()
            .zip(&reconstructed)
            .skip(fft_size)
            .take(length - 2 * fft_size)
            .map(|(left, right)| (left - right).abs())
            .fold(0.0_f32, f32::max);
        assert!(maximum < 1.0e-3, "maximum difference {maximum}");
    }
}
