//! Native timestamp classification for caller-supplied, ordered word units.
//! Language segmentation belongs upstream; this is not a replacement tokenizer.

use std::time::Instant;

pub use super::alignment_windows::{AlignmentWindowTrace, AudioScope};
use super::frontend::Frontend;
use super::model::ModelKind;
use super::timestamps::correct_timestamp_ms;
use super::weights::Qwen;

const MAX_ALIGNMENT_SAMPLES: usize = 4 * 60 * 60 * super::frontend::SAMPLE_RATE;

#[derive(Clone, Debug)]
pub struct AlignedWord {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
    /// When present, start/end are the searched audio scope, not a word time.
    pub timing_issue: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Alignment {
    pub words: Vec<AlignedWord>,
    pub raw_classes: Vec<u32>,
    pub raw_timestamp_ms: Vec<u64>,
    pub corrected_timestamp_ms: Vec<u64>,
    pub windows: Vec<AlignmentWindowTrace>,
    pub prompt_tokens: usize,
    pub encoder_seconds: f64,
    pub decoder_seconds: f64,
}

impl Qwen {
    pub fn align_wav(
        &self,
        wav: &std::path::Path,
        words: &[String],
        scopes: &[Option<AudioScope>],
        report: &mut dyn FnMut(u64, u64),
    ) -> Result<Alignment, String> {
        let samples = crate::wav::read_f32_wav(wav, super::frontend::SAMPLE_RATE as u32, 1)?;
        self.align_scoped_words(&samples, words, scopes, report)
    }

    pub fn align_long_words(
        &self,
        samples: &[f32],
        words: &[String],
        report: &mut dyn FnMut(u64, u64),
    ) -> Result<Alignment, String> {
        self.align_scoped_words(samples, words, &vec![None; words.len()], report)
    }

    pub fn align_scoped_words(
        &self,
        samples: &[f32],
        words: &[String],
        scopes: &[Option<AudioScope>],
        report: &mut dyn FnMut(u64, u64),
    ) -> Result<Alignment, String> {
        if samples.len() > MAX_ALIGNMENT_SAMPLES {
            return Err("Qwen alignment input exceeds the four-hour contract limit".to_string());
        }
        let period = self
            .config
            .timestamp_millis
            .ok_or("Qwen timestamp period is missing")?;
        // Encoder attention windows partition acoustic visibility, not the
        // transcript. Keep each caller scope together while the timestamp head
        // can represent it; its class count and period supply the actual range.
        let maximum = timestamp_context_samples(
            self.config
                .timestamp_classes
                .ok_or("Qwen timestamp classifier width is missing")?,
            period,
        )?;
        super::alignment_windows::align_scoped(
            samples,
            words,
            scopes,
            maximum,
            period as u64,
            |audio, units| self.align_words(audio, units),
            report,
        )
    }

