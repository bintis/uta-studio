use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 16 * 1024 * 1024;
#[cfg(test)]
const QWEN_ASR_MODEL_SHA256: &str =
    "b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e";
const QWEN_ASR_LONG_INPUT_POLICY: &str = "qwen-asr-windowed-90s-v1";
const QWEN_ASR_WINDOW_SECONDS: f64 = 90.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptAuthorityV1 {
    CallerCanonical,
    #[default]
    Generated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptArtifactV1 {
    pub contract: String,
    pub version: u32,
    #[serde(default)]
    pub authority: TranscriptAuthorityV1,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub text: String,
    #[serde(default)]
    pub tokens: Vec<TranscriptTokenV1>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
    pub source_experts: Vec<String>,
    #[serde(default)]
    pub alternatives: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_sha256: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_manifest_sha256: Option<String>,
    pub backend: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranscriptTokenV1 {
    pub id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f32>,
}

impl TranscriptArtifactV1 {
    pub fn validate(&self) -> EngineResult<()> {
        if self.contract != "uta.analysis-engine.transcript"
            || self.version != 1
            || self.text.trim().is_empty()
            || self.source_experts.is_empty()
            || self
                .source_experts
                .iter()
                .any(|source| source.trim().is_empty())
            || self.backend.trim().is_empty()
            || self
                .confidence
                .is_some_and(|value| !valid_confidence(value))
            || self.tokens.iter().any(|token| {
                token.id.trim().is_empty()
                    || token.text.trim().is_empty()
                    || token
                        .confidence
                        .is_some_and(|value| !valid_confidence(value))
            })
        {
            return Err(invalid("transcript artifact is invalid"));
        }
        match self.authority {
            TranscriptAuthorityV1::CallerCanonical => {
                if self.confidence.is_some()
                    || self.model_sha256.is_some()
                    || self.runtime_manifest_sha256.is_some()
                    || self.backend != "caller"
                {
                    return Err(invalid(
                        "caller-canonical transcript must not claim model confidence or provenance",
                    ));
                }
            }
            TranscriptAuthorityV1::Generated => {
                if self.model_sha256.is_none() || self.runtime_manifest_sha256.is_none() {
                    return Err(invalid("generated transcript provenance is incomplete"));
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenTranscriptEvidenceV2 {
    schema_version: u32,
    model_id: String,
    model_sha256: String,
    backend: String,
    runtime_manifest_sha256: String,
    language_contract: QwenLanguageContractV1,
    language: String,
    text: String,
    #[serde(default)]
    long_input: Option<QwenAsrLongInputV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenAsrLongInputV1 {
    policy: String,
    max_window_seconds: f64,
    source_duration_seconds: f64,
    segments: Vec<QwenAsrSegmentV1>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenAsrSegmentV1 {
    index: usize,
    audio_start_seconds: f64,
    audio_end_seconds: f64,
    detected_language: String,
    text_characters: usize,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct QwenLanguageContractV1 {
    version: u32,
    explicit_hint_policy: String,
    evidence_source: String,
}

pub fn parse_qwen_transcript(path: &Path) -> EngineResult<TranscriptArtifactV1> {
    let raw: QwenTranscriptEvidenceV2 = read_bounded_json(path)?;
    if raw.schema_version != 2
        || raw.model_id != "qwen3_asr_1_7b"
        || raw.language.trim().is_empty()
        || raw.text.trim().is_empty()
        || raw.backend != "vulkan"
        || raw.language_contract.version != 1
        || raw.language_contract.explicit_hint_policy != "reject"
        || raw.language_contract.evidence_source != "runtime_detected"
        || raw
            .long_input
            .as_ref()
            .is_some_and(|long_input| !valid_qwen_asr_windowing(long_input))
    {
        return Err(invalid(
            "Qwen transcript evidence identity or text is invalid",
        ));
    }
    let artifact = TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthorityV1::Generated,
        language: Some(raw.language.trim().to_string()),
        text: raw.text.trim().to_string(),
        tokens: Vec::new(),
        // The current worker emits no calibrated token or transcript probability.
        confidence: None,
        source_experts: vec![raw.model_id],
        alternatives: Vec::new(),
        model_sha256: Some(raw.model_sha256),
        runtime_manifest_sha256: Some(raw.runtime_manifest_sha256),
        backend: raw.backend,
    };
    artifact.validate()?;
    Ok(artifact)
}

fn read_bounded_json<T: serde::de::DeserializeOwned>(path: &Path) -> EngineResult<T> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("evidence size is invalid"));
    }
    let bytes = std::fs::read(path)
        .map_err(|error| invalid(format!("could not read evidence: {error}")))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid(format!("evidence JSON is invalid: {error}")))
}

fn valid_qwen_asr_windowing(value: &QwenAsrLongInputV1) -> bool {
    if value.policy != QWEN_ASR_LONG_INPUT_POLICY
        || !value.max_window_seconds.is_finite()
        || (value.max_window_seconds - QWEN_ASR_WINDOW_SECONDS).abs() > f64::EPSILON
        || !value.source_duration_seconds.is_finite()
        || value.source_duration_seconds <= 0.0
        || value.segments.is_empty()
    {
        return false;
    }
    let mut expected_start = 0.0;
    for (index, segment) in value.segments.iter().enumerate() {
        // A window covering no speech (e.g. a purely instrumental passage)
        // legitimately carries no language and no transcribed text; require
        // those two fields to agree on "silent" rather than demanding every
        // window contain real speech.
        let silent = segment.detected_language.trim().is_empty() && segment.text_characters == 0;
        let spoken = !segment.detected_language.trim().is_empty() && segment.text_characters > 0;
        if segment.index != index
            || !segment.audio_start_seconds.is_finite()
            || !segment.audio_end_seconds.is_finite()
            || (segment.audio_start_seconds - expected_start).abs() > 0.001
            || segment.audio_end_seconds <= segment.audio_start_seconds
            || segment.audio_end_seconds - segment.audio_start_seconds
                > QWEN_ASR_WINDOW_SECONDS + 0.001
            || !(silent || spoken)
        {
            return false;
        }
        expected_start = segment.audio_end_seconds;
    }
    (expected_start - value.source_duration_seconds).abs() <= 0.001
}

fn valid_confidence(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_schema_two_worker_shape_without_inventing_tokens() {
        let path =
            std::env::temp_dir().join(format!("uta-qwen-transcript-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 2,
                "model_id": "qwen3_asr_1_7b",
                "model_sha256": QWEN_ASR_MODEL_SHA256,
                "backend": "vulkan",
                "runtime_manifest_sha256": "b".repeat(64),
                "language_contract": {
                    "version": 1,
                    "explicit_hint_policy": "reject",
                    "evidence_source": "runtime_detected"
                },
                "language": "en",
                "text": "sing now",
                "long_input": {
                    "policy": QWEN_ASR_LONG_INPUT_POLICY,
                    "max_window_seconds": QWEN_ASR_WINDOW_SECONDS,
                    "source_duration_seconds": 100.0,
                    "segments": [
                        {"index":0,"audio_start_seconds":0.0,"audio_end_seconds":90.0,
                         "detected_language":"en","text_characters":4},
                        {"index":1,"audio_start_seconds":90.0,"audio_end_seconds":100.0,
                         "detected_language":"en","text_characters":3}
                    ]
                }
            }))
            .unwrap(),
        )
        .unwrap();
        let transcript = parse_qwen_transcript(&path).unwrap();
        assert_eq!(transcript.text, "sing now");
        assert_eq!(transcript.language.as_deref(), Some("en"));
        assert!(transcript.tokens.is_empty());
        assert_eq!(transcript.confidence, None);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn malformed_qwen_window_coverage_fails_closed() {
        let value = QwenAsrLongInputV1 {
            policy: QWEN_ASR_LONG_INPUT_POLICY.to_string(),
            max_window_seconds: QWEN_ASR_WINDOW_SECONDS,
            source_duration_seconds: 100.0,
            segments: vec![QwenAsrSegmentV1 {
                index: 0,
                audio_start_seconds: 1.0,
                audio_end_seconds: 90.0,
                detected_language: "en".to_string(),
                text_characters: 3,
            }],
        };
        assert!(!valid_qwen_asr_windowing(&value));
    }

    #[test]
    fn silent_instrumental_window_is_valid_qwen_coverage() {
        // A trailing instrumental outro legitimately produces a window with
        // no detected language and no transcribed text (confirmed against a
        // real song); coverage must still accept it as long as it stays
        // contiguous with its neighbors.
        let value = QwenAsrLongInputV1 {
            policy: QWEN_ASR_LONG_INPUT_POLICY.to_string(),
            max_window_seconds: QWEN_ASR_WINDOW_SECONDS,
            source_duration_seconds: 100.0,
            segments: vec![
                QwenAsrSegmentV1 {
                    index: 0,
                    audio_start_seconds: 0.0,
                    audio_end_seconds: 90.0,
                    detected_language: "en".to_string(),
                    text_characters: 4,
                },
                QwenAsrSegmentV1 {
                    index: 1,
                    audio_start_seconds: 90.0,
                    audio_end_seconds: 100.0,
                    detected_language: String::new(),
                    text_characters: 0,
                },
            ],
        };
        assert!(valid_qwen_asr_windowing(&value));
    }

    #[test]
    fn qwen_segment_language_and_text_must_agree_on_silence() {
        let inconsistent = QwenAsrLongInputV1 {
            policy: QWEN_ASR_LONG_INPUT_POLICY.to_string(),
            max_window_seconds: QWEN_ASR_WINDOW_SECONDS,
            source_duration_seconds: 90.0,
            segments: vec![QwenAsrSegmentV1 {
                index: 0,
                audio_start_seconds: 0.0,
                audio_end_seconds: 90.0,
                detected_language: String::new(),
                text_characters: 3,
            }],
        };
        assert!(!valid_qwen_asr_windowing(&inconsistent));
    }
}
