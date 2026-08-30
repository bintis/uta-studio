use std::collections::VecDeque;
use std::f32::consts::PI;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use sha2::{Digest, Sha256};

use crate::artifact::{
    ACOUSTIC_EVIDENCE_CONTRACT, ACOUSTIC_EVIDENCE_VERSION, AcousticEvidenceFrameV1,
    AcousticEvidenceV1,
};
use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};
use crate::execution::CancellationToken;
use crate::fingerprint::ACOUSTIC_DSP_VERSION;

const SAMPLE_RATE: u32 = 16_000;
const WINDOW_SAMPLES: usize = 512;
const HOP_SAMPLES: usize = 160;
const HOP_CANONICAL: u64 = 10_000;
const MAX_AUDIO_SECONDS: u64 = 4 * 60 * 60;

pub fn analyze_acoustic_evidence(
    ffmpeg: &Path,
    input: &Path,
    semantic_audio_role: &str,
    source_start: u64,
    source_duration: u64,
    cancellation: &CancellationToken,
) -> EngineResult<AcousticEvidenceV1> {
    if !ffmpeg.is_file() || !input.is_file() {
        return Err(EngineError::new(
            EngineErrorCode::MissingRequiredInput,
            "acoustic DSP requires packaged ffmpeg and an analysis audio input",
        ));
    }
    if semantic_audio_role.trim().is_empty() {
        return Err(output_error("acoustic DSP semantic audio role is empty"));
    }
    let mut command = Command::new(ffmpeg);
    command
        .args(["-v", "error", "-nostdin", "-i"])
        .arg(input)
        .args([
            "-map",
            "0:a:0",
            "-map_metadata",
            "-1",
            "-vn",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-f",
            "f32le",
            "pipe:1",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = command.spawn().map_err(|error| {
        EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not start acoustic DSP decode: {error}"),
        )
    })?;
    let mut stdout = child
        .stdout
        .take()
        .ok_or_else(|| output_error("acoustic DSP decode stdout was not captured"))?;
    let (sender, receiver) = mpsc::sync_channel(2);
    let stdout_reader = std::thread::spawn(move || {
        loop {
            let mut bytes = vec![0_u8; 64 * 1024];
            match stdout.read(&mut bytes) {
                Ok(0) => {
                    let _ = sender.send(Ok(Vec::new()));
                    break;
                }
                Ok(count) => {
                    bytes.truncate(count);
                    if sender.send(Ok(bytes)).is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.send(Err(error.to_string()));
                    break;
                }
            }
        }
    });
    let mut stderr = child
        .stderr
        .take()
        .ok_or_else(|| output_error("acoustic DSP decode stderr was not captured"))?;
    let stderr_reader = std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut chunk = [0_u8; 8 * 1024];
        loop {
            match stderr.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(count) if output.len() < 64 * 1024 => {
                    let remaining = 64 * 1024 - output.len();
                    output.extend_from_slice(&chunk[..count.min(remaining)]);
                }
                Ok(_) => {}
            }
        }
        output
    });

    let mut processor = AcousticProcessor::new(source_start);
    let mut decoded_digest = Sha256::new();
    let mut carry = Vec::with_capacity(3);
    let max_samples = u64::from(SAMPLE_RATE) * MAX_AUDIO_SECONDS;
    let mut sample_count = 0_u64;
    let mut failure = None;
    let mut was_cancelled = false;
    loop {
        if cancellation.is_cancelled() {
            was_cancelled = true;
            kill_process(&mut child);
            break;
        }
        let bytes = match receiver.recv_timeout(Duration::from_millis(25)) {
            Ok(Ok(bytes)) if bytes.is_empty() => break,
            Ok(Ok(bytes)) => bytes,
            Ok(Err(error)) => {
                failure = Some(format!("could not read acoustic DSP decode: {error}"));
                kill_process(&mut child);
                break;
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        carry.extend_from_slice(&bytes);
        let complete = carry.len() / 4 * 4;
        for sample in carry[..complete].chunks_exact(4) {
            let value = f32::from_le_bytes([sample[0], sample[1], sample[2], sample[3]]);
            if !value.is_finite() {
                failure = Some("acoustic DSP decode contains non-finite samples".to_string());
                break;
            }
            decoded_digest.update(sample);
            processor.push(value)?;
            sample_count += 1;
            if sample_count > max_samples {
                failure = Some("acoustic DSP input exceeds four hours".to_string());
                break;
            }
        }
        if complete > 0 {
            carry.drain(..complete);
        }
        if failure.is_some() {
            kill_process(&mut child);
            break;
        }
    }
    drop(receiver);
    let status = child.wait().map_err(|error| {
        EngineError::new(
            EngineErrorCode::DecodeFailed,
            format!("could not wait for acoustic DSP decode: {error}"),
        )
    })?;
    let _ = stdout_reader.join();
    let stderr = stderr_reader.join().unwrap_or_default();
    if was_cancelled {
        return Err(EngineError::new(
            EngineErrorCode::Cancelled,
            "acoustic DSP was cancelled",
        ));
    }
    if let Some(message) = failure {
        return Err(EngineError::new(EngineErrorCode::DecodeFailed, message));
    }
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr).trim().to_string();
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            if detail.is_empty() {
                format!("acoustic DSP decode failed with {status}")
            } else {
                format!("acoustic DSP decode failed: {detail}")
            },
        ));
    }
    if !carry.is_empty() || sample_count == 0 {
        return Err(EngineError::new(
            EngineErrorCode::DecodeFailed,
            "acoustic DSP decode is empty or malformed",
        ));
    }
    processor.finish(sample_count)?;
    let decoded_duration = sample_count
        .checked_mul(u64::from(CANONICAL_TIMEBASE))
        .and_then(|units| units.checked_add(u64::from(SAMPLE_RATE / 2)))
        .map(|units| units / u64::from(SAMPLE_RATE))
        .ok_or_else(|| output_error("acoustic DSP duration overflows"))?;
    if decoded_duration.abs_diff(source_duration) > HOP_CANONICAL {
        return Err(output_error(
            "acoustic DSP decode duration differs from the validated semantic input",
        ));
    }
    let evidence = AcousticEvidenceV1 {
        contract: ACOUSTIC_EVIDENCE_CONTRACT.to_string(),
        version: ACOUSTIC_EVIDENCE_VERSION,
        algorithm: ACOUSTIC_DSP_VERSION.to_string(),
        timebase: CANONICAL_TIMEBASE,
        start: source_start,
        hop: HOP_CANONICAL,
        sample_rate: SAMPLE_RATE,
        window_samples: WINDOW_SAMPLES as u32,
        semantic_audio_role: semantic_audio_role.to_string(),
        decoded_audio_sha256: format!("{:x}", decoded_digest.finalize()),
        frames: processor.frames,
    };
    evidence.validate()?;
    Ok(evidence)
}

