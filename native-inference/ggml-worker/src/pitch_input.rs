use std::path::Path;

use serde::Deserialize;

const MAX_EVIDENCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FRAMES: usize = 4 * 60 * 60 * 100;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
    schema_version: u32,
    model_id: String,
    source_model_sha256: String,
    model_gguf_sha256: String,
    runtime_manifest_sha256: String,
    backend: String,
    timeline_step_ms: u32,
    sample_rate: u32,
    frames: Vec<Frame>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    time: f64,
    hz: f32,
    confidence: f32,
    voiced: bool,
}

/// Reads the already-executed RMVPE artifact used by the Analysis Engine.
/// Digest-looking fields are provenance only; structural and semantic checks,
/// not re-hashing, establish whether this typed artifact can be consumed.
pub fn read_shared_rmvpe(path: &Path) -> Result<Vec<f32>, String> {
    let metadata = path
        .metadata()
        .map_err(|error| format!("shared RMVPE evidence is unavailable: {error}"))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EVIDENCE_BYTES {
        return Err("shared RMVPE evidence size is invalid".to_string());
    }
    let evidence: Evidence = serde_json::from_slice(
        &std::fs::read(path)
            .map_err(|error| format!("could not read shared RMVPE evidence: {error}"))?,
    )
    .map_err(|error| format!("shared RMVPE evidence JSON is invalid: {error}"))?;
    if evidence.schema_version != 1
        || evidence.model_id != "rmvpe"
        || evidence.source_model_sha256.trim().is_empty()
        || evidence.model_gguf_sha256.trim().is_empty()
        || evidence.runtime_manifest_sha256.trim().is_empty()
        || !matches!(evidence.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
        || evidence.timeline_step_ms != 10
        || evidence.sample_rate != 16_000
        || evidence.frames.is_empty()
        || evidence.frames.len() > MAX_FRAMES
    {
        return Err("shared RMVPE evidence identity is invalid".to_string());
    }
    evidence
        .frames
        .into_iter()
        .enumerate()
        .map(|(index, frame)| {
            let expected = index as f64 * 0.01;
            if !frame.time.is_finite()
                || (frame.time - expected).abs() > 1.0e-6
                || !frame.hz.is_finite()
                || frame.hz <= 0.0
                || !frame.confidence.is_finite()
                || !(0.0..=1.0).contains(&frame.confidence)
                || frame.voiced != (frame.confidence >= 0.03)
            {
                return Err(format!("shared RMVPE frame {index} is invalid"));
            }
            Ok(if frame.voiced { frame.hz } else { 0.0 })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_shared_pitch_preserves_unvoiced_frames_without_hash_verification() {
        let path = std::env::temp_dir().join(format!(
            "uta-shared-rmvpe-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "model_id": "rmvpe",
                "source_model_sha256": "provenance-source",
                "model_gguf_sha256": "provenance-gguf",
                "runtime_manifest_sha256": "provenance-runtime",
                "backend": "ggml_vulkan",
                "timeline_step_ms": 10,
                "sample_rate": 16000,
                "frames": [
                    {"time":0.0,"hz":440.0,"confidence":0.9,"voiced":true},
                    {"time":0.01,"hz":220.0,"confidence":0.0,"voiced":false}
                ]
            }))
            .unwrap(),
        )
        .unwrap();
        assert_eq!(read_shared_rmvpe(&path).unwrap(), [440.0, 0.0]);
        std::fs::remove_file(path).unwrap();
    }
}
