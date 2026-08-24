use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::mel::{MEL_BINS, SAMPLE_RATE, log_mel_spectrogram, to_channel_major_window};

const SOURCE_MODEL_SHA256: &str =
    "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd";
const MODEL_MANIFEST_SHA256: &str =
    "cdaf2775d8e17796daad2415bdaf7b3c915c4142fd92587c023e8d7b1b3d39fb";
const MODEL_CONVERSION_RECIPE_SHA256: &str =
    "ac3df548a9e51d36b5d5817ba6988eeaaa29f168d121588fd088daf91dbdf876";
const MODEL_BIN_SHA256: &str = "d284ea1b4a0908072b6f0a5a1298cb510a65752db7a287e48da6eab1246be67b";
const MIN_INPUT_FRAMES: usize = 32;
const MAX_INPUT_FRAMES: usize = 1_024;
const FRAME_STEP: usize = 32;
const OVERLAP_FRAMES: usize = 128;
const STRIDE_FRAMES: usize = MAX_INPUT_FRAMES - OVERLAP_FRAMES;
const PITCH_CLASSES: usize = 360;
const CENTS_OFFSET: f32 = 1_997.379_4;

#[derive(Debug, Deserialize)]
struct ModelManifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_onnx_sha256: String,
    runtime_recipe_sha256: String,
    input_frame_buckets: FrameBuckets,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct FrameBuckets {
    minimum: usize,
    maximum: usize,
    step: usize,
    overlap: usize,
}

struct ModelArtifact {
    directory: PathBuf,
    bin: PathBuf,
    files: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct PitchFrame {
    time: f64,
    hz: f32,
    confidence: f32,
    voiced: bool,
}

#[derive(Debug, Serialize)]
struct PitchEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_manifest_sha256: &'a str,
    model_bin_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    timeline_step_ms: u32,
    sample_rate: u32,
    frames: Vec<PitchFrame>,
}

fn model_artifact(config: &serde_json::Value) -> Result<ModelArtifact, String> {
    let directory = if let Some(path) = config.get("model_path").and_then(|value| value.as_str()) {
        let path = PathBuf::from(path);
        if path.is_dir() {
            path
        } else {
            path.parent()
                .ok_or_else(|| "configured RMVPE IR path has no parent directory".to_string())?
                .to_path_buf()
        }
    } else {
        let root = std::env::var_os("UTA_STUDIO_MODELS_PATH")
            .map(PathBuf::from)
            .ok_or_else(|| "UTA_STUDIO_MODELS_PATH is not configured".to_string())?;
        root.join("pitch/rmvpe/openvino-ir-2026.3.0-bucketed")
    };
    let bin = directory.join("rmvpe.bin");
    let manifest_path = directory.join("manifest.json");
    if !bin.is_file() || !manifest_path.is_file() {
        return Err(
            "RMVPE bucketed OpenVINO IR is not installed; use Settings > Models & runtime"
                .to_string(),
        );
    }
    if sha256(&manifest_path)? != MODEL_MANIFEST_SHA256 {
        return Err("RMVPE OpenVINO IR manifest hash mismatch".to_string());
    }
    let manifest: ModelManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("RMVPE OpenVINO IR manifest is invalid: {error}"))?;
    let buckets = &manifest.input_frame_buckets;
    if manifest.schema_version != 2
        || manifest.model_id != "rmvpe"
        || manifest.format != "openvino_ir_v11_bucketed"
        || manifest.source_onnx_sha256 != SOURCE_MODEL_SHA256
        || manifest.runtime_recipe_sha256 != MODEL_CONVERSION_RECIPE_SHA256
        || buckets.minimum != MIN_INPUT_FRAMES
        || buckets.maximum != MAX_INPUT_FRAMES
        || buckets.step != FRAME_STEP
        || buckets.overlap != OVERLAP_FRAMES
        || manifest.files.len() != (MAX_INPUT_FRAMES / FRAME_STEP) + 1
        || manifest.files.get("rmvpe.bin").map(String::as_str) != Some(MODEL_BIN_SHA256)
    {
        return Err("RMVPE OpenVINO IR identity does not match the worker recipe".to_string());
    }
    for frames in (MIN_INPUT_FRAMES..=MAX_INPUT_FRAMES).step_by(FRAME_STEP) {
        let name = format!("rmvpe-{frames:04}.xml");
        if !directory.join(&name).is_file() || !manifest.files.contains_key(&name) {
            return Err(format!("RMVPE OpenVINO IR bucket is missing: {name}"));
        }
    }
    Ok(ModelArtifact {
        directory,
        bin,
        files: manifest.files,
    })
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", digest.finalize()))
}

