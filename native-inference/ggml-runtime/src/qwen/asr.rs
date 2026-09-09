use std::time::Instant;

use super::frontend::Frontend;
use super::model::ModelKind;
use super::weights::Qwen;

pub const DEFAULT_MAX_NEW_TOKENS: usize = 256;
const MAX_ASR_SAMPLES: usize = 4 * 60 * 60 * super::frontend::SAMPLE_RATE;
const WINDOW_OVERLAP_SAMPLES: usize = super::frontend::SAMPLE_RATE;

#[derive(Debug)]
pub struct Transcription {
    pub text: String,
    /// Publisher language name; the Analysis Engine owns BCP-47 mapping.
    pub language_name: Option<String>,
    pub raw_text: String,
    pub generated_tokens: Vec<u32>,
    pub finished: bool,
    /// Windows whose decode ran to the token budget without predicting EOS and
    /// were therefore dropped. Zero for a single-window input, which fails
    /// instead because there is nothing else to keep.
    pub unfinished_windows: usize,
    pub prompt_tokens: usize,
    pub encoder_seconds: f64,
    pub decoder_seconds: f64,
}

impl Qwen {
    pub fn transcribe_wav(
        &self,
        wav: &std::path::Path,
        max_new_tokens: usize,
    ) -> Result<Transcription, String> {
        let samples = crate::wav::read_f32_wav(wav, super::frontend::SAMPLE_RATE as u32, 1)?;
        self.transcribe_long(&samples, max_new_tokens)
    }

    /// Runs bounded overlapping windows so a song-length input cannot turn
    /// encoder attention or the per-window decoder budget into an implicit
    /// whole-track truncation. Adjacent text is joined by deterministic
    /// suffix/prefix reconciliation; every window must reach EOS.
    pub fn transcribe_long(
        &self,
        samples: &[f32],
        max_new_tokens_per_window: usize,
    ) -> Result<Transcription, String> {
        if samples.len() > MAX_ASR_SAMPLES {
            return Err("Qwen ASR input exceeds the four-hour contract limit".to_string());
        }
        let window_samples = self
            .config
            .encoder_window_mel
            .checked_mul(super::frontend::HOP)
            .ok_or("Qwen ASR window size overflow")?;
        let windows = sample_windows(
            samples.len(),
            window_samples,
            WINDOW_OVERLAP_SAMPLES.min(window_samples / 4),
        )?;
        if windows.len() == 1 {
            let single = self.transcribe(samples, max_new_tokens_per_window)?;
            if !single.finished {
                return Err(format!(
                    "Qwen ASR exhausted its {max_new_tokens_per_window}-token decoder budget after {} tokens on a single-window input",
                    single.generated_tokens.len()
                ));
            }
            return Ok(single);
        }
        let mut text = String::new();
        let mut language_name: Option<String> = None;
        let mut raw_text = Vec::with_capacity(windows.len());
        let mut generated_tokens = Vec::new();
        let mut prompt_tokens = 0usize;
        let mut encoder_seconds = 0.0;
        let mut decoder_seconds = 0.0;
        let window_count = windows.len();
        let mut unfinished_windows = 0usize;
        for (start, end) in windows.into_iter() {
            let window = self.transcribe(&samples[start..end], max_new_tokens_per_window)?;
            if !window.finished {
                // A song is not speech from end to end. Intros, solos and
                // outros give the decoder nothing to transcribe, and an
                // attention decoder with nothing to say does not reliably
                // predict EOS -- it runs to the budget instead. Raising the
                // budget only buys more of the same output, so the window is
                // dropped and counted. Failing the whole track here would make
                // every real song untranscribable.
                unfinished_windows += 1;
                encoder_seconds += window.encoder_seconds;
                decoder_seconds += window.decoder_seconds;
                continue;
            }
            if let Some(detected) = window.language_name.as_deref() {
                if language_name
                    .as_deref()
                    .is_some_and(|previous| !previous.eq_ignore_ascii_case(detected))
                {
                    return Err(format!(
                        "Qwen ASR language changed between windows: {} then {detected}",
                        language_name.as_deref().unwrap_or_default()
                    ));
                }
                language_name.get_or_insert_with(|| detected.to_string());
            }
            text = merge_transcript_text(&text, &window.text);
            raw_text.push(window.raw_text);
            generated_tokens.extend(window.generated_tokens);
            prompt_tokens = prompt_tokens
                .checked_add(window.prompt_tokens)
                .ok_or("Qwen ASR prompt-token total overflow")?;
            encoder_seconds += window.encoder_seconds;
            decoder_seconds += window.decoder_seconds;
        }
        if unfinished_windows == window_count {
            return Err(format!(
                "Qwen ASR exhausted its {max_new_tokens_per_window}-token decoder budget on all {window_count} windows"
            ));
        }
        Ok(Transcription {
            text,
            language_name,
            raw_text: raw_text.join("\n<window>\n"),
            generated_tokens,
            finished: true,
            unfinished_windows,
            prompt_tokens,
            encoder_seconds,
            decoder_seconds,
        })
    }

