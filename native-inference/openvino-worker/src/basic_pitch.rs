use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::Serialize;

use crate::runtime;

const INPUT_SAMPLES: usize = 43_844;
const XML_SHA256: &str = "9df134bf18c66dde7b678be49329299ff6ca13be465f3df5b10ff38a75e5aa34";
const BIN_SHA256: &str = "50856c2bac689bb6fdc43ae21818e2a63c37f35207dc5adea22d52fc601efab3";
const MANIFEST_SHA256: &str = "01b35925daaeb40995f4e49b495e6f1ce9db47c7f41987b19fdc1b5c35f2c1b7";
const SOURCE_SHA256: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";

#[derive(Serialize)]
struct ActivationFrame {
    time: f64,
    note_max: f32,
    onset_max: f32,
    contour_class: usize,
    contour_confidence: f32,
}

#[derive(Serialize)]
struct BasicPitchEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_manifest_sha256: &'a str,
    model_xml_sha256: &'a str,
    model_bin_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    sample_rate: u32,
    frames: Vec<ActivationFrame>,
}

fn model_dir() -> Result<PathBuf, String> {
    let root = std::env::var_os("UTA_STUDIO_MODELS_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| "UTA_STUDIO_MODELS_PATH is not configured".to_string())?;
    let directory = root.join("boundary/basic-pitch/openvino-ir-2026.3.0-smoke");
    if runtime::sha256(&directory.join("manifest.json"))? != MANIFEST_SHA256
        || runtime::sha256(&directory.join("basic-pitch.xml"))? != XML_SHA256
        || runtime::sha256(&directory.join("basic-pitch.bin"))? != BIN_SHA256
    {
        return Err("Basic Pitch OpenVINO IR identity mismatch".to_string());
    }
    Ok(directory)
}

fn maximum(values: &[f32]) -> f32 {
    values.iter().copied().fold(0.0, f32::max)
}

pub fn infer(audio: &[f32], output_dir: &Path) -> Result<PathBuf, String> {
    let runtime_manifest = runtime::validate_runtime()?;
    let directory = model_dir()?;
    let mut core = Core::new().map_err(|error| error.to_string())?;
    if !core
        .available_devices()
        .map_err(|error| error.to_string())?
        .contains(&DeviceType::GPU)
    {
        return Err("OpenVINO GPU is unavailable; CPU fallback is forbidden".to_string());
    }
    core.set_properties(
        &DeviceType::GPU,
        [
            (RwPropertyKey::HintInferencePrecision, "f32"),
            (RwPropertyKey::HintExecutionMode, "ACCURACY"),
        ],
    )
    .map_err(|error| error.to_string())?;
    let graph = core
        .read_model_from_file(
            directory.join("basic-pitch.xml").to_string_lossy().as_ref(),
            directory.join("basic-pitch.bin").to_string_lossy().as_ref(),
        )
        .map_err(|error| format!("could not load Basic Pitch IR: {error}"))?;
    let mut compiled = core
        .compile_model(&graph, DeviceType::GPU)
        .map_err(|error| format!("could not compile Basic Pitch for GPU: {error}"))?;
    let mut request = compiled
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let shape = Shape::new(&[1, INPUT_SAMPLES as i64, 1]).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    let input = tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?;
    input.fill(0.0);
    input[..audio.len().min(INPUT_SAMPLES)]
        .copy_from_slice(&audio[..audio.len().min(INPUT_SAMPLES)]);
    request
        .set_input_tensor(&tensor)
        .map_err(|error| error.to_string())?;
    request
        .infer()
        .map_err(|error| format!("Basic Pitch GPU inference failed: {error}"))?;
    let notes = request
        .get_output_tensor_by_index(0)
        .map_err(|error| error.to_string())?;
    let onsets = request
        .get_output_tensor_by_index(1)
        .map_err(|error| error.to_string())?;
    let contours = request
        .get_output_tensor_by_index(2)
        .map_err(|error| error.to_string())?;
    let notes = notes.get_data::<f32>().map_err(|error| error.to_string())?;
    let onsets = onsets
        .get_data::<f32>()
        .map_err(|error| error.to_string())?;
    let contours = contours
        .get_data::<f32>()
        .map_err(|error| error.to_string())?;
    let frame_count = notes.len() / 88;
    if onsets.len() != frame_count * 88 || contours.len() != frame_count * 264 {
        return Err("Basic Pitch returned unexpected output shapes".to_string());
    }
    let frames = (0..frame_count)
        .map(|frame| {
            let contour = &contours[frame * 264..(frame + 1) * 264];
            let (contour_class, contour_confidence) = contour
                .iter()
                .copied()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .unwrap_or((0, 0.0));
            ActivationFrame {
                time: frame as f64 * INPUT_SAMPLES as f64 / 22_050.0 / frame_count as f64,
                note_max: maximum(&notes[frame * 88..(frame + 1) * 88]),
                onset_max: maximum(&onsets[frame * 88..(frame + 1) * 88]),
                contour_class,
                contour_confidence,
            }
        })
        .collect();
    let destination = output_dir.join("basic-pitch-activation-evidence.json");
    let temporary = output_dir.join("basic-pitch-activation-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &BasicPitchEvidence {
            schema_version: 1,
            model_id: "basic_pitch",
            source_model_sha256: SOURCE_SHA256,
            model_manifest_sha256: MANIFEST_SHA256,
            model_xml_sha256: XML_SHA256,
            model_bin_sha256: BIN_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: "openvino_gpu",
            sample_rate: 22_050,
            frames,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}