struct AcousticProcessor {
    source_start: u64,
    next_sample: u64,
    buffer: Vec<f32>,
    previous_spectrum: Option<Vec<f32>>,
    previous_periodicity: Option<f32>,
    previous_fundamental_hz: Option<f32>,
    pitch_deltas_cents: VecDeque<f32>,
    frames: Vec<AcousticEvidenceFrameV1>,
}

impl AcousticProcessor {
    fn new(source_start: u64) -> Self {
        Self {
            source_start,
            next_sample: 0,
            buffer: Vec::with_capacity(WINDOW_SAMPLES + 64 * 1024),
            previous_spectrum: None,
            previous_periodicity: None,
            previous_fundamental_hz: None,
            pitch_deltas_cents: VecDeque::with_capacity(12),
            frames: Vec::new(),
        }
    }

    fn push(&mut self, sample: f32) -> EngineResult<()> {
        self.buffer.push(sample);
        while self.buffer.len() >= WINDOW_SAMPLES {
            let mut window = [0.0_f32; WINDOW_SAMPLES];
            window.copy_from_slice(&self.buffer[..WINDOW_SAMPLES]);
            self.process(window)?;
            self.buffer.drain(..HOP_SAMPLES);
            self.next_sample += HOP_SAMPLES as u64;
        }
        Ok(())
    }

    fn finish(&mut self, total_samples: u64) -> EngineResult<()> {
        while self.next_sample < total_samples {
            let mut window = [0.0_f32; WINDOW_SAMPLES];
            let count = self.buffer.len().min(WINDOW_SAMPLES);
            window[..count].copy_from_slice(&self.buffer[..count]);
            self.process(window)?;
            let drain = self.buffer.len().min(HOP_SAMPLES);
            self.buffer.drain(..drain);
            self.next_sample += HOP_SAMPLES as u64;
        }
        Ok(())
    }

