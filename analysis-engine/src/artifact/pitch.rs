use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: usize = 4 * 60 * 60 * 100;
#[cfg(test)]
const RMVPE_SOURCE_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";
#[cfg(test)]
const RMVPE_MANIFEST_SHA256: &str =
    "cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb";
#[cfg(test)]
const RMVPE_BIN_SHA256: &str = "d284ea1b4a0908072b6f0a5a1298cb510a65752db7a287e48da6eab1246be67b";
#[cfg(test)]
const FCPE_SOURCE_SHA256: &str = "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0";
#[cfg(test)]
const FCPE_MANIFEST_SHA256: &str =
    "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6";
#[cfg(test)]
const FCPE_XML_SHA256: &str = "9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237";
#[cfg(test)]
const FCPE_BIN_SHA256: &str = "6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824";

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
    model_manifest_sha256: String,
    model_bin_sha256: String,
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
        || !matches!(raw.backend.as_str(), "openvino_gpu" | "openvino_cpu")
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
        "manifest_sha256".to_string(),
        serde_json::json!(raw.model_manifest_sha256),
    );
    model.insert(
        "weights_sha256".to_string(),
        serde_json::json!(raw.model_bin_sha256),
    );
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
struct FcpeEvidenceV2 {
    schema_version: u32,
    model_id: String,
    source_model_sha256: String,
    model_manifest_sha256: String,
    model_xml_sha256: String,
    model_bin_sha256: String,
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
    let raw: FcpeEvidenceV2 = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read FCPE evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("FCPE evidence JSON is invalid: {error}")))?;
    if raw.schema_version != 3
        || raw.model_id != "fcpe"
        || !matches!(raw.backend.as_str(), "openvino_gpu" | "openvino_cpu")
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
        "source_sha256".to_string(),
        serde_json::json!(raw.source_model_sha256),
    );
    model.insert(
        "manifest_sha256".to_string(),
        serde_json::json!(raw.model_manifest_sha256),
    );
    model.insert(
        "xml_sha256".to_string(),
        serde_json::json!(raw.model_xml_sha256),
    );
    model.insert(
        "weights_sha256".to_string(),
        serde_json::json!(raw.model_bin_sha256),
    );
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
                "model_manifest_sha256": RMVPE_MANIFEST_SHA256,
                "model_bin_sha256": RMVPE_BIN_SHA256,
                "runtime_manifest_sha256": "d".repeat(64),
                "backend": "openvino_gpu",
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
    fn parses_fcpe_without_fabricating_confidence() {
        let path = std::env::temp_dir().join(format!("uta-fcpe-{}.json", std::process::id()));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 3,
                "model_id": "fcpe",
                "source_model_sha256": FCPE_SOURCE_SHA256,
                "model_manifest_sha256": FCPE_MANIFEST_SHA256,
                "model_xml_sha256": FCPE_XML_SHA256,
                "model_bin_sha256": FCPE_BIN_SHA256,
                "runtime_manifest_sha256": "e".repeat(64),
                "backend": "openvino_gpu",
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
        assert_eq!(evidence.frequency_hz, [Some(523.4293_f32 as f64), None]);
        assert_eq!(evidence.confidence, [None, None]);
        std::fs::remove_file(path).unwrap();
    }
}
