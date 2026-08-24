use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, ElementType, Shape, Tensor};
use serde::Serialize;

use crate::runtime;

const SAMPLE_RATE: usize = 16_000;
const INPUT_SAMPLES: usize = 32_000;
const FRAME_HOP_SAMPLES: usize = 160;
const OUTPUT_FRAMES: usize = INPUT_SAMPLES / FRAME_HOP_SAMPLES + 1;
const XML_SHA256: &str = "9941d7251ff0bdedc7875cabd40c30c2c60db00b36a617c9e957044d669bc237";
const BIN_SHA256: &str = "6b6c62535552181c9efe305837af09a2a8987585ce368b2c522242b59676f824";
const MANIFEST_SHA256: &str = "bd356b9d018bbf55f7b87bbc8e4a712496b587a306249c941ff30beb5d548df6";
const SOURCE_SHA256: &str = "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0";

#[derive(Debug, PartialEq, Serialize)]
struct PitchFrame {
    time: f64,
    /// `None` serializes as JSON null and preserves an invalid model output.
    /// FCPE supplies no voicing probability, so this evidence must not invent
    /// a voiced/unvoiced classification.
    hz: Option<f32>,
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
    sample_rate: u32,
    window_samples: u32,
    window_hop_samples: u32,
    frames: Vec<PitchFrame>,
}

fn model_dir(config: &serde_json::Value) -> Result<PathBuf, String> {
    let directory = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "FCPE requires Runtime Manager-resolved config.model_path".to_string())?;
    if !directory.is_dir() {
        return Err("resolved FCPE model generation is unavailable".to_string());
    }
    let manifest = directory.join("manifest.json");
    let xml = directory.join("fcpe.xml");
    let bin = directory.join("fcpe.bin");
    if !manifest.is_file() || !xml.is_file() || !bin.is_file() {
        return Err("FCPE OpenVINO IR files are unavailable".to_string());
    }
    Ok(directory)
}

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<PathBuf, String> {
    if audio.is_empty() {
        return Err("FCPE requires non-empty decoded audio".to_string());
    }
    let runtime_manifest = runtime::validate_runtime()?;
    let directory = model_dir(config)?;
    let device = runtime::inference_device(config)?;
    let openvino_device = device.openvino();
    let mut core = Core::new().map_err(|error| error.to_string())?;
    runtime::configure_inference_core(&mut core, device)?;
    let graph = core
        .read_model_from_file(
            directory.join("fcpe.xml").to_string_lossy().as_ref(),
            directory.join("fcpe.bin").to_string_lossy().as_ref(),
        )
        .map_err(|error| format!("could not load FCPE IR: {error}"))?;
    let mut compiled = core
        .compile_model(&graph, openvino_device)
        .map_err(|error| format!("could not compile FCPE for {}: {error}", device.label()))?;
    let mut request = compiled
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    let shape = Shape::new(&[1, INPUT_SAMPLES as i64, 1]).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    let window_count = audio.len().div_ceil(INPUT_SAMPLES);
    let mut frames = Vec::with_capacity(audio.len() / FRAME_HOP_SAMPLES + 1);
    for window_index in 0..window_count {
        let window_start = window_index * INPUT_SAMPLES;
        let window_end = (window_start + INPUT_SAMPLES).min(audio.len());
        let input = tensor
            .get_data_mut::<f32>()
            .map_err(|error| error.to_string())?;
        input.fill(0.0);
        input[..window_end - window_start].copy_from_slice(&audio[window_start..window_end]);
        request
            .set_input_tensor(&tensor)
            .map_err(|error| error.to_string())?;
        request
            .infer()
            .map_err(|error| format!("FCPE {} inference failed: {error}", device.label()))?;
        let output = request
            .get_output_tensor()
            .map_err(|error| error.to_string())?;
        let data = output
            .get_data::<f32>()
            .map_err(|error| error.to_string())?;
        if data.len() < OUTPUT_FRAMES {
            return Err("FCPE output has fewer frames than its pinned tensor contract".to_string());
        }
        append_owned_frames(&mut frames, data, window_index, audio.len());
        progress(
            (window_index + 1) as f32 / window_count as f32,
            "Running FCPE pitch windows",
        );
    }
    if frames.is_empty() {
        return Err("FCPE produced no source-bounded pitch frames".to_string());
    }

    let destination = output_dir.join("fcpe-pitch-evidence.json");
    let temporary = output_dir.join("fcpe-pitch-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &PitchEvidence {
            schema_version: 3,
            model_id: "fcpe",
            source_model_sha256: SOURCE_SHA256,
            model_manifest_sha256: MANIFEST_SHA256,
            model_xml_sha256: XML_SHA256,
            model_bin_sha256: BIN_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: device.evidence_backend(),
            timeline_step_ms: 10,
            sample_rate: SAMPLE_RATE as u32,
            window_samples: INPUT_SAMPLES as u32,
            window_hop_samples: INPUT_SAMPLES as u32,
            frames,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}

fn append_owned_frames(
    frames: &mut Vec<PitchFrame>,
    data: &[f32],
    window_index: usize,
    source_samples: usize,
) {
    let window_start = window_index * INPUT_SAMPLES;
    for (local_index, value) in data.iter().take(OUTPUT_FRAMES).enumerate() {
        // The first frame of every later window duplicates the preceding
        // window's endpoint. The earlier window owns that exact boundary.
        if window_index > 0 && local_index == 0 {
            continue;
        }
        let source_sample = window_start + local_index * FRAME_HOP_SAMPLES;
        if source_sample > source_samples {
            break;
        }
        frames.push(pitch_frame(source_sample, *value));
    }
}

fn pitch_frame(source_sample: usize, value: f32) -> PitchFrame {
    let hz = (value.is_finite() && value > 0.0).then_some(value);
    PitchFrame {
        time: source_sample as f64 / SAMPLE_RATE as f64,
        hz,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_and_unvoiced_outputs_never_fabricate_pitch() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 0.0] {
            let frame = pitch_frame(480, value);
            assert_eq!(frame.hz, None);
            assert_eq!(frame.time, 0.03);
        }
        let voiced = pitch_frame(640, 440.25);
        assert_eq!(voiced.hz, Some(440.25));
    }

    #[test]
    fn window_stitching_has_no_duplicate_or_out_of_range_frames() {
        let data = vec![440.0; OUTPUT_FRAMES];
        let mut frames = Vec::new();
        append_owned_frames(&mut frames, &data, 0, INPUT_SAMPLES * 2 + 80);
        append_owned_frames(&mut frames, &data, 1, INPUT_SAMPLES * 2 + 80);
        append_owned_frames(&mut frames, &data, 2, INPUT_SAMPLES * 2 + 80);
        assert_eq!(frames.len(), OUTPUT_FRAMES * 2 - 1);
        assert!(frames.windows(2).all(|pair| pair[0].time < pair[1].time));
        assert_eq!(frames.last().unwrap().time, 4.0);
        assert!(
            frames.last().unwrap().time <= (INPUT_SAMPLES * 2 + 80) as f64 / SAMPLE_RATE as f64
        );
    }
}