    fn process(&mut self, window: [f32; WINDOW_SAMPLES]) -> EngineResult<()> {
        let start = (self.frames.len() as u64)
            .checked_mul(HOP_CANONICAL)
            .and_then(|offset| self.source_start.checked_add(offset))
            .ok_or_else(|| output_error("acoustic evidence timeline overflows"))?;
        let rms =
            (window.iter().map(|value| value * value).sum::<f32>() / WINDOW_SAMPLES as f32).sqrt();
        let spectrum = magnitude_spectrum(&window);
        let spectral_flux = self.previous_spectrum.as_ref().map(|previous| {
            spectrum
                .iter()
                .zip(previous)
                .map(|(current, previous)| (current - previous).max(0.0))
                .sum::<f32>()
                / spectrum.len() as f32
        });
        let (periodicity, snr_db, fundamental_hz) =
            periodicity_snr_and_fundamental(&window, &spectrum, rms);
        let cents_delta = self
            .previous_fundamental_hz
            .zip(fundamental_hz)
            .map(|(previous, current)| 1_200.0 * (current / previous).log2())
            .filter(|delta| delta.is_finite());
        if let Some(delta) = cents_delta {
            if self.pitch_deltas_cents.len() == 12 {
                self.pitch_deltas_cents.pop_front();
            }
            self.pitch_deltas_cents.push_back(delta);
        } else {
            self.pitch_deltas_cents.clear();
        }
        let vibrato_activation = vibrato_activation(&self.pitch_deltas_cents, periodicity);
        let glide_activation = cents_delta
            .map(|delta| ((delta.abs() - 5.0) / 75.0).clamp(0.0, 1.0) * periodicity)
            .unwrap_or(0.0);
        let ornament_activation = cents_delta
            .map(|delta| (delta.abs() / 180.0).clamp(0.0, 1.0) * periodicity)
            .unwrap_or(0.0);
        let high_frequency_ratio = spectrum
            .iter()
            .enumerate()
            .filter(|(index, _)| (*index + 1) * SAMPLE_RATE as usize / WINDOW_SAMPLES >= 2_000)
            .map(|(_, magnitude)| *magnitude)
            .sum::<f32>();
        let breath_activation = ((high_frequency_ratio / 0.4).clamp(0.0, 1.0)
            * (1.0 - periodicity)
            * (rms / 0.02).clamp(0.0, 1.0))
        .clamp(0.0, 1.0);
        let voicing_transition_activation = self
            .previous_periodicity
            .map(|previous| (periodicity - previous).abs())
            .unwrap_or(0.0)
            .clamp(0.0, 1.0);
        self.frames.push(AcousticEvidenceFrameV1 {
            start,
            rms,
            spectral_flux,
            periodicity,
            snr_db,
            fundamental_hz,
            vibrato_activation,
            glide_activation,
            ornament_activation,
            breath_activation,
            voicing_transition_activation,
        });
        self.previous_spectrum = Some(spectrum);
        self.previous_periodicity = Some(periodicity);
        self.previous_fundamental_hz = fundamental_hz;
        Ok(())
    }
}

fn magnitude_spectrum(samples: &[f32; WINDOW_SAMPLES]) -> Vec<f32> {
    let mut real = [0.0_f32; WINDOW_SAMPLES];
    let mut imaginary = [0.0_f32; WINDOW_SAMPLES];
    for (index, sample) in samples.iter().enumerate() {
        let hann = 0.5 - 0.5 * (2.0 * PI * index as f32 / (WINDOW_SAMPLES - 1) as f32).cos();
        real[index] = sample * hann;
    }
    let mut target = 0usize;
    for index in 1..WINDOW_SAMPLES {
        let mut bit = WINDOW_SAMPLES >> 1;
        while target & bit != 0 {
            target ^= bit;
            bit >>= 1;
        }
        target ^= bit;
        if index < target {
            real.swap(index, target);
        }
    }
    let mut length = 2;
    while length <= WINDOW_SAMPLES {
        let angle = -2.0 * PI / length as f32;
        let step_real = angle.cos();
        let step_imaginary = angle.sin();
        for start in (0..WINDOW_SAMPLES).step_by(length) {
            let mut twiddle_real = 1.0;
            let mut twiddle_imaginary = 0.0;
            for offset in 0..length / 2 {
                let even = start + offset;
                let odd = even + length / 2;
                let odd_real = real[odd] * twiddle_real - imaginary[odd] * twiddle_imaginary;
                let odd_imaginary = real[odd] * twiddle_imaginary + imaginary[odd] * twiddle_real;
                real[odd] = real[even] - odd_real;
                imaginary[odd] = imaginary[even] - odd_imaginary;
                real[even] += odd_real;
                imaginary[even] += odd_imaginary;
                let next_real = twiddle_real * step_real - twiddle_imaginary * step_imaginary;
                twiddle_imaginary = twiddle_real * step_imaginary + twiddle_imaginary * step_real;
                twiddle_real = next_real;
            }
        }
        length *= 2;
    }
    let mut magnitudes = (1..=WINDOW_SAMPLES / 2)
        .map(|index| real[index].hypot(imaginary[index]))
        .collect::<Vec<_>>();
    let total = magnitudes.iter().sum::<f32>().max(f32::EPSILON);
    for magnitude in &mut magnitudes {
        *magnitude /= total;
    }
    magnitudes
}

