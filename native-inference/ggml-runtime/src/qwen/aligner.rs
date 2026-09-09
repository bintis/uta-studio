//! Native timestamp classification for caller-supplied, ordered word units.
//! Language segmentation belongs upstream; this is not a replacement tokenizer.

use std::time::Instant;

use super::frontend::Frontend;
use super::model::ModelKind;
use super::timestamps::correct_timestamp_ms;
use super::weights::Qwen;

const MAX_ALIGNMENT_SAMPLES: usize = 4 * 60 * 60 * super::frontend::SAMPLE_RATE;
const WINDOW_MARGIN_SAMPLES: usize = super::frontend::SAMPLE_RATE;

#[derive(Clone, Debug)]
pub struct AlignedWord {
    pub text: String,
    pub start_seconds: f64,
    pub end_seconds: f64,
}

#[derive(Clone, Debug)]
pub struct Alignment {
    pub words: Vec<AlignedWord>,
    pub raw_classes: Vec<u32>,
    pub raw_timestamp_ms: Vec<u64>,
    pub corrected_timestamp_ms: Vec<u64>,
    pub prompt_tokens: usize,
    pub encoder_seconds: f64,
    pub decoder_seconds: f64,
}

impl Qwen {
    pub fn align_wav(&self, wav: &std::path::Path, words: &[String]) -> Result<Alignment, String> {
        let samples = crate::wav::read_f32_wav(wav, super::frontend::SAMPLE_RATE as u32, 1)?;
        self.align_long_words(&samples, words)
    }

    /// Bounds song-length encoder attention. Word groups are assigned to
    /// deterministic core windows by cumulative text weight, while one-second
    /// audio margins protect boundaries at each split. Caller timing remains
    /// an Analysis Engine concern and can replace this coarse partition in a
    /// later calibrated scheduler without changing the worker contract.
    pub fn align_long_words(&self, samples: &[f32], words: &[String]) -> Result<Alignment, String> {
        if samples.len() > MAX_ALIGNMENT_SAMPLES {
            return Err("Qwen alignment input exceeds the four-hour contract limit".to_string());
        }
        let window_samples = self
            .config
            .encoder_window_mel
            .checked_mul(super::frontend::HOP)
            .ok_or("Qwen alignment window size overflow")?;
        if samples.len() <= window_samples {
            return self.align_words(samples, words);
        }
        let margin = WINDOW_MARGIN_SAMPLES.min(window_samples / 4);
        let cores = alignment_core_windows(samples.len(), window_samples, margin)?;
        let assignments = assign_words_to_windows(words, samples.len(), &cores)?;
        let mut aligned_words = Vec::with_capacity(words.len());
        let mut raw_classes = Vec::with_capacity(words.len() * 2);
        let mut raw_timestamp_ms = Vec::with_capacity(words.len() * 2);
        let mut corrected_timestamp_ms = Vec::with_capacity(words.len() * 2);
        let mut prompt_tokens = 0usize;
        let mut encoder_seconds = 0.0;
        let mut decoder_seconds = 0.0;
        for ((core_start, core_end), assigned) in cores.into_iter().zip(assignments) {
            if assigned.is_empty() {
                continue;
            }
            let audio_start = core_start.saturating_sub(margin);
            let audio_end = core_end.saturating_add(margin).min(samples.len());
            let text = assigned
                .iter()
                .map(|&index| words[index].clone())
                .collect::<Vec<_>>();
            let local = self.align_words(&samples[audio_start..audio_end], &text)?;
            let offset_ms = u64::try_from(audio_start)
                .map_err(|_| "Qwen alignment window offset exceeds u64")?
                .saturating_mul(1_000)
                / super::frontend::SAMPLE_RATE as u64;
            aligned_words.extend(local.words.into_iter().map(|mut word| {
                word.start_seconds += offset_ms as f64 / 1_000.0;
                word.end_seconds += offset_ms as f64 / 1_000.0;
                word
            }));
            raw_classes.extend(local.raw_classes);
            raw_timestamp_ms.extend(
                local
                    .raw_timestamp_ms
                    .into_iter()
                    .map(|time| time.saturating_add(offset_ms)),
            );
            corrected_timestamp_ms.extend(
                local
                    .corrected_timestamp_ms
                    .into_iter()
                    .map(|time| time.saturating_add(offset_ms)),
            );
            prompt_tokens = prompt_tokens
                .checked_add(local.prompt_tokens)
                .ok_or("Qwen alignment prompt-token total overflow")?;
            encoder_seconds += local.encoder_seconds;
            decoder_seconds += local.decoder_seconds;
        }
        if aligned_words.len() != words.len() {
            return Err("Qwen alignment windowing changed the word count".to_string());
        }
        let source_ms = u64::try_from(samples.len())
            .map_err(|_| "Qwen alignment sample count exceeds u64")?
            .saturating_mul(1_000)
            / super::frontend::SAMPLE_RATE as u64;
        let mut corrected = correct_timestamp_ms(&corrected_timestamp_ms);
        repair_word_ranges(&mut corrected, source_ms)?;
        for (index, word) in aligned_words.iter_mut().enumerate() {
            word.start_seconds = corrected[index * 2] as f64 / 1_000.0;
            word.end_seconds = corrected[index * 2 + 1] as f64 / 1_000.0;
        }
        Ok(Alignment {
            words: aligned_words,
            raw_classes,
            raw_timestamp_ms,
            corrected_timestamp_ms: corrected,
            prompt_tokens,
            encoder_seconds,
            decoder_seconds,
        })
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
            })
            .collect();
        Ok(Alignment {
            words: aligned,
            raw_classes,
            raw_timestamp_ms,
            corrected_timestamp_ms,
            prompt_tokens: tokens.len(),
            encoder_seconds,
            decoder_seconds,
        })
    }
}

