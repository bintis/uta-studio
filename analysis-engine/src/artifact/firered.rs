use std::path::Path;

use serde::Deserialize;

use super::{TranscriptArtifactV1, TranscriptAuthorityV1};
use crate::contract::{EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 4 * 1024 * 1024;
const REVISION: &str = "FireRedTeam/FireRedASR2-AED@2304afed56eacfee6256dee5937ed22ffa0b64ec";
const SAMPLE_RATE: usize = 16_000;
const MAX_INPUT_SAMPLES: usize = 4 * 60 * 60 * SAMPLE_RATE;
const WINDOW_SAMPLES: usize = 37_199;
const WINDOW_OVERLAP_SAMPLES: usize = SAMPLE_RATE;
const FEATURE_FRAMES: usize = 230;
const ENCODER_FRAMES: usize = 58;
const MAX_GENERATED_TOKENS: usize = 11;
const VOCAB_SIZE: u32 = 8_667;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvidence {
    schema_version: u32,
    model_id: String,
    selected_source_revision: String,
    model_content_digest: String,
    runtime_content_digest: String,
    backend: String,
    contract_scope: String,
    sample_rate: usize,
    input_samples: usize,
    window_samples: usize,
    window_overlap_samples: usize,
    window_count: usize,
    feature_frames: usize,
    encoder_frames: usize,
    max_generated_tokens: usize,
    text: String,
    token_ids: Vec<u32>,
    windows: Vec<RawWindow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    index: usize,
    start_sample: usize,
    end_sample: usize,
    text: String,
    token_ids: Vec<u32>,
}

pub fn parse_firered_transcript(path: &Path) -> EngineResult<TranscriptArtifactV1> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("FireRed evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("FireRed evidence size is invalid"));
    }
    let raw: RawEvidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read FireRed evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("FireRed evidence JSON is invalid: {error}")))?;
    let expected_ranges = sample_windows(raw.input_samples)?;
    let windows_valid = raw.windows.len() == expected_ranges.len()
        && raw.windows.iter().zip(expected_ranges).enumerate().all(
            |(index, (window, (start, end)))| {
                window.index == index
                    && window.start_sample == start
                    && window.end_sample == end
                    && window.text == window.text.trim()
                    && window.token_ids.len() < MAX_GENERATED_TOKENS
                    && window.token_ids.iter().all(|token| *token < VOCAB_SIZE)
            },
        );
    let mut expected_tokens = Vec::new();
    for window in &raw.windows {
        merge_token_ids(&mut expected_tokens, &window.token_ids);
    }
    if raw.schema_version != 2
        || raw.model_id != "firered_asr2_aed"
        || raw.selected_source_revision != REVISION
        || raw.model_content_digest.trim().is_empty()
        || raw.runtime_content_digest.trim().is_empty()
        || !matches!(raw.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
        || raw.contract_scope != "overlapping_windowed_230_feature_frame_sequence"
        || raw.sample_rate != SAMPLE_RATE
        || raw.input_samples == 0
        || raw.input_samples > MAX_INPUT_SAMPLES
        || raw.window_samples != WINDOW_SAMPLES
        || raw.window_overlap_samples != WINDOW_OVERLAP_SAMPLES
        || raw.window_count != raw.windows.len()
        || !windows_valid
        || raw.feature_frames != FEATURE_FRAMES
        || raw.encoder_frames != ENCODER_FRAMES
        || raw.max_generated_tokens != MAX_GENERATED_TOKENS
        || raw.text.trim().is_empty()
        || raw.text != raw.text.trim()
        || raw.token_ids.is_empty()
        || raw.token_ids != expected_tokens
    {
        return Err(invalid(
            "FireRed evidence identity or bounded contract is invalid",
        ));
    }
    let artifact = TranscriptArtifactV1 {
        contract: "uta.analysis-engine.transcript".to_string(),
        version: 1,
        authority: TranscriptAuthorityV1::Generated,
        language: None,
        text: raw.text,
        tokens: Vec::new(),
        confidence: None,
        source_experts: vec![raw.model_id],
        alternatives: Vec::new(),
        model_sha256: Some(raw.model_content_digest),
        runtime_manifest_sha256: Some(raw.runtime_content_digest),
        backend: raw.backend,
    };
    artifact.validate()?;
    Ok(artifact)
}

fn sample_windows(samples: usize) -> EngineResult<Vec<(usize, usize)>> {
    if samples == 0 || samples > MAX_INPUT_SAMPLES {
        return Err(invalid("FireRed input sample count is invalid"));
    }
    if samples <= WINDOW_SAMPLES {
        return Ok(vec![(0, samples)]);
    }
    let step = WINDOW_SAMPLES - WINDOW_OVERLAP_SAMPLES;
    let mut windows = Vec::new();
    let mut start = 0usize;
    loop {
        let end = start.saturating_add(WINDOW_SAMPLES).min(samples);
        windows.push((start, end));
        if end == samples {
            break;
        }
        start = start
            .checked_add(step)
            .ok_or_else(|| invalid("FireRed window position overflows"))?;
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

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence() -> serde_json::Value {
        serde_json::json!({
            "schema_version": 2,
            "model_id": "firered_asr2_aed",
            "selected_source_revision": REVISION,
            "model_content_digest": "model-generation",
            "runtime_content_digest": "runtime-generation",
            "backend": "ggml_vulkan",
            "contract_scope": "overlapping_windowed_230_feature_frame_sequence",
            "sample_rate": 16000,
            "input_samples": 58398,
            "window_samples": 37199,
            "window_overlap_samples": 16000,
            "window_count": 2,
            "feature_frames": 230,
            "encoder_frames": 58,
            "max_generated_tokens": 11,
            "text": "hello world",
            "token_ids": [42, 43, 44],
            "windows": [
                {"index":0,"start_sample":0,"end_sample":37199,"text":"hello","token_ids":[42,43]},
                {"index":1,"start_sample":21199,"end_sample":58398,"text":"world","token_ids":[43,44]}
            ]
        })
    }

    #[test]
    fn parser_preserves_challenger_identity_without_confidence() {
        let path = std::env::temp_dir().join(format!("uta-firered-{}.json", std::process::id()));
        std::fs::write(&path, serde_json::to_vec(&evidence()).unwrap()).unwrap();
        let transcript = parse_firered_transcript(&path).unwrap();
        assert_eq!(transcript.text, "hello world");
        assert_eq!(transcript.confidence, None);
        assert_eq!(transcript.source_experts, ["firered_asr2_aed"]);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parser_rejects_a_token_sequence_that_disagrees_with_window_stitching() {
        let path =
            std::env::temp_dir().join(format!("uta-firered-invalid-{}.json", std::process::id()));
        let mut value = evidence();
        value["token_ids"] = serde_json::json!([42, 43, 43, 44]);
        std::fs::write(&path, serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(parse_firered_transcript(&path).is_err());
        std::fs::remove_file(path).unwrap();
    }
}