    pub fn transcribe(
        &self,
        samples: &[f32],
        max_new_tokens: usize,
    ) -> Result<Transcription, String> {
        if self.config.kind != ModelKind::Asr {
            return Err("Qwen transcription requires the ASR model".to_string());
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err("Qwen ASR input contains a non-finite sample".to_string());
        }
        let mel = Frontend::whisper(self.config.mel_bins).compute(samples);
        let encoder_start = Instant::now();
        let audio = self.encode_audio(&mel)?;
        let encoder_seconds = encoder_start.elapsed().as_secs_f64();
        let (prompt, audio_offset) = build_prompt(self, audio.rows)?;
        let prompt_tokens = prompt.len();
        let decoder_start = Instant::now();
        let eos = self.tokenizer.id("<|im_end|>")?;
        let mut generated_tokens = Vec::with_capacity(max_new_tokens.min(256));
        let mut finished = false;
        if max_new_tokens != 0 {
            let capacity = prompt_tokens
                .checked_add(max_new_tokens)
                .ok_or("Qwen ASR decoder capacity overflow")?;
            let mut decoder = self.decoder_session(capacity)?;
            let mut logits = decoder.decode(&prompt, Some((&audio, audio_offset)))?;
            for step in 0..max_new_tokens {
                let token = argmax(&logits.values)?;
                generated_tokens.push(token);
                if token == eos {
                    finished = true;
                    break;
                }
                if step + 1 < max_new_tokens {
                    logits = decoder.decode(&[token], None)?;
                }
            }
        }
        let decoder_seconds = decoder_start.elapsed().as_secs_f64();
        let raw_text = self.tokenizer.decode(&generated_tokens, true)?;
        let (language_name, text) = parse_answer(&raw_text);
        Ok(Transcription {
            text,
            language_name,
            raw_text,
            generated_tokens,
            finished,
            unfinished_windows: 0,
            prompt_tokens,
            encoder_seconds,
            decoder_seconds,
        })
    }
}

pub(super) fn build_prompt(model: &Qwen, audio_rows: usize) -> Result<(Vec<u32>, usize), String> {
    if model.config.kind != ModelKind::Asr {
        return Err("Qwen ASR prompt requires the ASR model".to_string());
    }
    let mut prompt = model
        .tokenizer
        .encode("<|im_start|>system\n<|im_end|>\n<|im_start|>user\n")?;
    prompt.push(model.config.audio_start);
    let audio_offset = prompt.len();
    prompt.extend(std::iter::repeat_n(model.config.audio_pad, audio_rows));
    prompt.push(model.config.audio_end);
    prompt.extend(
        model
            .tokenizer
            .encode("<|im_end|>\n<|im_start|>assistant\n")?,
    );
    Ok((prompt, audio_offset))
}

pub(super) fn argmax(values: &[f32]) -> Result<u32, String> {
    let mut best = 0;
    if values.is_empty() {
        return Err("Qwen ASR output logits are empty".to_string());
    }
    for (index, &value) in values.iter().enumerate() {
        if !value.is_finite() {
            return Err(format!("Qwen ASR output logit {index} is not finite"));
        }
        if value > values[best] {
            best = index;
        }
    }
    u32::try_from(best).map_err(|_| "Qwen ASR output index exceeds u32".to_string())
}

fn sample_windows(
    samples: usize,
    window_samples: usize,
    overlap_samples: usize,
) -> Result<Vec<(usize, usize)>, String> {
    if samples == 0 || window_samples == 0 || overlap_samples >= window_samples {
        return Err("Qwen ASR window geometry is invalid".to_string());
    }
    if samples <= window_samples {
        return Ok(vec![(0, samples)]);
    }
    let step = window_samples - overlap_samples;
    let mut windows = Vec::new();
    let mut start = 0usize;
    loop {
        let end = start.saturating_add(window_samples).min(samples);
        windows.push((start, end));
        if end == samples {
            break;
        }
        start = start
            .checked_add(step)
            .ok_or("Qwen ASR window position overflow")?;
    }
    Ok(windows)
}

