use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{
    BoundaryAuthority, BoundaryLevel, CANONICAL_TIMEBASE, EngineError, EngineErrorCode,
    EngineResult,
};

const MAX_EVIDENCE_BYTES: u64 = 32 * 1024 * 1024;
const QWEN_COARSE_FALLBACK_PROFILE: &str = "qwen-align-coarse-generated-transcript-v1";
#[cfg(test)]
const QWEN_ALIGN_MODEL_SHA256: &str =
    "c70553d4e363b752db9110bba0a1ef5fb87355cd80e14703c457fbe7f39a936b";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentArtifactV1 {
    pub contract: String,
    pub version: u32,
    pub transcript: String,
    pub language: Option<String>,
    pub items: Vec<AlignmentItemV1>,
    pub source_expert: String,
    pub model_sha256: String,
    pub runtime_manifest_sha256: String,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AlignmentItemV1 {
    pub id: String,
    pub text: String,
    pub level: BoundaryLevel,
    pub start: u64,
    pub duration: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub authority: BoundaryAuthority,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenAlignmentEvidenceV2 {
    schema_version: u32,
    model_id: String,
    model_sha256: String,
    backend: String,
    runtime_manifest_sha256: String,
    text_normalization_profile: String,
    language_normalization_profile: String,
    alignment_semantics_profile: String,
    transcript: String,
    language: Option<String>,
    runtime_language: Option<String>,
    #[serde(default)]
    fallback: Option<QwenAlignmentFallbackV1>,
    #[serde(default)]
    long_input: Option<QwenLongInputEvidenceV1>,
    words: Vec<QwenWord>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenAlignmentFallbackV1 {
    profile: String,
    trigger: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenLongInputEvidenceV1 {
    policy: String,
    max_window_seconds: f64,
    source_duration_seconds: f64,
    text_unit_count: usize,
    segments: Vec<QwenAlignmentSegmentV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenAlignmentSegmentV1 {
    index: usize,
    audio_start_seconds: f64,
    audio_end_seconds: f64,
    context_unit_start: usize,
    target_unit_start: usize,
    target_unit_end: usize,
    measured_units: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenWord {
    word: String,
    start: f64,
    end: f64,
}

fn read_qwen_alignment_evidence(path: &Path) -> EngineResult<QwenAlignmentEvidenceV2> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("alignment evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("alignment evidence size is invalid"));
    }
    serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read alignment evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("alignment evidence JSON is invalid: {error}")))
}

pub fn qwen_alignment_uses_coarse_fallback(path: &Path) -> EngineResult<bool> {
    Ok(read_qwen_alignment_evidence(path)?
        .fallback
        .is_some_and(|fallback| fallback.profile == QWEN_COARSE_FALLBACK_PROFILE))
}

pub fn parse_qwen_alignment(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<AlignmentArtifactV1> {
    let raw = read_qwen_alignment_evidence(path)?;
    if raw.schema_version != 2
        || raw.model_id != "qwen3_forced_aligner_0_6b"
        || raw.transcript.trim().is_empty()
        || raw.backend != "vulkan"
        || raw.text_normalization_profile != "qwen-align-text-preserve-v1"
        || raw.language_normalization_profile != "qwen-align-language-v1"
        || raw.alignment_semantics_profile != "qwen-align-token-word-80ms-v1"
        || !valid_language_pair(raw.language.as_deref(), raw.runtime_language.as_deref())
        || raw.fallback.as_ref().is_some_and(|fallback| {
            fallback.profile != QWEN_COARSE_FALLBACK_PROFILE
                || fallback.trigger != "measurement_retries_exhausted"
        })
        || raw.words.is_empty()
        || compact_text(
            &raw.words
                .iter()
                .map(|word| word.word.as_str())
                .collect::<String>(),
        ) != compact_text(&raw.transcript)
        || raw.long_input.as_ref().is_some_and(|evidence| {
            !valid_long_input_evidence(evidence, source_duration, &raw.transcript)
        })
    {
        return Err(invalid("Qwen alignment evidence identity is invalid"));
    }
    let mut items = Vec::with_capacity(raw.words.len());
    let mut previous_end = 0_u64;
    for (index, word) in raw.words.into_iter().enumerate() {
        let local_start = seconds_to_canonical(word.start)?;
        let local_end = seconds_to_canonical(word.end)?;
        if word.word.trim().is_empty()
            || local_end <= local_start
            || local_start % 80_000 != 0
            || local_end % 80_000 != 0
            || local_start < previous_end
            || local_end > source_duration
        {
            return Err(invalid(
                "Qwen alignment words are empty, overlapping, or unordered",
            ));
        }
        previous_end = local_end;
        let start = source_start
            .checked_add(local_start)
            .ok_or_else(|| invalid("alignment start overflows the source timeline"))?;
        let duration = local_end - local_start;
        start
            .checked_add(duration)
            .ok_or_else(|| invalid("alignment end overflows the source timeline"))?;
        items.push(AlignmentItemV1 {
            id: format!("word-{index}"),
            text: word.word,
            level: BoundaryLevel::Word,
            start,
            duration,
            // Current Qwen worker has no calibrated per-word probability.
            confidence: None,
            authority: BoundaryAuthority::Soft,
        });
    }
    Ok(AlignmentArtifactV1 {
        contract: "uta.analysis-engine.alignment".to_string(),
        version: 1,
        transcript: raw.transcript,
        language: raw.language,
        items,
        source_expert: raw.model_id,
        model_sha256: raw.model_sha256,
        runtime_manifest_sha256: raw.runtime_manifest_sha256,
        backend: raw.backend,
    })
}

fn valid_long_input_evidence(
    evidence: &QwenLongInputEvidenceV1,
    source_duration: u64,
    transcript: &str,
) -> bool {
    let source_seconds = source_duration as f64 / f64::from(CANONICAL_TIMEBASE);
    if evidence.policy != "qwen-align-windowed-v1"
        || evidence.max_window_seconds != 140.0
        || !evidence.source_duration_seconds.is_finite()
        || (evidence.source_duration_seconds - source_seconds).abs() > 0.001
        || evidence.text_unit_count != alignment_text_unit_count(transcript)
        || evidence.text_unit_count == 0
        || evidence.segments.is_empty()
    {
        return false;
    }
    let mut previous_target_end = 0_usize;
    for (index, segment) in evidence.segments.iter().enumerate() {
        if segment.index != index
            || !segment.audio_start_seconds.is_finite()
            || !segment.audio_end_seconds.is_finite()
            || segment.audio_start_seconds < 0.0
            || segment.audio_end_seconds <= segment.audio_start_seconds
            || segment.audio_end_seconds - segment.audio_start_seconds
                > evidence.max_window_seconds + 0.001
            || segment.audio_end_seconds > evidence.source_duration_seconds + 0.001
            || segment.context_unit_start > segment.target_unit_start
            || segment.target_unit_start != previous_target_end
            || segment.target_unit_end <= segment.target_unit_start
            || segment.target_unit_end > evidence.text_unit_count
            || segment.measured_units == 0
        {
            return false;
        }
        previous_target_end = segment.target_unit_end;
    }
    previous_target_end == evidence.text_unit_count
}

fn valid_language_pair(language: Option<&str>, runtime_language: Option<&str>) -> bool {
    matches!(
        (language, runtime_language),
        (None, None)
            | (Some("zh" | "yue"), Some("chinese"))
            | (Some("en"), Some("english"))
            | (Some("fr"), Some("french"))
            | (Some("de"), Some("german"))
            | (Some("it"), Some("italian"))
            | (Some("ja"), Some("japanese"))
            | (Some("ko"), Some("korean"))
            | (Some("pt"), Some("portuguese"))
            | (Some("ru"), Some("russian"))
            | (Some("es"), Some("spanish"))
    )
}

fn is_dense_script_character(character: char) -> bool {
    matches!(character,
        '\u{3000}'..='\u{303F}'
        | '\u{3040}'..='\u{30FF}'
        | '\u{3400}'..='\u{4DBF}'
        | '\u{4E00}'..='\u{9FFF}'
        | '\u{FF00}'..='\u{FFEF}'
    )
}

fn alignment_text_unit_count(transcript: &str) -> usize {
    let mut count = 0_usize;
    let mut in_word = false;
    for character in transcript.chars() {
        if character.is_whitespace() {
            if in_word {
                count += 1;
                in_word = false;
            }
        } else if is_dense_script_character(character) {
            if in_word {
                count += 1;
                in_word = false;
            }
            count += 1;
        } else {
            in_word = true;
        }
    }
    count + usize::from(in_word)
}

fn compact_text(text: &str) -> String {
    text.chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn seconds_to_canonical(seconds: f64) -> EngineResult<u64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid("alignment time is not finite and non-negative"));
    }
    let units = seconds * f64::from(CANONICAL_TIMEBASE);
    if units > u64::MAX as f64 {
        return Err(invalid("alignment time overflows canonical units"));
    }
    Ok(units.round() as u64)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_worker_seconds_to_ordered_integer_timeline() {
        let path = std::env::temp_dir().join(format!("uta-qwen-align-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 2,
                "model_id": "qwen3_forced_aligner_0_6b",
                "model_sha256": QWEN_ALIGN_MODEL_SHA256,
                "backend": "vulkan",
                "runtime_manifest_sha256": "b".repeat(64),
                "text_normalization_profile": "qwen-align-text-preserve-v1",
                "language_normalization_profile": "qwen-align-language-v1",
                "alignment_semantics_profile": "qwen-align-token-word-80ms-v1",
                "transcript": "sing now",
                "language": "en",
                "runtime_language": "english",
                "long_input": {
                    "policy": "qwen-align-windowed-v1",
                    "max_window_seconds": 140.0,
                    "source_duration_seconds": 1.0,
                    "text_unit_count": 2,
                    "segments": [{
                        "index": 0,
                        "audio_start_seconds": 0.0,
                        "audio_end_seconds": 1.0,
                        "context_unit_start": 0,
                        "target_unit_start": 0,
                        "target_unit_end": 2,
                        "measured_units": 2
                    }]
                },
                "words": [
                    {"word":"sing","start":0.08,"end":0.4},
                    {"word":"now","start":0.48,"end":0.88}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let alignment = parse_qwen_alignment(&path, 2_000_000, 1_000_000).unwrap();
        assert_eq!(alignment.items[0].start, 2_080_000);
        assert_eq!(alignment.items[1].duration, 400_000);
        assert_eq!(alignment.items[0].authority, BoundaryAuthority::Soft);
        assert_eq!(
            parse_qwen_alignment(&path, 0, 800_000).unwrap_err().code,
            EngineErrorCode::OutputValidationFailed
        );
        std::fs::remove_file(path).unwrap();
    }
}
