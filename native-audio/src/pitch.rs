//! Synthesized pitch audition.
//!
//! Checking a chart by ear needs to answer "does this note ask for the pitch I
//! meant?", which the song audio cannot answer on its own. This renders the
//! chart's own note targets to a short tone track and plays it on a second,
//! independent stream, so the song audio is never altered, resampled, or mixed
//! into anything that gets written back.

use std::{
    path::PathBuf,
    sync::{
        Mutex,
        atomic::{AtomicU64, Ordering},
    },
};

use crate::EditorAudioPlayer;

const SAMPLE_RATE: u32 = 44_100;
/// Fade in and out of every tone. Long enough to remove the click of a hard
/// edge, short enough not to soften where a note begins.
const ATTACK_SECONDS: f64 = 0.008;
const RELEASE_SECONDS: f64 = 0.03;

/// One note to sound, positioned relative to the start of the preview.
#[derive(Debug, Clone, Copy)]
pub struct PitchTone {
    pub start_secs: f64,
    pub duration_secs: f64,
    pub midi: f64,
}

/// Renders tones as a 16-bit mono PCM WAV. Uncompressed and never stored: the
/// buffer exists only for as long as the audition plays.
pub fn render_pitch_preview(tones: &[PitchTone], duration_secs: f64, volume: f64) -> Vec<u8> {
    let rate = f64::from(SAMPLE_RATE);
    let total = ((duration_secs.max(0.0) * rate).ceil() as usize).min(rate as usize * 600);
    let mut samples = vec![0f32; total];
    let volume = volume.clamp(0.0, 1.0) as f32;

    for tone in tones {
        if !tone.start_secs.is_finite() || !tone.duration_secs.is_finite() || !tone.midi.is_finite()
        {
            continue;
        }
        let frequency = 440.0 * 2f64.powf((tone.midi - 69.0) / 12.0);
        if !(20.0..=12_000.0).contains(&frequency) {
            continue;
        }
        let start = (tone.start_secs.max(0.0) * rate).round() as usize;
        let length = (tone.duration_secs.max(0.0) * rate).round() as usize;
        for offset in 0..length {
            let Some(sample) = samples.get_mut(start + offset) else {
                break;
            };
            let seconds = offset as f64 / rate;
            let remaining = (length - offset) as f64 / rate;
            let envelope = (seconds / ATTACK_SECONDS)
                .min(remaining / RELEASE_SECONDS)
                .clamp(0.0, 1.0);
            let phase = std::f64::consts::TAU * frequency * seconds;
            // A hint of the third harmonic makes the pitch easier to place by
            // ear than a bare sine without turning it into a buzz.
            let value = phase.sin() + 0.18 * (3.0 * phase).sin();
            *sample += (value * envelope) as f32 * 0.42 * volume;
        }
    }

    encode_wav(&samples)
}

fn encode_wav(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() * 2;
    let mut wav = Vec::with_capacity(44 + data_len);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + data_len) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data_len as u32).to_le_bytes());
    for sample in samples {
        let clamped = (sample.clamp(-1.0, 1.0) * f32::from(i16::MAX)) as i16;
        wav.extend_from_slice(&clamped.to_le_bytes());
    }
    wav
}

/// The second audio stream that plays rendered pitch tones.
pub struct PitchAudition {
    player: EditorAudioPlayer,
    rendered: Mutex<Option<PathBuf>>,
}

static PREVIEW_SEQUENCE: AtomicU64 = AtomicU64::new(0);

impl PitchAudition {
    pub fn new() -> Self {
        Self {
            player: EditorAudioPlayer::new(),
            rendered: Mutex::new(None),
        }
    }