fn alignment_core_windows(
    samples: usize,
    window_samples: usize,
    margin: usize,
) -> Result<Vec<(usize, usize)>, String> {
    let core = window_samples
        .checked_sub(margin.saturating_mul(2))
        .filter(|core| *core > 0)
        .ok_or("Qwen alignment window is too short for its margins")?;
    if samples == 0 {
        return Err("Qwen alignment input is empty".to_string());
    }
    let mut windows = Vec::new();
    let mut start = 0usize;
    while start < samples {
        let end = start.saturating_add(core).min(samples);
        windows.push((start, end));
        start = end;
    }
    Ok(windows)
}

fn assign_words_to_windows(
    words: &[String],
    samples: usize,
    windows: &[(usize, usize)],
) -> Result<Vec<Vec<usize>>, String> {
    if words.is_empty() || windows.is_empty() || samples == 0 {
        return Err("Qwen alignment window assignment is empty".to_string());
    }
    let weights = words
        .iter()
        .map(|word| {
            word.chars()
                .filter(|value| !value.is_whitespace())
                .count()
                .max(1)
        })
        .collect::<Vec<_>>();
    let total = weights
        .iter()
        .try_fold(0usize, |sum, weight| sum.checked_add(*weight))
        .ok_or("Qwen alignment text weight overflow")?;
    let denominator = total
        .checked_mul(2)
        .ok_or("Qwen alignment text weight overflow")?;
    let mut assignments = vec![Vec::new(); windows.len()];
    let mut preceding = 0usize;
    for (word_index, weight) in weights.into_iter().enumerate() {
        let midpoint_twice = preceding
            .checked_mul(2)
            .and_then(|value| value.checked_add(weight))
            .ok_or("Qwen alignment text position overflow")?;
        let sample = (midpoint_twice as u128 * samples as u128 / denominator as u128)
            .min(samples.saturating_sub(1) as u128) as usize;
        let window = windows
            .iter()
            .position(|(start, end)| (*start..*end).contains(&sample))
            .ok_or("Qwen alignment text position missed every audio window")?;
        assignments[window].push(word_index);
        preceding = preceding
            .checked_add(weight)
            .ok_or("Qwen alignment text position overflow")?;
    }
    Ok(assignments)
}

fn repair_word_ranges(timestamps: &mut [u64], source_ms: u64) -> Result<(), String> {
    if source_ms == 0 || !timestamps.len().is_multiple_of(2) {
        return Err("Qwen alignment repaired timestamp shape is invalid".to_string());
    }
    let mut previous_end = 0u64;
    for pair in timestamps.chunks_exact_mut(2) {
        let start = pair[0].max(previous_end).min(source_ms);
        if start >= source_ms {
            return Err("Qwen alignment has no source time left for a word".to_string());
        }
        let end = pair[1].max(start.saturating_add(1)).min(source_ms);
        if end <= start {
            return Err("Qwen alignment produced an empty word range".to_string());
        }
        pair[0] = start;
        pair[1] = end;
        previous_end = end;
    }
    Ok(())
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
    fn long_alignment_windows_are_bounded_and_assign_every_word_once() {
        let windows = alignment_core_windows(2_500, 1_200, 100).unwrap();
        assert_eq!(windows, [(0, 1_000), (1_000, 2_000), (2_000, 2_500)]);
        let words = ["one".to_string(), "two".to_string(), "three".to_string()];
        let assigned = assign_words_to_windows(&words, 2_500, &windows).unwrap();
        assert_eq!(
            assigned.iter().flatten().copied().collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert!(assigned.iter().all(|group| group.len() <= words.len()));
    }

    #[test]
    fn repaired_window_boundaries_are_positive_ordered_and_source_bounded() {
        let mut timestamps = vec![0, 20, 10, 10, 90, 200];
        repair_word_ranges(&mut timestamps, 100).unwrap();
        assert_eq!(timestamps, [0, 20, 20, 21, 90, 100]);
        assert!(repair_word_ranges(&mut [100, 100], 100).is_err());
    }
}