fn merge_transcript_text(left: &str, right: &str) -> String {
    let left = left.trim();
    let right = right.trim();
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }
    let left_words = left.split_whitespace().collect::<Vec<_>>();
    let right_words = right.split_whitespace().collect::<Vec<_>>();
    if left_words.len() > 1 || right_words.len() > 1 {
        let overlap = (1..=left_words.len().min(right_words.len()))
            .rev()
            .find(|&count| {
                left_words[left_words.len() - count..]
                    .iter()
                    .zip(&right_words[..count])
                    .all(|(left, right)| left.eq_ignore_ascii_case(right))
            })
            .unwrap_or(0);
        return left_words
            .into_iter()
            .chain(right_words.into_iter().skip(overlap))
            .collect::<Vec<_>>()
            .join(" ");
    }
    let left_chars = left
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect::<Vec<_>>();
    let right_chars = right
        .chars()
        .filter(|value| !value.is_whitespace())
        .collect::<Vec<_>>();
    let overlap = (2..=left_chars.len().min(right_chars.len()))
        .rev()
        .find(|&count| left_chars[left_chars.len() - count..] == right_chars[..count])
        .unwrap_or(0);
    left_chars
        .into_iter()
        .chain(right_chars.into_iter().skip(overlap))
        .collect()
}

pub(super) fn parse_answer(raw: &str) -> (Option<String>, String) {
    match raw.split_once("<asr_text>") {
        Some((prefix, text)) => {
            let language = prefix
                .trim()
                .strip_prefix("language ")
                .map(str::trim)
                .filter(|language| !language.is_empty() && *language != "None")
                .map(str::to_owned);
            (language, text.trim().to_owned())
        }
        None => (None, raw.trim().to_owned()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DeviceKind, GgmlRuntime};
    use std::path::PathBuf;

    fn path(name: &str) -> PathBuf {
        std::env::var_os(name)
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("set {name}"))
    }

    #[test]
    fn answer_parser_and_argmax_preserve_asr_semantics() {
        assert_eq!(argmax(&[-2.0, 3.0, 3.0]).unwrap(), 1);
        assert!(argmax(&[f32::NAN]).is_err());
        assert!(argmax(&[]).is_err());
        assert_eq!(
            parse_answer("language English<asr_text> All he just is for us. "),
            (Some("English".into()), "All he just is for us.".into())
        );
        assert_eq!(
            parse_answer("language None<asr_text>"),
            (None, String::new())
        );
    }

    #[test]
    fn long_input_windows_are_bounded_and_text_overlap_is_reconciled() {
        assert_eq!(
            sample_windows(25, 10, 2).unwrap(),
            [(0, 10), (8, 18), (16, 25)]
        );
        assert_eq!(
            merge_transcript_text("we sing this song", "this song together"),
            "we sing this song together"
        );
        assert_eq!(
            merge_transcript_text("风吹沙蝶恋", "沙蝶恋花"),
            "风吹沙蝶恋花"
        );
        assert!(sample_windows(10, 10, 10).is_err());
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, Qwen ASR GGUF, and raw 16 kHz F32 speech"]
    fn actual_asr_transcription_matches_historical_tokens() {
        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let requested_kind =
            std::env::var("UTA_TEST_GGML_DEVICE_KIND").expect("set UTA_TEST_GGML_DEVICE_KIND");
        let expected_kind = match requested_kind.as_str() {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION")
            .expect("set UTA_TEST_GGML_DEVICE_DESCRIPTION");
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested Qwen ASR test device is unavailable");
        let model = Qwen::load(runtime, &device, &path("UTA_TEST_QWEN_GGUF")).unwrap();
        let bytes = std::fs::read(path("UTA_TEST_QWEN_ASR_F32")).unwrap();
        assert!(bytes.len().is_multiple_of(4));
        let samples = bytes
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes(chunk.try_into().unwrap()))
            .collect::<Vec<_>>();
        let transcription = model.transcribe(&samples, DEFAULT_MAX_NEW_TOKENS).unwrap();
        assert_eq!(
            transcription.generated_tokens,
            [
                11_528, 6_364, 151_704, 2_403, 566, 1_101, 374, 369, 601, 13, 151_645
            ]
        );
        assert_eq!(transcription.language_name.as_deref(), Some("English"));
        assert_eq!(transcription.text, "All he just is for us.");
        assert!(transcription.finished);
        assert_eq!(transcription.prompt_tokens, 171);
    }
}