    /// Renders and starts the tones. Returns without sounding anything when
    /// there is nothing pitched in range.
    pub fn start(
        &self,
        tones: &[PitchTone],
        duration_secs: f64,
        volume: f64,
    ) -> Result<(), String> {
        self.stop();
        if tones.is_empty() || duration_secs <= 0.0 {
            return Ok(());
        }
        let wav = render_pitch_preview(tones, duration_secs, volume);
        let sequence = PREVIEW_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "uta-studio-pitch-{}-{sequence}.wav",
            std::process::id()
        ));
        std::fs::write(&path, &wav)
            .map_err(|error| format!("Could not prepare the pitch preview: {error}"))?;
        let started = self
            .player
            .load_path(&path)
            .and_then(|_| self.player.play());
        match started {
            Ok(_) => {
                if let Ok(mut rendered) = self.rendered.lock() {
                    *rendered = Some(path);
                }
                Ok(())
            }
            Err(error) => {
                let _ = std::fs::remove_file(&path);
                Err(error)
            }
        }
    }

    /// Stops the tones and removes the rendered buffer.
    pub fn stop(&self) {
        let _ = self.player.stop();
        if let Ok(mut rendered) = self.rendered.lock()
            && let Some(path) = rendered.take()
        {
            let _ = std::fs::remove_file(path);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.player
            .status()
            .map(|status| status.playing && !status.ended)
            .unwrap_or(false)
    }
}

impl Default for PitchAudition {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PitchAudition {
    fn drop(&mut self) {
        self.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(start: f64, duration: f64, midi: f64) -> PitchTone {
        PitchTone {
            start_secs: start,
            duration_secs: duration,
            midi,
        }
    }

    fn samples(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect()
    }

    #[test]
    fn the_preview_is_a_playable_mono_wav_of_the_requested_length() {
        let wav = render_pitch_preview(&[tone(0.0, 0.5, 69.0)], 1.0, 1.0);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u16::from_le_bytes([wav[22], wav[23]]), 1, "mono");
        assert_eq!(
            u32::from_le_bytes([wav[24], wav[25], wav[26], wav[27]]),
            SAMPLE_RATE
        );
        assert_eq!(samples(&wav).len(), SAMPLE_RATE as usize);
    }

    #[test]
    fn silence_outside_a_tone_stays_silent() {
        let wav = render_pitch_preview(&[tone(0.0, 0.2, 60.0)], 0.5, 1.0);
        let samples = samples(&wav);
        let tail = &samples[(SAMPLE_RATE as f64 * 0.3) as usize..];
        assert!(tail.iter().all(|sample| *sample == 0));
        let head = &samples[..(SAMPLE_RATE as f64 * 0.1) as usize];
        assert!(head.iter().any(|sample| sample.abs() > 1_000));
    }

    #[test]
    fn a_tone_sounds_at_the_pitch_the_note_asks_for() {
        // Count zero crossings over a steady second of A4 and expect 440 Hz.
        let wav = render_pitch_preview(&[tone(0.0, 1.0, 69.0)], 1.0, 1.0);
        let samples = samples(&wav);
        let steady =
            &samples[(SAMPLE_RATE as f64 * 0.05) as usize..(SAMPLE_RATE as f64 * 0.95) as usize];
        let crossings = steady
            .windows(2)
            .filter(|pair| (pair[0] < 0) != (pair[1] < 0))
            .count();
        let hz = crossings as f64 / 2.0 / 0.9;
        assert!((hz - 440.0).abs() < 6.0, "measured {hz} Hz");
    }

    #[test]
    fn tones_never_clip_when_they_overlap() {
        let wav = render_pitch_preview(
            &[
                tone(0.0, 1.0, 60.0),
                tone(0.0, 1.0, 64.0),
                tone(0.0, 1.0, 67.0),
            ],
            1.0,
            1.0,
        );
        assert!(samples(&wav).iter().all(|sample| *sample != i16::MIN));
    }

    #[test]
    fn a_silent_request_renders_nothing_to_play() {
        assert!(render_pitch_preview(&[], 0.0, 1.0).len() == 44);
    }
}
