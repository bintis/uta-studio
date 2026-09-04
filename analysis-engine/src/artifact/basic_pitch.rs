use std::path::Path;

use serde::Deserialize;

use crate::contract::{CANONICAL_TIMEBASE, EngineError, EngineErrorCode, EngineResult};

const MAX_EVIDENCE_BYTES: u64 = 64 * 1024 * 1024;
#[cfg(test)]
const SOURCE_SHA256: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";
#[cfg(test)]
const MANIFEST_SHA256: &str = "01b35925daaeb40995f4e49b495e6f1ce9db47c7f41987b19fdc1b5c35f2c1b7";
#[cfg(test)]
const XML_SHA256: &str = "9df134bf18c66dde7b678be49329299ff6ca13be465f3df5b10ff38a75e5aa34";
#[cfg(test)]
const BIN_SHA256: &str = "50856c2bac689bb6fdc43ae21818e2a63c37f35207dc5adea22d52fc601efab3";

#[derive(Debug, Clone, PartialEq)]
pub struct BasicPitchEvidenceV3 {
    pub frames: Vec<BasicPitchFrameV3>,
    pub model_manifest_sha256: String,
    pub runtime_manifest_sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BasicPitchFrameV3 {
    pub time: u64,
    pub note_activation: f32,
    pub onset_activation: f32,
    pub contour_class: usize,
    pub contour_activation: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvidence {
    schema_version: u32,
    model_id: String,
    #[serde(rename = "source_model_sha256")]
    _source_model_sha256: String,
    model_manifest_sha256: String,
    #[serde(rename = "model_xml_sha256")]
    _model_xml_sha256: String,
    #[serde(rename = "model_bin_sha256")]
    _model_bin_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    fft_hop_samples: u32,
    overlap_frames: usize,
    padding_samples: u32,
    frames_per_window: usize,
    owned_frames_per_window: usize,
    frames: Vec<RawFrame>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawFrame {
    time: f64,
    note_max: f32,
    onset_max: f32,
    contour_class: usize,
    contour_score: f32,
}

pub fn parse_basic_pitch_evidence(
    path: &Path,
    source_start: u64,
    source_duration: u64,
) -> EngineResult<BasicPitchEvidenceV3> {
    let metadata = std::fs::metadata(path)
        .map_err(|error| invalid(format!("Basic Pitch evidence is unavailable: {error}")))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err(invalid("Basic Pitch evidence size is invalid"));
    }
    let raw: RawEvidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| invalid(format!("could not read Basic Pitch evidence: {error}")))?,
    )
    .map_err(|error| invalid(format!("Basic Pitch evidence JSON is invalid: {error}")))?;
    if raw.schema_version != 3
        || raw.model_id != "basic_pitch"
        || !matches!(
            raw.backend.as_str(),
            "openvino_gpu" | "openvino_cpu" | "ggml_native"
        )
        || raw.sample_rate != 22_050
        || raw.window_samples != 43_844
        || raw.window_hop_samples != 36_164
        || raw.fft_hop_samples != 256
        || raw.overlap_frames != 30
        || raw.padding_samples != 3_840
        || raw.frames_per_window != 172
        || raw.owned_frames_per_window != 142
        || raw.frames.is_empty()
    {
        return Err(invalid("Basic Pitch evidence identity is invalid"));
    }
    let mut frames = Vec::with_capacity(raw.frames.len());
    let mut previous = None;
    for (index, frame) in raw.frames.into_iter().enumerate() {
        if !frame.time.is_finite()
            || frame.time < 0.0
            || frame.contour_class >= 264
            || !valid_activation(frame.note_max)
            || !valid_activation(frame.onset_max)
            || !valid_activation(frame.contour_score)
        {
            return Err(invalid("Basic Pitch frame is invalid"));
        }
        let local = (frame.time * f64::from(CANONICAL_TIMEBASE)).round();
        let expected = ((index * 256) as f64 / 22_050.0 * f64::from(CANONICAL_TIMEBASE)).round();
        if local < 0.0 || local > source_duration as f64 || (local - expected).abs() > 1.0 {
            return Err(invalid("Basic Pitch frame is outside the source timeline"));
        }
        let time = source_start
            .checked_add(local as u64)
            .ok_or_else(|| invalid("Basic Pitch timeline overflows"))?;
        if previous.is_some_and(|old| time <= old) {
            return Err(invalid("Basic Pitch timeline is not strictly ordered"));
        }
        previous = Some(time);
        frames.push(BasicPitchFrameV3 {
            time,
            note_activation: frame.note_max,
            onset_activation: frame.onset_max,
            contour_class: frame.contour_class,
            contour_activation: frame.contour_score,
        });
    }
    Ok(BasicPitchEvidenceV3 {
        frames,
        model_manifest_sha256: raw.model_manifest_sha256,
        runtime_manifest_sha256: raw.runtime_manifest_sha256,
    })
}

fn valid_activation(value: f32) -> bool {
    value.is_finite() && (0.0..=1.0).contains(&value)
}

fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::new(EngineErrorCode::OutputValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_keeps_source_local_activations_without_confidence_claims() {
        let path =
            std::env::temp_dir().join(format!("uta-basic-pitch-{}.json", std::process::id()));
        let second_time = 256.0 / 22_050.0;
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 3,
                "model_id": "basic_pitch",
                "source_model_sha256": SOURCE_SHA256,
                "model_manifest_sha256": MANIFEST_SHA256,
                "model_xml_sha256": XML_SHA256,
                "model_bin_sha256": BIN_SHA256,
                "runtime_manifest_sha256": "a".repeat(64),
                "backend": "openvino_gpu",
                "sample_rate": 22050,
                "window_samples": 43844,
                "window_hop_samples": 36164,
                "fft_hop_samples": 256,
                "overlap_frames": 30,
                "padding_samples": 3840,
                "frames_per_window": 172,
                "owned_frames_per_window": 142,
                "frames": [
                    {"time": 0.0, "note_max": 0.2, "onset_max": 0.3, "contour_class": 4, "contour_score": 0.4},
                    {"time": second_time, "note_max": 0.5, "onset_max": 0.6, "contour_class": 5, "contour_score": 0.7}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        let evidence = parse_basic_pitch_evidence(&path, 1_000_000, 20_000).unwrap();
        assert_eq!(evidence.frames[1].time, 1_011_610);
        assert_eq!(evidence.frames[1].onset_activation, 0.6);
        std::fs::remove_file(path).unwrap();
    }
}
