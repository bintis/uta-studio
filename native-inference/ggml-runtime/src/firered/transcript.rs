use super::{
    EOS, FEATURE_FRAMES, FireRed, MAX_WINDOW_SAMPLES, MIN_WINDOW_SAMPLES, SAMPLE_RATE, SOS,
    VOCAB_SIZE, extract_features,
};

const MAX_INPUT_SAMPLES: usize = 4 * 60 * 60 * SAMPLE_RATE;
pub const WINDOW_OVERLAP_SAMPLES: usize = SAMPLE_RATE;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptWindow {
    pub index: usize,
    pub start_sample: usize,
    pub end_sample: usize,
    pub text: String,
    pub token_ids: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transcription {
    pub text: String,
    pub token_ids: Vec<u32>,
    pub windows: Vec<TranscriptWindow>,
    /// Windows whose decode never predicted EOS and contributed no text. A
    /// song is not speech end to end, so this is expected to be non-zero.
    pub unfinished_windows: usize,
}

impl FireRed {
    pub fn transcribe_wav(
        &self,
        wav: &std::path::Path,
        cmvn: &[u8],
        vocabulary: &[u8],
        progress: impl FnMut(u64, u64),
    ) -> Result<Transcription, String> {
        let samples = crate::wav::read_f32_wav(wav, SAMPLE_RATE as u32, 1)?;
        self.transcribe_long(&samples, cmvn, vocabulary, progress)
    }

    /// Transcribes up to four hours as bounded overlapping FireRed windows.
    /// Each window must reach EOS. Overlap is reconciled by the longest exact
    /// token suffix/prefix so model-owned token identity and assembled text
    /// cannot diverge.
    pub fn transcribe_long(
        &self,
        samples: &[f32],
        cmvn: &[u8],
        vocabulary: &[u8],
        mut progress: impl FnMut(u64, u64),
    ) -> Result<Transcription, String> {
        if samples.is_empty() || samples.len() > MAX_INPUT_SAMPLES {
            return Err(
                "FireRed input must contain between one sample and four hours of audio".to_string(),
            );
        }
        if samples.iter().any(|sample| !sample.is_finite()) {
            return Err("FireRed input contains a non-finite sample".to_string());
        }
        let vocabulary = load_vocabulary(vocabulary)?;
        let ranges = sample_windows(samples.len(), MAX_WINDOW_SAMPLES, WINDOW_OVERLAP_SAMPLES)?;
        let total = ranges.len() as u64;
        let mut windows = Vec::with_capacity(ranges.len());
        let mut token_ids = Vec::new();
        let mut unfinished_windows = 0usize;
        for (index, (start, end)) in ranges.into_iter().enumerate() {
            let mut padded = vec![0.0_f32; (end - start).max(MIN_WINDOW_SAMPLES)];
            padded[..end - start].copy_from_slice(&samples[start..end]);
            let (features, feature_frames) = extract_features(&padded, cmvn)?;
            if feature_frames != FEATURE_FRAMES {
                return Err(format!(
                    "FireRed window {index} produced {feature_frames} feature frames instead of {FEATURE_FRAMES}"
                ));
            }
            let encoded = self.encode(&features)?;
            let generated = self.greedy_decode(&encoded)?;
            if generated.last() != Some(&EOS) {
                // An attention decoder with nothing to transcribe does not
                // predict EOS, it runs to the budget. Instrumental windows are
                // certain in a song, so the window contributes nothing and is
                // counted rather than failing the whole track.
                unfinished_windows += 1;
                windows.push(TranscriptWindow {
                    index,
                    start_sample: start,
                    end_sample: end,
                    text: String::new(),
                    token_ids: Vec::new(),
                });
                progress(index as u64 + 1, total);
                continue;
            }
            let (window_tokens, text) = decode_window(&generated, &vocabulary)?;
            merge_token_ids(&mut token_ids, &window_tokens);
            windows.push(TranscriptWindow {
                index,
                start_sample: start,
                end_sample: end,
                text,
                token_ids: window_tokens,
            });
            progress(index as u64 + 1, total);
        }
        let text = decode_lexical_tokens(&token_ids, &vocabulary);
        if text.is_empty() {
            return Err(format!(
                "FireRed produced no transcript across {} windows, {unfinished_windows} of which never reached EOS",
                windows.len()
            ));
        }
        Ok(Transcription {
            text,
            token_ids,
            windows,
            unfinished_windows,
        })
    }
}

fn load_vocabulary(bytes: &[u8]) -> Result<Vec<String>, String> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "FireRed token vocabulary is not UTF-8".to_string())?;
    let mut vocabulary = vec![String::new(); VOCAB_SIZE];
    let mut seen = vec![false; VOCAB_SIZE];
    for line in text.lines() {
        let (token, id) = line
            .trim_end()
            .rsplit_once(' ')
            .ok_or_else(|| "FireRed token vocabulary is malformed".to_string())?;
        let id = id
            .trim()
            .parse::<usize>()
            .map_err(|_| "FireRed token vocabulary id is not an integer".to_string())?;
        if id >= VOCAB_SIZE || seen[id] {
            return Err("FireRed token vocabulary id is duplicated or out of range".to_string());
        }
        vocabulary[id] = token.to_string();
        seen[id] = true;
    }
    if seen.iter().any(|present| !present) {
        return Err(format!(
            "FireRed token vocabulary must define every id from 0 through {}",
            VOCAB_SIZE - 1
        ));
    }
    Ok(vocabulary)
}

