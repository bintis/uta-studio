use std::collections::BTreeMap;
use std::path::Path;

use serde::Deserialize;

use super::{TranscriptArtifactV1, TranscriptAuthorityV1};
use crate::contract::{EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 4 * 1024 * 1024;
#[cfg(test)]
const MANIFEST_SHA256: &str = "093335b6a113e5eead88bb011a7870d61f18319e8d0204523c3ce9d82e6c8c35";
const REVISION: &str = "42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258";
/// The official FireRedTeam checkpoint used by the native (`ggml_native`)
/// route -- see `native-inference/firered/src/engine.rs::NATIVE_SOURCE_REVISION`.
const NATIVE_REVISION: &str =
    "FireRedTeam/FireRedASR2-AED@2304afed56eacfee6256dee5937ed22ffa0b64ec";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvidence {
    schema_version: u32,
    model_id: String,
    selected_source_revision: String,
    #[serde(rename = "source_graph_sha256")]
    _source_graph_sha256: BTreeMap<String, String>,
    model_manifest_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    contract_scope: String,
    input_samples: usize,
    window_samples: usize,
    window_count: usize,
    feature_frames: usize,
    encoder_frames: usize,
    decoder_cache_max: usize,
    text: String,
    token_ids: Vec<i64>,
    ctc_frames: usize,
    windows: Vec<RawWindow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawWindow {
    index: usize,
    start_sample: usize,
    end_sample: usize,
    text: String,
    token_ids: Vec<i64>,
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
    let expected_window_count = raw.input_samples.div_ceil(37_199);
    let windows_valid = raw.windows.len() == expected_window_count
        && raw.windows.iter().enumerate().all(|(index, window)| {
            window.index == index
                && window.start_sample == index * 37_199
                && window.end_sample == ((index + 1) * 37_199).min(raw.input_samples)
                && window.end_sample > window.start_sample
                && window.text == window.text.trim()
                && window.token_ids.len() <= 11
                && window.token_ids.iter().all(|token| *token >= 0)
        });
    let expected_tokens = raw
        .windows
        .iter()
        .flat_map(|window| window.token_ids.iter().copied())
        .collect::<Vec<_>>();
    let expected_text = raw
        .windows
        .iter()
        .filter(|window| !window.text.is_empty())
        .map(|window| window.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    if raw.schema_version != 3
        || raw.model_id != "firered_asr2_aed"
        || !matches!(
            raw.selected_source_revision.as_str(),
            REVISION | NATIVE_REVISION
        )
        || !matches!(
            raw.backend.as_str(),
            "openvino_gpu" | "openvino_cpu" | "ggml_native"
        )
        || raw.contract_scope != "windowed_230_feature_frame_sequence"
        || raw.input_samples == 0
        || raw.window_samples != 37_199
        || raw.window_count != expected_window_count
        || !windows_valid
        || raw.feature_frames != 230
        || raw.encoder_frames != 58
        || raw.ctc_frames != 58
        || raw.decoder_cache_max != 10
        || raw.text.trim().is_empty()
        || raw.text != expected_text
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
        text: raw.text.trim().to_string(),
        tokens: Vec::new(),
        confidence: None,
        source_experts: vec![raw.model_id],
        alternatives: Vec::new(),
        model_sha256: Some(raw.model_manifest_sha256),
        runtime_manifest_sha256: Some(raw.runtime_manifest_sha256),
        backend: raw.backend,
    };
    artifact.validate()?;
    Ok(artifact)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_preserves_challenger_identity_without_confidence() {
        let path = std::env::temp_dir().join(format!("uta-firered-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 3,
                "model_id": "firered_asr2_aed",
                "selected_source_revision": REVISION,
                "source_graph_sha256": {
                    "encoder": "0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9",
                    "decoder": "aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833",
                    "ctc": "8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4"
                },
                "model_manifest_sha256": MANIFEST_SHA256,
                "runtime_manifest_sha256": "a".repeat(64),
                "backend": "openvino_gpu",
                "contract_scope": "windowed_230_feature_frame_sequence",
                "input_samples": 74398,
                "window_samples": 37199,
                "window_count": 2,
                "feature_frames": 230,
                "encoder_frames": 58,
                "decoder_cache_max": 10,
                "text": "hello world",
                "token_ids": [42, 43],
                "ctc_frames": 58,
                "windows": [
                    {"index":0,"start_sample":0,"end_sample":37199,"text":"hello","token_ids":[42]},
                    {"index":1,"start_sample":37199,"end_sample":74398,"text":"world","token_ids":[43]}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let transcript = parse_firered_transcript(&path).unwrap();
        assert_eq!(transcript.text, "hello world");
        assert_eq!(transcript.confidence, None);
        assert_eq!(transcript.source_experts, ["firered_asr2_aed"]);
        std::fs::remove_file(path).unwrap();
    }
}
