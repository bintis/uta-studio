use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use uta_ggml_runtime::firered::{
    ENCODER_FRAMES, FEATURE_FRAMES, FireRed, MAX_GENERATED_TOKENS, MAX_WINDOW_SAMPLES, SAMPLE_RATE,
    Transcription, VOCAB_SIZE, WINDOW_OVERLAP_SAMPLES,
};
use uta_ggml_runtime::{DeviceDescriptor, GgmlRuntime};

const MODEL_ID: &str = "firered_asr2_aed";
const SOURCE_REVISION: &str =
    "FireRedTeam/FireRedASR2-AED@2304afed56eacfee6256dee5937ed22ffa0b64ec";

#[derive(Debug, Deserialize)]
struct Request {
    model_content_digest: String,
    model_artifacts: BTreeMap<String, PathBuf>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Evidence {
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
    /// Windows whose decode never reached EOS and contributed no text.
    /// Instrumental passages make this expected on a song.
    #[serde(default)]
    unfinished_windows: usize,
    feature_frames: usize,
    encoder_frames: usize,
    max_generated_tokens: usize,
    text: String,
    token_ids: Vec<u32>,
    windows: Vec<WindowEvidence>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WindowEvidence {
    index: usize,
    start_sample: usize,
    end_sample: usize,
    text: String,
    token_ids: Vec<u32>,
}

#[allow(clippy::too_many_arguments)]
pub fn infer(
    runtime: Arc<GgmlRuntime>,
    device: &DeviceDescriptor,
    model_path: &Path,
    wav: &Path,
    runtime_content_digest: &str,
    backend: &str,
    config: &serde_json::Value,
    destination: &Path,
    progress: impl FnMut(u64, u64),
) -> Result<(), String> {
    let request: Request = serde_json::from_value(config.clone())
        .map_err(|error| format!("FireRed request is invalid: {error}"))?;
    if request.model_content_digest.trim().is_empty()
        || runtime_content_digest.trim().is_empty()
        || !matches!(backend, "ggml_cpu" | "ggml_vulkan")
    {
        return Err("FireRed execution provenance is invalid".to_string());
    }
    let configured_model = named_artifact(&request, "model")?;
    if configured_model != model_path {
        return Err("FireRed named model artifact disagrees with model_path".to_string());
    }
    let cmvn = std::fs::read(named_artifact(&request, "cmvn")?)
        .map_err(|error| format!("FireRed CMVN sidecar is unavailable: {error}"))?;
    let tokens = std::fs::read(named_artifact(&request, "tokens")?)
        .map_err(|error| format!("FireRed token sidecar is unavailable: {error}"))?;
    let model = FireRed::load(runtime, device, model_path)?;
    let transcription = model.transcribe_wav(wav, &cmvn, &tokens, progress)?;
    let evidence = evidence(
        request.model_content_digest,
        runtime_content_digest,
        backend,
        transcription,
    )?;
    write_evidence(destination, &evidence)
}

pub fn publish(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.exists() {
        return Err("FireRed transcript evidence destination already exists".to_string());
    }
    let bytes = std::fs::read(source)
        .map_err(|error| format!("could not read raw FireRed evidence: {error}"))?;
    let evidence: Evidence = serde_json::from_slice(&bytes)
        .map_err(|error| format!("raw FireRed evidence is invalid: {error}"))?;
    validate_evidence(&evidence)?;
    let temporary = destination.with_extension(format!(
        "json.{}.{}.tmp",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("could not create FireRed publication temporary: {error}"))?;
        encode_evidence(&mut file, &evidence)?;
        drop(file);
        std::fs::hard_link(&temporary, destination).map_err(|error| {
            format!("could not publish FireRed transcript without overwrite: {error}")
        })?;
        std::fs::remove_file(&temporary)
            .map_err(|error| format!("could not remove FireRed publication temporary: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn named_artifact<'a>(request: &'a Request, name: &str) -> Result<&'a Path, String> {
    request
        .model_artifacts
        .get(name)
        .map(PathBuf::as_path)
        .filter(|path| path.is_file())
        .ok_or_else(|| format!("FireRed named artifact {name} is unavailable"))
}

fn evidence(
    model_content_digest: String,
    runtime_content_digest: &str,
    backend: &str,
    transcription: Transcription,
) -> Result<Evidence, String> {
    let input_samples = transcription
        .windows
        .last()
        .map(|window| window.end_sample)
        .ok_or_else(|| "FireRed transcription has no windows".to_string())?;
    let evidence = Evidence {
        schema_version: 2,
        model_id: MODEL_ID.to_string(),
        selected_source_revision: SOURCE_REVISION.to_string(),
        model_content_digest,
        runtime_content_digest: runtime_content_digest.to_string(),
        backend: backend.to_string(),
        contract_scope: "overlapping_windowed_230_feature_frame_sequence".to_string(),
        sample_rate: SAMPLE_RATE,
        input_samples,
        window_samples: MAX_WINDOW_SAMPLES,
        window_overlap_samples: WINDOW_OVERLAP_SAMPLES,
        window_count: transcription.windows.len(),
        unfinished_windows: transcription.unfinished_windows,
        feature_frames: FEATURE_FRAMES,
        encoder_frames: ENCODER_FRAMES,
        max_generated_tokens: MAX_GENERATED_TOKENS,
        text: transcription.text,
        token_ids: transcription.token_ids,
        windows: transcription
            .windows
            .into_iter()
            .map(|window| WindowEvidence {
                index: window.index,
                start_sample: window.start_sample,
                end_sample: window.end_sample,
                text: window.text,
                token_ids: window.token_ids,
            })
            .collect(),
    };
    validate_evidence(&evidence)?;
    Ok(evidence)
}

fn validate_evidence(evidence: &Evidence) -> Result<(), String> {
    let expected_ranges = sample_windows(evidence.input_samples)?;
    let windows_valid = evidence.windows.len() == expected_ranges.len()
        && evidence
            .windows
            .iter()
            .zip(expected_ranges)
            .enumerate()
            .all(|(index, (window, (start, end)))| {
                window.index == index
                    && window.start_sample == start
                    && window.end_sample == end
                    && window.text == window.text.trim()
                    && window.token_ids.len() < MAX_GENERATED_TOKENS
                    && window
                        .token_ids
                        .iter()
                        .all(|token| (*token as usize) < VOCAB_SIZE)
            });
    if evidence.schema_version != 2
        || evidence.model_id != MODEL_ID
        || evidence.selected_source_revision != SOURCE_REVISION
        || evidence.model_content_digest.trim().is_empty()
        || evidence.runtime_content_digest.trim().is_empty()
        || !matches!(evidence.backend.as_str(), "ggml_cpu" | "ggml_vulkan")
        || evidence.contract_scope != "overlapping_windowed_230_feature_frame_sequence"
        || evidence.sample_rate != SAMPLE_RATE
        || evidence.input_samples == 0
        || evidence.window_samples != MAX_WINDOW_SAMPLES
        || evidence.window_overlap_samples != WINDOW_OVERLAP_SAMPLES
        || evidence.window_count != evidence.windows.len()
        || evidence.feature_frames != FEATURE_FRAMES
        || evidence.encoder_frames != ENCODER_FRAMES
        || evidence.max_generated_tokens != MAX_GENERATED_TOKENS
        || evidence.text.trim().is_empty()
        || evidence.text != evidence.text.trim()
        || evidence.token_ids.is_empty()
        || evidence
            .token_ids
            .iter()
            .any(|token| (*token as usize) >= VOCAB_SIZE)
        || !windows_valid
    {
        return Err("raw FireRed transcript evidence is structurally invalid".to_string());
    }
    let mut assembled = Vec::new();
    for window in &evidence.windows {
        merge_token_ids(&mut assembled, &window.token_ids);
    }
    if assembled != evidence.token_ids {
        return Err("FireRed assembled token identity is invalid".to_string());
    }
    Ok(())
}

fn sample_windows(samples: usize) -> Result<Vec<(usize, usize)>, String> {
    if samples == 0 {
        return Err("FireRed sample count is empty".to_string());
    }
    if samples <= MAX_WINDOW_SAMPLES {
        return Ok(vec![(0, samples)]);
    }
    let step = MAX_WINDOW_SAMPLES - WINDOW_OVERLAP_SAMPLES;
    let mut windows = Vec::new();
    let mut start = 0usize;
    loop {
        let end = start.saturating_add(MAX_WINDOW_SAMPLES).min(samples);
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

fn write_evidence(destination: &Path, evidence: &Evidence) -> Result<(), String> {
    if destination.exists() {
        return Err("raw FireRed transcript target already exists".to_string());
    }
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(|error| format!("could not create raw FireRed evidence: {error}"))?;
    encode_evidence(&mut file, evidence)
}

fn encode_evidence(writer: &mut impl Write, evidence: &Evidence) -> Result<(), String> {
    serde_json::to_writer(&mut *writer, evidence)
        .map_err(|error| format!("could not encode FireRed evidence: {error}"))?;
    writer
        .write_all(b"\n")
        .map_err(|error| format!("could not finish FireRed evidence: {error}"))?;
    writer
        .flush()
        .map_err(|error| format!("could not flush FireRed evidence: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use uta_ggml_runtime::firered::TranscriptWindow;

    fn transcription() -> Transcription {
        Transcription {
            text: "你好世界".to_string(),
            token_ids: vec![1202, 2246, 1019, 4710],
            windows: vec![TranscriptWindow {
                index: 0,
                start_sample: 0,
                end_sample: 37_040,
                text: "你好世界".to_string(),
                token_ids: vec![1202, 2246, 1019, 4710],
            }],
            unfinished_windows: 0,
        }
    }

    #[test]
    fn evidence_preserves_bounded_transcript_and_provenance() {
        let evidence = evidence(
            "model-generation".to_string(),
            "runtime-generation",
            "ggml_vulkan",
            transcription(),
        )
        .unwrap();
        assert_eq!(evidence.schema_version, 2);
        assert_eq!(evidence.text, "你好世界");
        assert_eq!(evidence.token_ids, [1202, 2246, 1019, 4710]);
        assert_eq!(evidence.windows.len(), 1);
    }

    #[test]
    fn evidence_rejects_inconsistent_assembled_tokens() {
        let mut evidence = evidence(
            "model-generation".to_string(),
            "runtime-generation",
            "ggml_cpu",
            transcription(),
        )
        .unwrap();
        evidence.token_ids.push(42);
        assert!(validate_evidence(&evidence).is_err());
    }
}