fn decode_window(tokens: &[u32], vocabulary: &[String]) -> Result<(Vec<u32>, String), String> {
    if tokens.first() != Some(&SOS) || tokens.last() != Some(&EOS) {
        return Err("FireRed decoder window did not reach EOS within its token budget".to_string());
    }
    let lexical = tokens[1..tokens.len() - 1]
        .iter()
        .copied()
        .filter(|token| {
            vocabulary
                .get(*token as usize)
                .is_some_and(|value| is_lexical_token(value))
        })
        .collect::<Vec<_>>();
    let text = decode_lexical_tokens(&lexical, vocabulary);
    Ok((lexical, text))
}

fn is_lexical_token(value: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && !(value.starts_with('<') && value.ends_with('>'))
}

fn decode_lexical_tokens(tokens: &[u32], vocabulary: &[String]) -> String {
    tokens
        .iter()
        .filter_map(|token| vocabulary.get(*token as usize))
        .map(|token| token.replace('\u{2581}', " "))
        .collect::<String>()
        .trim()
        .to_string()
}

fn sample_windows(
    samples: usize,
    window_samples: usize,
    overlap_samples: usize,
) -> Result<Vec<(usize, usize)>, String> {
    if samples == 0 || window_samples == 0 || overlap_samples >= window_samples {
        return Err("FireRed window geometry is invalid".to_string());
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
            .ok_or_else(|| "FireRed window position overflow".to_string())?;
    }
    Ok(windows)
}

