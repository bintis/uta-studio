use super::{Request, audio, progress, required};
use serde_json::{Value, json};
use std::path::Path;
use uta_libtorch_runtime::{Model, firered, qwen};

pub fn asr(model: Model, wav: &Path, request: &Request) -> Result<Value, String> {
    let route = qwen::Qwen::from_model(model)?;
    let result = route.transcribe_wav(
        wav,
        qwen::asr::DEFAULT_MAX_NEW_TOKENS,
        request.language.as_deref(),
        &mut progress,
    )?;
    Ok(
        json!({"text":result.text,"language":result.language_name,"raw_text":result.raw_text,
        "tokens":result.generated_tokens,"finished":result.finished,"unfinished_windows":result.unfinished_windows,
        "prompt_tokens":result.prompt_tokens,"encoder_seconds":result.encoder_seconds,"decoder_seconds":result.decoder_seconds,
        "segments":result.segments.iter().map(|segment| json!({"start_sample":segment.start_sample,"end_sample":segment.end_sample,
            "text_start":segment.text_start,"text_end":segment.text_end})).collect::<Vec<_>>()}),
    )
}
pub fn fire(model: Model, wav: &Path, request: &Request) -> Result<Value, String> {
    let cmvn = std::fs::read(required(&request.cmvn, "FireRed CMVN")?)
        .map_err(|error| error.to_string())?;
    let vocabulary = std::fs::read(required(&request.vocabulary, "FireRed vocabulary")?)
        .map_err(|error| error.to_string())?;
    let result =
        firered::FireRed::from_model(model).transcribe_wav(wav, &cmvn, &vocabulary, progress)?;
    Ok(
        json!({"text":result.text,"tokens":result.token_ids,"unfinished_windows":result.unfinished_windows,
        "windows":result.windows.iter().map(|window| json!({"index":window.index,"start_sample":window.start_sample,
            "end_sample":window.end_sample,"text":window.text,"tokens":window.token_ids})).collect::<Vec<_>>()}),
    )
}

fn word_units(text: &str) -> Vec<(String, usize, usize)> {
    let mut result = Vec::new();
    let mut offset = 0;
    let mut ascii = String::new();
    let mut ascii_start = 0;
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            if ascii.is_empty() {
                ascii_start = offset;
            }
            ascii.push(character);
        } else {
            if !ascii.is_empty() {
                result.push((std::mem::take(&mut ascii), ascii_start, offset));
            }
            if character.is_alphanumeric() {
                result.push((character.to_string(), offset, offset + 1));
            }
        }
        if !character.is_whitespace() {
            offset += 1;
        }
    }
    if !ascii.is_empty() {
        result.push((ascii, ascii_start, offset));
    }
    result
}
pub fn align(model: Model, wav: &Path, request: &Request) -> Result<Value, String> {
    let source = audio::read_json(required(&request.transcript, "real Qwen ASR evidence")?)?;
    let text = source["text"].as_str().ok_or("ASR evidence has no text")?;
    let segments = source["segments"]
        .as_array()
        .ok_or("ASR evidence has no audio/text anchors")?;
    let units = word_units(text);
    let mut words = Vec::new();
    let mut scopes = Vec::new();
    for (text, start, end) in units {
        let matching = segments
            .iter()
            .filter(|segment| {
                segment["text_start"]
                    .as_u64()
                    .is_some_and(|first| (first as usize) < end)
                    && segment["text_end"]
                        .as_u64()
                        .is_some_and(|last| last as usize > start)
            })
            .collect::<Vec<_>>();
        let scope = if matching.is_empty() {
            None
        } else {
            let starts = matching
                .iter()
                .map(|segment| {
                    segment["start_sample"]
                        .as_u64()
                        .ok_or("invalid ASR sample anchor")
                })
                .collect::<Result<Vec<_>, _>>()?;
            let ends = matching
                .iter()
                .map(|segment| {
                    segment["end_sample"]
                        .as_u64()
                        .ok_or("invalid ASR sample anchor")
                })
                .collect::<Result<Vec<_>, _>>()?;
            Some(qwen::aligner::AudioScope {
                start_sample: *starts.iter().min().unwrap() as usize,
                end_sample: *ends.iter().max().unwrap() as usize,
            })
        };
        words.push(text);
        scopes.push(scope);
    }
    let route = qwen::Qwen::from_model(model)?;
    let result = route.align_wav(wav, &words, &scopes, &mut progress)?;
    audio::finite(
        result
            .words
            .iter()
            .flat_map(|word| [word.start_seconds, word.end_seconds]),
    )?;
    Ok(
        json!({"transcript":text,"source_transcript":request.transcript,"word_units":"lexical Han characters and ASCII runs with actual ASR audio scopes",
        "words":result.words.iter().enumerate().map(|(index,word)| json!({"id":format!("word-{index}"),"text":word.text,
            "start_seconds":word.start_seconds,"end_seconds":word.end_seconds,"timing_issue":word.timing_issue})).collect::<Vec<_>>(),
        "raw_classes":result.raw_classes,"raw_timestamp_ms":result.raw_timestamp_ms,"corrected_timestamp_ms":result.corrected_timestamp_ms,
        "prompt_tokens":result.prompt_tokens,"encoder_seconds":result.encoder_seconds,"decoder_seconds":result.decoder_seconds,
        "windows":result.windows.iter().map(|window| json!({"start_micros":window.start_micros,"end_micros":window.end_micros,
            "first_word":window.first_word,"word_count":window.word_count,"anchored":window.anchored,"raw_timestamp_ms":window.raw_timestamp_ms,
            "corrected_timestamp_ms":window.corrected_timestamp_ms,"timing_issues":window.timing_issues})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chinese_units_preserve_asr_unicode_offsets_without_inventing_timestamps() {
        assert_eq!(
            word_units("风吹，唱 hello!"),
            [
                ("风".into(), 0, 1),
                ("吹".into(), 1, 2),
                ("唱".into(), 3, 4),
                ("hello".into(), 4, 9)
            ]
        );
        assert_eq!(
            word_units("hello world"),
            [("hello".into(), 0, 5), ("world".into(), 5, 10)]
        );
    }
}
