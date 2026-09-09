use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: usize = 4 * 60 * 60 * 100;
#[cfg(test)]
const RMVPE_SOURCE_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PitchEvidenceV03 {
    pub format: String,
    pub format_version: String,
    pub timebase: u64,
    pub start: u64,
    pub hop: u64,
    pub frequency_hz: Vec<Option<f64>>,
    /// Calibrated confidence when the expert supplies one. `None` preserves
    /// truthful uncalibrated evidence such as FCPE instead of inventing a score.
    pub confidence: Vec<Option<f64>>,
    pub model: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RmvpeEvidence {
    schema_version: u32,
    model_id: String,
    source_model_sha256: String,
    model_gguf_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    timeline_step_ms: u32,
    sample_rate: u32,
    frames: Vec<RmvpeFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RmvpeFrame {
    time: f64,
    hz: f32,
    confidence: f32,
    voiced: bool,
}

pub fn parse_rmvpe_pitch(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<PitchEvidenceV03> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("RMVPE evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("RMVPE evidence size is invalid"));
    }
    let raw: RmvpeEvidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read RMVPE evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("RMVPE evidence JSON is invalid: {error}")))?;
    if raw.schema_version != 1
        || raw.model_id != "rmvpe"
        || !matches!(raw.backend.as_str(), "ggml_vulkan" | "ggml_cpu")
        || raw.timeline_step_ms == 0
        || raw.sample_rate == 0
        || raw.frames.is_empty()
        || raw.frames.len() > MAX_FRAMES
    {
        return Err(invalid("RMVPE evidence identity or shape is invalid"));
    }
    let hop = u64::from(raw.timeline_step_ms)
        .checked_mul(1_000)
        .ok_or_else(|| invalid("RMVPE timeline hop overflows"))?;
    let frame_count = raw.frames.len();
    let mut frequency_hz = Vec::with_capacity(frame_count);
    let mut confidence = Vec::with_capacity(frame_count);
    for (index, frame) in raw.frames.into_iter().enumerate() {
        let expected = index as u64 * hop;
        let actual = seconds_to_canonical(frame.time)?;
        if actual.abs_diff(expected) > 1
            || !frame.hz.is_finite()
            || frame.hz <= 0.0
            || !frame.confidence.is_finite()
            || !(0.0..=1.0).contains(&frame.confidence)
            || frame.voiced != (frame.confidence >= 0.03)
        {
            return Err(invalid(
                "RMVPE frames are invalid or not on the declared grid",
            ));
        }
        frequency_hz.push(frame.voiced.then_some(f64::from(frame.hz)));
        confidence.push(Some(f64::from(frame.confidence)));
    }
    let mut model = BTreeMap::new();
    model.insert("id".to_string(), serde_json::json!(raw.model_id));
    model.insert(
        "source_sha256".to_string(),
        serde_json::json!(raw.source_model_sha256),
    );
    model.insert(
        "weights_sha256".to_string(),
        serde_json::json!(raw.model_gguf_sha256),
    );
    model.insert("artifact_format".to_string(), serde_json::json!("gguf_f32"));
    model.insert(
        "runtime_manifest_sha256".to_string(),
        serde_json::json!(raw.runtime_manifest_sha256),
    );
    model.insert("backend".to_string(), serde_json::json!(raw.backend));
    model.insert(
        "sample_rate".to_string(),
        serde_json::json!(raw.sample_rate),
    );
    let local_end = (frame_count - 1)
        .try_into()
        .ok()
        .and_then(|count: u64| count.checked_mul(hop))
        .ok_or_else(|| invalid("pitch evidence duration overflows"))?;
    if local_end > source_duration {
        return Err(invalid(
            "pitch evidence exceeds the decoded source duration",
        ));
    }
    source_start
        .checked_add(local_end)
        .ok_or_else(|| invalid("pitch evidence overflows the source timeline"))?;
    Ok(PitchEvidenceV03 {
        format: "uta.pitch-evidence".to_string(),
        format_version: "0.3.0".to_string(),
        timebase: u64::from(CANONICAL_TIMEBASE),
        start: source_start,
        hop,
        frequency_hz,
        confidence,
        model,
    })
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FcpeEvidence {
    schema_version: u32,
    model_id: String,
    model_gguf_size_bytes: u64,
    runtime_manifest_sha256: String,
    backend: String,
    timeline_step_ms: u32,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    frames: Vec<FcpeFrame>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FcpeFrame {
    time: f64,
    hz: Option<f32>,
}

pub fn parse_fcpe_pitch(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<PitchEvidenceV03> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("FCPE evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("FCPE evidence size is invalid"));
    }
    let raw: FcpeEvidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read FCPE evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("FCPE evidence JSON is invalid: {error}")))?;
    if raw.schema_version != 1
        || raw.model_id != "fcpe"
        || raw.model_gguf_size_bytes != 43_309_760
        || !matches!(raw.backend.as_str(), "ggml_vulkan" | "ggml_cpu")
        || raw.timeline_step_ms != 10
        || raw.sample_rate != 16_000
        || raw.window_samples != 32_000
        || raw.window_hop_samples != 32_000
        || raw.frames.is_empty()
        || raw.frames.len() > MAX_FRAMES
    {
        return Err(invalid("FCPE evidence identity or shape is invalid"));
    }
    let hop = u64::from(raw.timeline_step_ms) * 1_000;
    let frame_count = raw.frames.len();
    let mut frequency_hz = Vec::with_capacity(frame_count);
    let mut confidence = Vec::with_capacity(frame_count);
    for (index, frame) in raw.frames.into_iter().enumerate() {
        let expected = index as u64 * hop;
        let actual = seconds_to_canonical(frame.time)?;
        let valid_semantics = frame.hz.is_none_or(|hz| hz.is_finite() && hz > 0.0);
        if actual.abs_diff(expected) > 1 || !valid_semantics {
            return Err(invalid(
                "FCPE frames are invalid or not on the declared grid",
            ));
        }
        frequency_hz.push(frame.hz.map(f64::from));
        confidence.push(None);
    }
    let local_end = (frame_count - 1) as u64 * hop;
    if local_end > source_duration {
        return Err(invalid("FCPE evidence exceeds the decoded source duration"));
    }
    source_start
        .checked_add(local_end)
        .ok_or_else(|| invalid("FCPE evidence overflows the source timeline"))?;
    let mut model = BTreeMap::new();
    model.insert("id".to_string(), serde_json::json!(raw.model_id));
    model.insert(
        "weights_bytes".to_string(),
        serde_json::json!(raw.model_gguf_size_bytes),
    );
    model.insert("artifact_format".to_string(), serde_json::json!("gguf_f32"));
    model.insert(
        "runtime_manifest_sha256".to_string(),
        serde_json::json!(raw.runtime_manifest_sha256),
    );
    model.insert("backend".to_string(), serde_json::json!(raw.backend));
    model.insert(
        "sample_rate".to_string(),
        serde_json::json!(raw.sample_rate),
    );
    model.insert(
        "window_samples".to_string(),
        serde_json::json!(raw.window_samples),
    );
    Ok(PitchEvidenceV03 {
        format: "uta.pitch-evidence".to_string(),
        format_version: "0.3.0".to_string(),
        timebase: u64::from(CANONICAL_TIMEBASE),
        start: source_start,
        hop,
        frequency_hz,
        confidence,
        model,
    })
}