fn merge_token_ids(assembled: &mut Vec<u32>, next: &[u32]) {
    let overlap = (1..=assembled.len().min(next.len()))
        .rev()
        .find(|&count| assembled[assembled.len() - count..] == next[..count])
        .unwrap_or(0);
    assembled.extend_from_slice(&next[overlap..]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_input_windows_are_bounded_and_overlap() {
        assert_eq!(
            sample_windows(100, 40, 10).unwrap(),
            [(0, 40), (30, 70), (60, 100)]
        );
        assert!(sample_windows(10, 10, 10).is_err());
    }

    #[test]
    fn transcript_tokens_reconcile_the_longest_exact_overlap() {
        let mut assembled = vec![1, 2, 3, 4];
        merge_token_ids(&mut assembled, &[3, 4, 5, 6]);
        assert_eq!(assembled, [1, 2, 3, 4, 5, 6]);
        merge_token_ids(&mut assembled, &[7]);
        assert_eq!(assembled, [1, 2, 3, 4, 5, 6, 7]);
    }

    #[test]
    #[ignore = "requires an explicit packaged runtime, device, rewritten FireRed GGUF, WAV, CMVN, and token vocabulary"]
    fn actual_firered_transcribes_a_reference_wav_end_to_end() {
        use crate::{DeviceKind, GgmlRuntime};
        use std::path::PathBuf;

        fn path(name: &str) -> PathBuf {
            std::env::var_os(name)
                .map(PathBuf::from)
                .unwrap_or_else(|| panic!("set {name}"))
        }

        let runtime = GgmlRuntime::load(&path("UTA_TEST_GGML_RUNTIME_DIR")).unwrap();
        let expected_kind = match std::env::var("UTA_TEST_GGML_DEVICE_KIND")
            .expect("set UTA_TEST_GGML_DEVICE_KIND")
            .as_str()
        {
            "cpu" => DeviceKind::Cpu,
            "integrated_gpu" => DeviceKind::IntegratedGpu,
            "discrete_gpu" => DeviceKind::DiscreteGpu,
            other => panic!("unsupported test device kind: {other}"),
        };
        let description = std::env::var("UTA_TEST_GGML_DEVICE_DESCRIPTION").unwrap_or_default();
        let device = runtime
            .devices()
            .unwrap()
            .into_iter()
            .find(|device| {
                device.kind == expected_kind && device.description.contains(&description)
            })
            .expect("requested FireRed test device is unavailable");
        let model = FireRed::load(runtime, &device, &path("UTA_TEST_FIRERED_GGUF")).unwrap();
        let cmvn = std::fs::read(path("UTA_TEST_FIRERED_CMVN")).unwrap();
        let vocabulary_bytes = std::fs::read(path("UTA_TEST_FIRERED_TOKENS")).unwrap();

        // Decode the production input path directly first, so a whole-chain
        // failure reports the model's own tokens instead of only the assembled
        // text being empty.
        let samples =
            crate::wav::read_f32_wav(&path("UTA_TEST_FIRERED_WAV"), SAMPLE_RATE as u32, 1).unwrap();
        let mut padded = vec![0.0_f32; samples.len().max(MIN_WINDOW_SAMPLES)];
        padded[..samples.len()].copy_from_slice(&samples);
        let (features, frames) = extract_features(&padded, &cmvn).unwrap();
        assert_eq!(frames, FEATURE_FRAMES);
        let encoded = model.encode(&features).unwrap();
        let generated = model.greedy_decode(&encoded).unwrap();
        let vocabulary = load_vocabulary(&vocabulary_bytes).unwrap();
        eprintln!(
            "FireRed whole-chain tokens: {generated:?} -> {:?}",
            generated
                .iter()
                .map(|token| vocabulary[*token as usize].clone())
                .collect::<Vec<_>>()
        );

        let transcription = model
            .transcribe_wav(
                &path("UTA_TEST_FIRERED_WAV"),
                &cmvn,
                &vocabulary_bytes,
                |_, _| {},
            )
            .unwrap();
        eprintln!(
            "FireRed transcript: {:?} tokens {:?}",
            transcription.text, transcription.token_ids
        );
        assert!(!transcription.text.is_empty());
    }

    #[test]
    fn decoder_requires_eos_and_preserves_lexical_ids() {
        let mut vocabulary = vec![String::new(); VOCAB_SIZE];
        vocabulary[SOS as usize] = "<sos>".to_string();
        vocabulary[EOS as usize] = "<eos>".to_string();
        vocabulary[42] = "\u{2581}sing".to_string();
        vocabulary[43] = "ing".to_string();
        assert_eq!(
            decode_window(&[SOS, 42, 43, EOS], &vocabulary).unwrap(),
            (vec![42, 43], "singing".to_string())
        );
        assert!(decode_window(&[SOS, 42], &vocabulary).is_err());
    }
}