fn periodicity_snr_and_fundamental(
    samples: &[f32; WINDOW_SAMPLES],
    spectrum: &[f32],
    rms: f32,
) -> (f32, f32, Option<f32>) {
    let minimum_bin = (50 * WINDOW_SAMPLES / SAMPLE_RATE as usize).max(1);
    let maximum_bin = (1_000 * WINDOW_SAMPLES / SAMPLE_RATE as usize).min(spectrum.len() - 1);
    let strongest = (minimum_bin..=maximum_bin)
        .max_by(|left, right| spectrum[*left - 1].total_cmp(&spectrum[*right - 1]))
        .unwrap_or(minimum_bin);
    let center_lag = (WINDOW_SAMPLES / strongest.max(1)).clamp(16, 320);
    let mean = samples.iter().sum::<f32>() / WINDOW_SAMPLES as f32;
    let power = samples
        .iter()
        .map(|sample| (sample - mean).powi(2))
        .sum::<f32>();
    if power <= 1.0e-12 {
        return (0.0, -120.0, None);
    }
    let mut best = (0.0_f32, center_lag);
    for lag in center_lag.saturating_sub(2)..=(center_lag + 2).min(WINDOW_SAMPLES - 1) {
        let mut cross = 0.0;
        let mut left_power = 0.0;
        let mut right_power = 0.0;
        for index in lag..WINDOW_SAMPLES {
            let left = samples[index] - mean;
            let right = samples[index - lag] - mean;
            cross += left * right;
            left_power += left * left;
            right_power += right * right;
        }
        let correlation = cross / (left_power * right_power).sqrt().max(f32::EPSILON);
        if correlation > best.0 {
            best = (correlation, lag);
        }
    }
    let periodicity = best.0.clamp(0.0, 1.0);
    let mut residual_power = 0.0;
    for index in best.1..WINDOW_SAMPLES {
        let residual = (samples[index] - mean) - periodicity * (samples[index - best.1] - mean);
        residual_power += residual * residual;
    }
    let snr_db = (10.0 * (power / residual_power.max(1.0e-12)).log10()).clamp(-120.0, 120.0);
    let fundamental_hz =
        (periodicity >= 0.35 && rms >= 1.0e-4).then_some(SAMPLE_RATE as f32 / best.1 as f32);
    (periodicity, snr_db, fundamental_hz)
}

fn vibrato_activation(deltas: &VecDeque<f32>, periodicity: f32) -> f32 {
    if deltas.len() < 6 {
        return 0.0;
    }
    let meaningful = deltas
        .iter()
        .copied()
        .filter(|delta| delta.abs() >= 1.0)
        .collect::<Vec<_>>();
    if meaningful.len() < 4 {
        return 0.0;
    }
    let reversals = meaningful
        .windows(2)
        .filter(|pair| pair[0].is_sign_positive() != pair[1].is_sign_positive())
        .count();
    let mean_magnitude =
        meaningful.iter().map(|delta| delta.abs()).sum::<f32>() / meaningful.len() as f32;
    ((reversals as f32 / 4.0).clamp(0.0, 1.0)
        * ((mean_magnitude - 2.0) / 48.0).clamp(0.0, 1.0)
        * periodicity)
        .clamp(0.0, 1.0)
}

