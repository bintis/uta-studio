use std::f32::consts::PI;
use std::sync::Arc;

use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

#[derive(Debug, Clone, PartialEq)]
pub struct MelConfig {
    pub sample_rate: usize,
    pub n_fft: usize,
    pub win_length: usize,
    pub hop_length: usize,
    pub n_mels: usize,
    pub fmin: f32,
    pub fmax: f32,
    pub clip_value: f32,
}

impl Default for MelConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            n_fft: 2_048,
            win_length: 2_048,
            hop_length: 441,
            n_mels: 80,
            fmin: 0.0,
            fmax: 8_000.0,
            clip_value: 1.0e-5,
        }
    }
}

/// GAME's periodic-Hann, magnitude, Slaney-normalized log-mel frontend.
#[derive(Clone)]
pub struct MelExtractor {
    config: MelConfig,
    window: Vec<f32>,
    mel_filterbank: Vec<f32>,
    fft: Arc<dyn rustfft::Fft<f32>>,
}

impl MelExtractor {
    pub fn new(config: MelConfig) -> Result<Self, String> {
        validate_config(&config)?;
        let mut planner = FftPlanner::<f32>::new();
        let fft = planner.plan_fft_forward(config.n_fft);
        Ok(Self {
            window: periodic_hann(config.win_length),
            mel_filterbank: slaney_filterbank(&config),
            fft,
            config,
        })
    }

    pub fn config(&self) -> &MelConfig {
        &self.config
    }

    pub fn frame_count(&self, sample_count: usize) -> usize {
        let (left, right) = padding(self.config.win_length, self.config.hop_length);
        let padded = sample_count.saturating_add(left).saturating_add(right);
        if sample_count == 0 || padded < self.config.win_length {
            0
        } else {
            (padded - self.config.win_length) / self.config.hop_length + 1
        }
    }

    /// Returns frame-major `[frames, n_mels]` log-mel values.
    pub fn extract(&self, audio: &[f32]) -> Result<Vec<f32>, String> {
        let frames = self.frame_count(audio.len());
        if frames == 0 {
            return Ok(Vec::new());
        }
        if audio.iter().any(|sample| !sample.is_finite()) {
            return Err("GAME mel input contains a non-finite sample".to_string());
        }
        let (left, right) = padding(self.config.win_length, self.config.hop_length);
        let padded = reflect_pad(audio, left, right)?;
        let frequency_bins = self.config.n_fft / 2 + 1;
        let mut input = vec![Complex32::default(); self.config.n_fft];
        let mut scratch = vec![Complex32::default(); self.fft.get_inplace_scratch_len()];
        let mut magnitude = vec![0.0_f32; frequency_bins];
        let mut output = vec![0.0_f32; frames * self.config.n_mels];

        for frame in 0..frames {
            let start = frame * self.config.hop_length;
            input.fill(Complex32::default());
            for index in 0..self.config.win_length {
                input[index].re = padded[start + index] * self.window[index];
            }
            self.fft.process_with_scratch(&mut input, &mut scratch);
            for (value, bin) in magnitude.iter_mut().zip(input.iter()) {
                *value = bin.norm();
            }
            for mel in 0..self.config.n_mels {
                let weights =
                    &self.mel_filterbank[mel * frequency_bins..(mel + 1) * frequency_bins];
                let value = weights
                    .iter()
                    .zip(&magnitude)
                    .map(|(weight, magnitude)| weight * magnitude)
                    .sum::<f32>();
                output[frame * self.config.n_mels + mel] = value.max(self.config.clip_value).ln();
            }
        }
        Ok(output)
    }
}

fn validate_config(config: &MelConfig) -> Result<(), String> {
    if config.sample_rate == 0
        || config.n_fft == 0
        || config.win_length == 0
        || config.hop_length == 0
        || config.n_mels == 0
    {
        return Err("GAME mel dimensions must be positive".to_string());
    }
    if config.win_length > config.n_fft {
        return Err("GAME mel window exceeds the FFT size".to_string());
    }
    if config.hop_length > config.win_length {
        return Err("GAME mel hop exceeds the window size".to_string());
    }
    if !config.fmin.is_finite()
        || !config.fmax.is_finite()
        || config.fmin < 0.0
        || config.fmax <= config.fmin
        || config.fmax > config.sample_rate as f32 / 2.0
    {
        return Err("GAME mel frequency range is invalid".to_string());
    }
    if !config.clip_value.is_finite() || config.clip_value <= 0.0 {
        return Err("GAME mel clip value must be positive and finite".to_string());
    }
    Ok(())
}

fn periodic_hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| 0.5 - 0.5 * (2.0 * PI * index as f32 / size as f32).cos())
        .collect()
}

fn padding(window: usize, hop: usize) -> (usize, usize) {
    let difference = window - hop;
    (difference / 2, difference.div_ceil(2))
}