    pub fn align_words(&self, samples: &[f32], words: &[String]) -> Result<Alignment, String> {
        if self.config.kind != ModelKind::Aligner {
            return Err("Qwen word alignment requires the timestamp classifier model".to_string());
        }
        if words.is_empty() {
            return Ok(Alignment {
                words: Vec::new(),
                raw_classes: Vec::new(),
                raw_timestamp_ms: Vec::new(),
                corrected_timestamp_ms: Vec::new(),
                windows: Vec::new(),
                prompt_tokens: 0,
                encoder_seconds: 0.0,
                decoder_seconds: 0.0,
            });
        }
        if samples.len() < super::frontend::HOP {
            return Err("Qwen word alignment requires at least one audio frame".to_string());
        }

        let frontend = Frontend::whisper(self.config.mel_bins);
        let mel = frontend.compute(samples);
        let encoder_started = Instant::now();
        let audio = self.encode_audio(&mel)?;
        let encoder_seconds = encoder_started.elapsed().as_secs_f64();

        let mut tokens = vec![self.config.audio_start];
        tokens.extend(std::iter::repeat_n(self.config.audio_pad, audio.rows));
        tokens.push(self.config.audio_end);
        let timestamp = self
            .config
            .timestamp_token
            .ok_or("Qwen timestamp token is missing")?;
        let mut selected = Vec::with_capacity(words.len() * 2);
        for word in words {
            tokens.extend(self.tokenizer.encode(word)?);
            selected.push(tokens.len());
            tokens.push(timestamp);
            selected.push(tokens.len());
            tokens.push(timestamp);
        }

        let decoder_started = Instant::now();
        let logits = self.classify_prompt(&tokens, Some((&audio, 1)), &selected)?;
        drop(audio); // The classifier has consumed the retained encoder output.
        let decoder_seconds = decoder_started.elapsed().as_secs_f64();
        let raw_classes = logits
            .values
            .chunks_exact(logits.classes)
            .map(argmax)
            .collect::<Result<Vec<_>, _>>()?;
        let period_ms = u64::try_from(
            self.config
                .timestamp_millis
                .ok_or("Qwen timestamp period is missing")?,
        )
        .map_err(|_| "Qwen timestamp period is outside u64".to_string())?;
        let raw_timestamp_ms = raw_classes
            .iter()
            .map(|class| {
                u64::from(*class)
                    .checked_mul(period_ms)
                    .ok_or_else(|| "Qwen timestamp multiplication overflow".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let corrected_timestamp_ms = correct_timestamp_ms(&raw_timestamp_ms);
        let aligned = words
            .iter()
            .enumerate()
            .map(|(index, word)| AlignedWord {
                text: word.clone(),
                start_seconds: corrected_timestamp_ms[index * 2] as f64 / 1_000.0,
                end_seconds: corrected_timestamp_ms[index * 2 + 1] as f64 / 1_000.0,
                timing_issue: None,
            })
            .collect();
        let mut alignment = Alignment {
            words: aligned,
            raw_classes,
            raw_timestamp_ms,
            corrected_timestamp_ms,
            windows: Vec::new(),
            prompt_tokens: tokens.len(),
            encoder_seconds,
            decoder_seconds,
        };
        super::alignment_windows::resolve_local_timing(&mut alignment, samples.len(), period_ms);
        Ok(alignment)
    }
}

fn timestamp_context_samples(classes: usize, period_ms: usize) -> Result<usize, String> {
    classes
        .checked_mul(period_ms)
        .and_then(|millis| millis.checked_mul(super::frontend::SAMPLE_RATE))
        .map(|samples| samples / 1_000)
        .filter(|samples| *samples > 0)
        .ok_or_else(|| "Qwen timestamp context geometry overflows or is empty".to_string())
}

fn argmax(row: &[f32]) -> Result<u32, String> {
    let index = row
        .iter()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(right.1))
        .ok_or("Qwen timestamp classifier returned an empty row")?
        .0;
    u32::try_from(index).map_err(|_| "Qwen timestamp class is outside u32".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argmax_is_deterministic_and_rejects_empty_rows() {
        assert_eq!(argmax(&[1.0, 4.0, 4.0, 2.0]).unwrap(), 2);
        assert!(argmax(&[]).is_err());
    }

    #[test]
    fn a_whole_song_scope_reaches_the_decoder_with_its_complete_transcript() {
        let maximum = timestamp_context_samples(5_000, 80).unwrap();
        assert_eq!(maximum, 400 * super::super::frontend::SAMPLE_RATE);
        let samples = vec![0.0; 55 * super::super::frontend::SAMPLE_RATE];
        let words = vec!["unit".to_string(); 108];
        let scopes = vec![None; words.len()];
        let mut calls = 0;
        let aligned = super::super::alignment_windows::align_scoped(
            &samples,
            &words,
            &scopes,
            maximum,
            80,
            |audio, units| {
                calls += 1;
                assert_eq!(audio.len(), samples.len());
                assert_eq!(units, words.as_slice());
                let raw_timestamp_ms = (0..units.len())
                    .flat_map(|index| [index as u64 * 480, index as u64 * 480 + 400])
                    .collect::<Vec<_>>();
                Ok(Alignment {
                    words: units
                        .iter()
                        .enumerate()
                        .map(|(index, word)| AlignedWord {
                            text: word.clone(),
                            start_seconds: index as f64 * 0.48,
                            end_seconds: index as f64 * 0.48 + 0.4,
                            timing_issue: None,
                        })
                        .collect(),
                    raw_classes: raw_timestamp_ms
                        .iter()
                        .map(|value| (value / 80) as u32)
                        .collect(),
                    corrected_timestamp_ms: raw_timestamp_ms.clone(),
                    raw_timestamp_ms,
                    windows: Vec::new(),
                    prompt_tokens: units.len() * 3,
                    encoder_seconds: 0.0,
                    decoder_seconds: 0.0,
                })
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(calls, 1);
        assert_eq!(aligned.words.len(), words.len());
        assert!(aligned.words.iter().all(|word| word.timing_issue.is_none()));
    }

    #[test]
    fn timestamp_context_tracks_the_loaded_head_geometry() {
        assert_eq!(timestamp_context_samples(1_000, 40).unwrap(), 40 * 16_000);
        assert!(timestamp_context_samples(usize::MAX, 80).is_err());
    }
}