fn seconds_to_canonical(seconds: f64) -> EngineResult<u64> {
    if !seconds.is_finite() || seconds < 0.0 {
        return Err(invalid("pitch evidence time is invalid"));
    }
    let units = seconds * f64::from(CANONICAL_TIMEBASE);
    if units > u64::MAX as f64 {
        return Err(invalid("pitch evidence time overflows"));
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
    fn maps_unvoiced_frames_without_quantizing_continuous_pitch() {
        let path = std::env::temp_dir().join(format!("uta-rmvpe-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "rmvpe",
                "source_model_sha256": RMVPE_SOURCE_SHA256,
                "model_gguf_sha256": "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2",
                "runtime_manifest_sha256": "d".repeat(64),
                "backend": "ggml_cpu",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "frames": [
                    {"time":0.0,"hz":439.7,"confidence":0.9,"voiced":true},
                    {"time":0.01,"hz":120.0,"confidence":0.01,"voiced":false}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let evidence = parse_rmvpe_pitch(&path, 2_000_000, 20_000).unwrap();
        assert_eq!(evidence.start, 2_000_000);
        assert_eq!(evidence.hop, 10_000);
        assert_eq!(evidence.frequency_hz, [Some(439.7_f32 as f64), None]);
        assert_eq!(
            evidence.confidence,
            [Some(0.9_f32 as f64), Some(0.01_f32 as f64)]
        );
        assert_eq!(
            parse_rmvpe_pitch(&path, 0, 9_999).unwrap_err().code,
            EngineErrorCode::OutputValidationFailed
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_ggml_rmvpe_with_truthful_gguf_identity() {
        let path = std::env::temp_dir().join(format!("uta-rmvpe-ggml-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "rmvpe",
                "source_model_sha256": RMVPE_SOURCE_SHA256,
                "model_gguf_sha256": "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2",
                "runtime_manifest_sha256": "d".repeat(64),
                "backend": "ggml_vulkan",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "frames": [
                    {"time":0.0,"hz":220.3,"confidence":0.88,"voiced":true},
                    {"time":0.01,"hz":120.0,"confidence":0.01,"voiced":false}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let evidence = parse_rmvpe_pitch(&path, 0, 10_000).unwrap();
        assert_eq!(evidence.model["backend"], "ggml_vulkan");
        assert_eq!(evidence.model["artifact_format"], "gguf_f32");
        assert_eq!(
            evidence.model["weights_sha256"],
            "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2"
        );
        assert!(!evidence.model.contains_key("manifest_sha256"));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_retired_rmvpe_backend_identity() {
        let path = std::env::temp_dir().join(format!("uta-rmvpe-wgpu-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "rmvpe",
                "source_model_sha256": RMVPE_SOURCE_SHA256,
                "model_gguf_sha256": "1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2",
                "runtime_manifest_sha256": "d".repeat(64),
                "backend": "wgpu_vulkan",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "frames": [
                    {"time":0.0,"hz":220.3,"confidence":0.88,"voiced":true},
                    {"time":0.01,"hz":120.0,"confidence":0.01,"voiced":false}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(parse_rmvpe_pitch(&path, 0, 10_000).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_retired_fcpe_shape() {
        let path = std::env::temp_dir().join(format!("uta-fcpe-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "fcpe",
                "source_model_sha256": "a".repeat(64),
                "model_manifest_sha256": "b".repeat(64),
                "model_xml_sha256": "c".repeat(64),
                "model_bin_sha256": "d".repeat(64),
                "runtime_manifest_sha256": "e".repeat(64),
                "backend": "wgpu_vulkan",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "window_samples": 32000,
                "window_hop_samples": 32000,
                "frames": [
                    {"time":0.0,"hz":523.4293},
                    {"time":0.01,"hz":null}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(parse_fcpe_pitch(&path, 0, 10_000).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn parses_rust_ggml_fcpe_with_truthful_size_identity() {
        let path = std::env::temp_dir().join(format!("uta-fcpe-ggml-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "fcpe",
                "model_gguf_size_bytes": 43_309_760,
                "runtime_manifest_sha256": "e".repeat(64),
                "backend": "ggml_cpu",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "window_samples": 32000,
                "window_hop_samples": 32000,
                "frames": [
                    {"time":0.0,"hz":523.4293},
                    {"time":0.01,"hz":null}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let evidence = parse_fcpe_pitch(&path, 0, 10_000).unwrap();
        assert_eq!(evidence.confidence, [None, None]);
        assert_eq!(evidence.model["backend"], "ggml_cpu");
        assert_eq!(evidence.model["artifact_format"], "gguf_f32");
        assert_eq!(evidence.model["weights_bytes"], 43_309_760);
        assert!(!evidence.model.contains_key("source_sha256"));
        assert!(!evidence.model.contains_key("weights_sha256"));
        assert!(!evidence.model.contains_key("manifest_sha256"));
        std::fs::remove_file(path).unwrap();
    }
}