fn reflect_pad(audio: &[f32], left: usize, right: usize) -> Result<Vec<f32>, String> {
    if audio.is_empty() || left >= audio.len() || right >= audio.len() {
        return Err(format!(
            "GAME mel reflection padding exceeds the waveform: left={left} right={right} length={}",
            audio.len()
        ));
    }
    let mut padded = vec![0.0_f32; left + audio.len() + right];
    for index in 0..left {
        padded[index] = audio[left - index];
    }
    padded[left..left + audio.len()].copy_from_slice(audio);
    for index in 0..right {
        padded[left + audio.len() + index] = audio[audio.len() - 2 - index];
    }
    Ok(padded)
}

fn slaney_frequency_spacing() -> f32 {
    200.0 / 3.0
}

fn slaney_min_log_mel() -> f32 {
    1_000.0 / slaney_frequency_spacing()
}

fn slaney_log_step() -> f32 {
    6.4_f32.ln() / 27.0
}

fn hz_to_slaney_mel(hz: f32) -> f32 {
    if hz >= 1_000.0 {
        slaney_min_log_mel() + (hz / 1_000.0).ln() / slaney_log_step()
    } else {
        hz / slaney_frequency_spacing()
    }
}

fn slaney_mel_to_hz(mel: f32) -> f32 {
    if mel >= slaney_min_log_mel() {
        1_000.0 * (slaney_log_step() * (mel - slaney_min_log_mel())).exp()
    } else {
        mel * slaney_frequency_spacing()
    }
}

fn slaney_filterbank(config: &MelConfig) -> Vec<f32> {
    let frequency_bins = config.n_fft / 2 + 1;
    let minimum_mel = hz_to_slaney_mel(config.fmin);
    let maximum_mel = hz_to_slaney_mel(config.fmax);
    let points = (0..config.n_mels + 2)
        .map(|index| {
            let mel = minimum_mel
                + (maximum_mel - minimum_mel) * index as f32 / (config.n_mels + 1) as f32;
            slaney_mel_to_hz(mel)
        })
        .collect::<Vec<_>>();
    let mut filterbank = vec![0.0_f32; config.n_mels * frequency_bins];
    for mel in 0..config.n_mels {
        let lower = points[mel];
        let center = points[mel + 1];
        let upper = points[mel + 2];
        let normalization = 2.0 / (upper - lower);
        for frequency in 0..frequency_bins {
            let hz = frequency as f32 * config.sample_rate as f32 / config.n_fft as f32;
            let weight = if hz >= lower && hz <= center {
                (hz - lower) / (center - lower)
            } else if hz > center && hz <= upper {
                (upper - hz) / (upper - center)
            } else {
                0.0
            };
            filterbank[mel * frequency_bins + frequency] = weight * normalization;
        }
    }
    filterbank
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_game_medium_frontend() {
        assert_eq!(
            MelConfig::default(),
            MelConfig {
                sample_rate: 44_100,
                n_fft: 2_048,
                win_length: 2_048,
                hop_length: 441,
                n_mels: 80,
                fmin: 0.0,
                fmax: 8_000.0,
                clip_value: 1.0e-5,
            }
        );
    }

    #[test]
    fn slaney_scale_round_trips() {
        for hz in [0.0, 440.0, 1_000.0, 8_000.0] {
            let recovered = slaney_mel_to_hz(hz_to_slaney_mel(hz));
            assert!((recovered - hz).abs() <= 1.0e-4 * hz.max(1.0));
        }
    }

    #[test]
    fn periodic_hann_matches_torch() {
        assert_eq!(periodic_hann(4), vec![0.0, 0.5, 1.0, 0.5]);
    }

    #[test]
    fn reflection_excludes_edge_values() {
        assert_eq!(
            reflect_pad(&[1.0, 2.0, 3.0, 4.0], 2, 2).unwrap(),
            vec![3.0, 2.0, 1.0, 2.0, 3.0, 4.0, 3.0, 2.0]
        );
    }

    #[test]
    fn frame_count_matches_game_contract() {
        let extractor = MelExtractor::new(MelConfig::default()).unwrap();
        assert_eq!(extractor.frame_count(0), 0);
        assert_eq!(extractor.frame_count(441), 1);
        assert_eq!(extractor.frame_count(44_100), 100);
    }

    #[test]
    fn small_log_mel_example_matches_reference() {
        let extractor = MelExtractor::new(MelConfig {
            sample_rate: 8,
            n_fft: 4,
            win_length: 4,
            hop_length: 2,
            n_mels: 1,
            fmin: 0.0,
            fmax: 4.0,
            clip_value: 1.0e-5,
        })
        .unwrap();
        let actual = extractor.extract(&[1.0, 0.0, 0.0, 0.0]).unwrap();
        let expected = [0.25_f32.ln(), 1.0e-5_f32.ln()];
        for (actual, expected) in actual.iter().zip(expected) {
            assert!((actual - expected).abs() < 1.0e-5);
        }
    }
}
