use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::Serialize;

use crate::runtime;

const INPUT_SAMPLES: usize = 32_000;
const XML_SHA256: &str = "9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237";
const BIN_SHA256: &str = "6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824";
const MANIFEST_SHA256: &str = "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6";
const SOURCE_SHA256: &str = "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0";

#[derive(Serialize)]
struct PitchFrame {
    time: f64,
    hz: f32,
    voiced: bool,
}

#[derive(Serialize)]
struct PitchEvidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    source_model_sha256: &'a str,
    model_manifest_sha256: &'a str,
    model_xml_sha256: &'a str,
    model_bin_sha256: &'a str,
    runtime_manifest_sha256: &'a str,
    backend: &'a str,
    timeline_step_ms: u32,
    frames: Vec<PitchFrame>,
}

fn model_dir() -> Result<PathBuf, String> {
    let root = std::env::var_os("UTA_STUDIO_MODELS_PATH")
        .map(PathBuf::from)
        .ok_or_else(|| "UTA_STUDIO_MODELS_PATH is not configured".to_string())?;
    let directory = root.join("pitch/fcpe/openvino-ir-2026.3.0-smoke");
    let manifest = directory.join("manifest.json");
    let xml = directory.join("fcpe.xml");
    let bin = directory.join("fcpe.bin");
    if runtime::sha256(&manifest)? != MANIFEST_SHA256
        || runtime::sha256(&xml)? != XML_SHA256
        || runtime::sha256(&bin)? != BIN_SHA256
    {
        return Err("FCPE OpenVINO IR identity mismatch".to_string());
    }
    Ok(directory)
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
            directory.join("fcpe.xml").to_string_lossy().as_ref(),
            directory.join("fcpe.bin").to_string_lossy().as_ref(),
        )
        .map_err(|error| format!("could not load FCPE IR: {error}"))?;
    let mut compiled = core
        .compile_model(&graph, DeviceType::GPU)
        .map_err(|error| format!("could not compile FCPE for GPU: {error}"))?;
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
        .map_err(|error| format!("FCPE GPU inference failed: {error}"))?;
    let output = request
        .get_output_tensor()
        .map_err(|error| error.to_string())?;
    let data = output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?;
    let frames = data
        .iter()
        .take(INPUT_SAMPLES / 160 + 1)
        .enumerate()
        .map(|(index, value)| PitchFrame {
            time: index as f64 * 0.01,
            hz: if value.is_finite() { *value } else { 0.0 },
            voiced: value.is_finite() && *value > 0.0,
        })
        .collect();
    let destination = output_dir.join("fcpe-pitch-evidence.json");
    let temporary = output_dir.join("fcpe-pitch-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &PitchEvidence {
            schema_version: 1,
            model_id: "fcpe",
            source_model_sha256: SOURCE_SHA256,
            model_manifest_sha256: MANIFEST_SHA256,
            model_xml_sha256: XML_SHA256,
            model_bin_sha256: BIN_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: "openvino_gpu",
            timeline_step_ms: 10,
            frames,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}