fn local_average_hz(activation: &[f32]) -> (f32, f32) {
    let (center, confidence) = activation
        .iter()
        .copied()
        .enumerate()
        .max_by(|left, right| left.1.total_cmp(&right.1))
        .unwrap_or((0, 0.0));
    let start = center.saturating_sub(4);
    let end = (center + 4).min(PITCH_CLASSES - 1);
    let mut weighted_cents = 0.0;
    let mut weight = 0.0;
    for (class, salience) in activation
        .iter()
        .copied()
        .enumerate()
        .take(end + 1)
        .skip(start)
    {
        weighted_cents += salience * (20.0 * class as f32 + CENTS_OFFSET);
        weight += salience;
    }
    let cents = if weight > f32::EPSILON {
        weighted_cents / weight
    } else {
        20.0 * center as f32 + CENTS_OFFSET
    };
    (
        10.0 * 2.0_f32.powf(cents / 1_200.0),
        confidence.clamp(0.0, 1.0),
    )
}

fn activation_frame<'a>(
    data: &'a [f32],
    dimensions: &[i64],
    padded_frames: usize,
    frame: usize,
    scratch: &'a mut [f32; PITCH_CLASSES],
) -> Result<&'a [f32], String> {
    match dimensions {
        [1, frames, classes]
            if *frames as usize == padded_frames && *classes as usize == PITCH_CLASSES =>
        {
            let start = frame * PITCH_CLASSES;
            Ok(&data[start..start + PITCH_CLASSES])
        }
        [1, classes, frames]
            if *frames as usize == padded_frames && *classes as usize == PITCH_CLASSES =>
        {
            for class in 0..PITCH_CLASSES {
                scratch[class] = data[class * padded_frames + frame];
            }
            Ok(&scratch[..])
        }
        _ => Err(format!("unexpected RMVPE output shape: {dimensions:?}")),
    }
}

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    let model = model_artifact(config)?;
    progress(0.02, "Validating source-built OpenVINO runtime");
    let runtime_manifest_sha256 = crate::runtime::validate_runtime()?;
    progress(0.03, "Validating RMVPE OpenVINO IR identity");
    if sha256(&model.bin)? != MODEL_BIN_SHA256 {
        return Err("RMVPE OpenVINO IR weights hash mismatch".to_string());
    }

    progress(0.08, "Computing native log-mel features");
    let (frame_major, frames) = log_mel_spectrogram(audio, |fraction| {
        progress(0.08 + 0.32 * fraction, "Computing native log-mel features");
    })?;

    progress(0.42, "Loading RMVPE OpenVINO IR on GPU");
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    let devices = core
        .available_devices()
        .map_err(|error| format!("could not enumerate OpenVINO devices: {error}"))?;
    if !devices
        .iter()
        .any(|device| matches!(device, DeviceType::GPU))
    {
        return Err(
            "OpenVINO GPU is unavailable; CPU production fallback is forbidden".to_string(),
        );
    }
    core.set_properties(
        &DeviceType::GPU,
        [
            (RwPropertyKey::HintInferencePrecision, "f32"),
            (RwPropertyKey::HintExecutionMode, "ACCURACY"),
            (
                RwPropertyKey::Other("GPU_ENABLE_LOOP_UNROLLING".into()),
                "NO",
            ),
        ],
    )
    .map_err(|error| format!("could not configure OpenVINO GPU accuracy mode: {error}"))?;
    crate::runtime::configure_low_impact_gpu_queue(&mut core)?;
    let bin_text = model
        .bin
        .to_str()
        .ok_or_else(|| "RMVPE IR weights path is not valid UTF-8".to_string())?;
    let window_count = if frames <= MAX_INPUT_FRAMES {
        1
    } else {
        (frames - MAX_INPUT_FRAMES).div_ceil(STRIDE_FRAMES) + 1
    };
    let mut compiled_models = BTreeMap::<usize, CompiledModel>::new();
    let mut evidence_frames = Vec::with_capacity(frames);
    let mut scratch = [0.0; PITCH_CLASSES];
    let mut start = 0;
    for window in 0..window_count {
        let remaining = frames - start;
        let final_window = remaining <= MAX_INPUT_FRAMES;
        let input_frames = remaining
            .clamp(MIN_INPUT_FRAMES, MAX_INPUT_FRAMES)
            .div_ceil(FRAME_STEP)
            * FRAME_STEP;
        if let std::collections::btree_map::Entry::Vacant(entry) =
            compiled_models.entry(input_frames)
        {
            let name = format!("rmvpe-{input_frames:04}.xml");
            let xml = model.directory.join(&name);
            let expected = model
                .files
                .get(&name)
                .ok_or_else(|| format!("RMVPE IR manifest is missing {name}"))?;
            if sha256(&xml)? != *expected {
                return Err(format!("RMVPE OpenVINO IR graph hash mismatch: {name}"));
            }
            let model_text = xml
                .to_str()
                .ok_or_else(|| "RMVPE IR graph path is not valid UTF-8".to_string())?;
            let graph = core
                .read_model_from_file(model_text, bin_text)
                .map_err(|error| format!("could not read RMVPE OpenVINO IR: {error}"))?;
            let compiled = core
                .compile_model(&graph, DeviceType::GPU)
                .map_err(|error| format!("could not compile RMVPE IR for GPU: {error}"))?;
            entry.insert(compiled);
        }
        let compiled = compiled_models
            .get_mut(&input_frames)
            .ok_or_else(|| "compiled RMVPE IR bucket disappeared".to_string())?;
        let mut request = compiled
            .create_infer_request()
            .map_err(|error| format!("could not create RMVPE inference request: {error}"))?;
        let shape = Shape::new(&[1, MEL_BINS as i64, input_frames as i64])
            .map_err(|error| error.to_string())?;
        let mut tensor =
            Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
        let mel = to_channel_major_window(&frame_major, frames, start, input_frames);
        tensor
            .get_data_mut::<f32>()
            .map_err(|error| error.to_string())?
            .copy_from_slice(&mel);
        request
            .set_input_tensor(&tensor)
            .map_err(|error| format!("could not bind RMVPE IR input: {error}"))?;
        progress(
            0.55 + 0.3 * window as f32 / window_count as f32,
            "Running RMVPE OpenVINO IR on GPU",
        );
        request
            .infer()
            .map_err(|error| format!("RMVPE OpenVINO GPU inference failed: {error}"))?;
        let output = request
            .get_output_tensor()
            .map_err(|error| format!("could not read RMVPE output: {error}"))?;
        let dimensions = output
            .get_shape()
            .map_err(|error| error.to_string())?
            .get_dimensions()
            .to_vec();
        let data = output
            .get_data::<f32>()
            .map_err(|error| format!("RMVPE output is not float32: {error}"))?;
        let keep_start = if start == 0 { 0 } else { OVERLAP_FRAMES / 2 };
        let keep_end = if final_window {
            remaining
        } else {
            MAX_INPUT_FRAMES - OVERLAP_FRAMES / 2
        };
        for local_frame in keep_start..keep_end {
            let activation =
                activation_frame(data, &dimensions, input_frames, local_frame, &mut scratch)?;
            let (hz, confidence) = local_average_hz(activation);
            let frame = start + local_frame;
            evidence_frames.push(PitchFrame {
                time: frame as f64 * 0.01,
                hz,
                confidence,
                voiced: confidence >= 0.03,
            });
        }
        if final_window {
            break;
        }
        start += STRIDE_FRAMES;
    }
    if evidence_frames.len() != frames {
        return Err("RMVPE overlap stitching did not preserve the evidence timeline".to_string());
    }
    progress(0.88, "Decoding calibrated pitch evidence");
    let evidence = PitchEvidence {
        schema_version: 1,
        model_id: "rmvpe",
        source_model_sha256: SOURCE_MODEL_SHA256,
        model_manifest_sha256: MODEL_MANIFEST_SHA256,
        model_bin_sha256: MODEL_BIN_SHA256,
        runtime_manifest_sha256: &runtime_manifest_sha256,
        backend: "openvino_gpu",
        timeline_step_ms: 10,
        sample_rate: SAMPLE_RATE as u32,
        frames: evidence_frames,
    };
    let temporary = output_dir.join("rmvpe-pitch-evidence.json.tmp");
    let destination = output_dir.join("rmvpe-pitch-evidence.json");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(&mut file, &evidence).map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    progress(1.0, "RMVPE evidence complete");
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_average_uses_nearby_salience_without_rounding_to_midi() {
        let mut activation = [0.0; PITCH_CLASSES];
        activation[100] = 0.8;
        activation[101] = 0.4;
        let (hz, confidence) = local_average_hz(&activation);
        assert!(hz.is_finite() && hz > 0.0);
        assert_eq!(confidence, 0.8);
        let center_only = 10.0 * 2.0_f32.powf((20.0 * 100.0 + CENTS_OFFSET) / 1_200.0);
        assert_ne!(hz, center_only);
    }
}