fn kill_process(child: &mut std::process::Child) {
    #[cfg(unix)]
    unsafe {
        let _ = libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.kill();
}

fn output_error(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_sine_produces_typed_non_placeholder_evidence() {
        let samples = (0..SAMPLE_RATE as usize)
            .map(|index| (2.0 * PI * 440.0 * index as f32 / SAMPLE_RATE as f32).sin() * 0.5)
            .collect::<Vec<_>>();
        let analyze = || {
            let mut processor = AcousticProcessor::new(2_000_000);
            for sample in &samples {
                processor.push(*sample).unwrap();
            }
            processor.finish(samples.len() as u64).unwrap();
            processor.frames
        };
        let first = analyze();
        let second = analyze();
        assert_eq!(first, second);
        assert_eq!(first[0].start, 2_000_000);
        assert!(first[0].rms > 0.3);
        assert!(first[0].periodicity > 0.8);
        assert!(first[0].snr_db.is_finite());
        assert!(first[0].spectral_flux.is_none());
        assert!(first[1].spectral_flux.is_some());
    }

    fn analyze_samples(samples: &[f32]) -> Vec<AcousticEvidenceFrameV1> {
        let mut processor = AcousticProcessor::new(0);
        for sample in samples {
            processor.push(*sample).unwrap();
        }
        processor.finish(samples.len() as u64).unwrap();
        processor.frames
    }

    fn swept_tone(mut frequency: impl FnMut(f32) -> f32, seconds: f32) -> Vec<f32> {
        let count = (SAMPLE_RATE as f32 * seconds) as usize;
        let mut phase = 0.0_f32;
        (0..count)
            .map(|index| {
                let time = index as f32 / SAMPLE_RATE as f32;
                phase += 2.0 * PI * frequency(time) / SAMPLE_RATE as f32;
                phase.sin() * 0.5
            })
            .collect()
    }

    #[test]
    fn generated_expressive_fixtures_remain_source_local_and_deterministic() {
        let steady = analyze_samples(&swept_tone(|_| 440.0, 1.0));
        assert!(
            steady
                .iter()
                .filter_map(|frame| frame.fundamental_hz)
                .count()
                > 50
        );
        assert!(
            steady
                .iter()
                .skip(12)
                .all(|frame| frame.vibrato_activation < 0.05)
        );

        let vibrato = analyze_samples(&swept_tone(
            |time| 440.0 * 2.0_f32.powf((45.0 * (2.0 * PI * 5.5 * time).sin()) / 1_200.0),
            1.5,
        ));
        assert!(
            vibrato
                .iter()
                .map(|frame| frame.vibrato_activation)
                .max_by(f32::total_cmp)
                .unwrap()
                > 0.05
        );

        let glide = analyze_samples(&swept_tone(
            |time| 220.0 * 2.0_f32.powf(time.clamp(0.0, 1.0)),
            1.0,
        ));
        assert!(
            glide
                .iter()
                .map(|frame| frame.glide_activation)
                .max_by(f32::total_cmp)
                .unwrap()
                > 0.05
        );

        let mut state = 0x1234_5678_u32;
        let noise = (0..SAMPLE_RATE as usize)
            .map(|_| {
                state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                (state as f32 / u32::MAX as f32 * 2.0 - 1.0) * 0.15
            })
            .collect::<Vec<_>>();
        let breath = analyze_samples(&noise);
        assert!(
            breath
                .iter()
                .map(|frame| frame.breath_activation)
                .max_by(f32::total_cmp)
                .unwrap()
                > 0.1
        );

        let silence = analyze_samples(&vec![0.0; SAMPLE_RATE as usize / 2]);
        assert!(silence.iter().all(|frame| {
            frame.fundamental_hz.is_none()
                && frame.breath_activation == 0.0
                && frame.vibrato_activation == 0.0
        }));

        let mut repeated = swept_tone(|_| 440.0, 0.4);
        repeated.extend(vec![0.0; SAMPLE_RATE as usize / 10]);
        repeated.extend(swept_tone(|_| 440.0, 0.4));
        let repeated = analyze_samples(&repeated);
        assert!(
            repeated
                .iter()
                .filter_map(|frame| frame.spectral_flux)
                .max_by(f32::total_cmp)
                .unwrap()
                > 0.001
        );
    }

    #[test]
    #[cfg(unix)]
    fn cancellation_kills_and_reaps_stalled_decode() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;

        let root = std::env::temp_dir().join(format!("uta-acoustic-cancel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let input = root.join("input.wav");
        std::fs::write(&input, b"fixture").unwrap();
        let ffmpeg = root.join("ffmpeg");
        let staging = root.join("ffmpeg.part");
        {
            let mut file = std::fs::File::create(&staging).unwrap();
            file.write_all(b"#!/bin/sh\nwhile :; do :; done\n").unwrap();
            file.sync_all().unwrap();
        }
        std::fs::set_permissions(&staging, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::rename(staging, &ffmpeg).unwrap();
        let cancellation = CancellationToken::default();
        let other = cancellation.clone();
        let canceller = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            other.cancel();
        });
        let started = std::time::Instant::now();
        let error =
            analyze_acoustic_evidence(&ffmpeg, &input, "lead_vocal", 0, 1_000_000, &cancellation)
                .unwrap_err();
        canceller.join().unwrap();
        assert_eq!(error.code, EngineErrorCode::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(2));
        std::fs::remove_dir_all(root).unwrap();
    }
}
