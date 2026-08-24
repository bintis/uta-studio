use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::Serialize;

use crate::runtime;

const INPUT_SAMPLES: usize = 43_844;
const FFT_HOP_SAMPLES: usize = 256;
const OVERLAP_FRAMES: usize = 30;
const HALF_OVERLAP_FRAMES: usize = OVERLAP_FRAMES / 2;
const OVERLAP_SAMPLES: usize = OVERLAP_FRAMES * FFT_HOP_SAMPLES;
const PADDING_SAMPLES: usize = OVERLAP_SAMPLES / 2;
const WINDOW_HOP_SAMPLES: usize = INPUT_SAMPLES - OVERLAP_SAMPLES;
const FRAMES_PER_WINDOW: usize = 172;
const OWNED_FRAMES_PER_WINDOW: usize = FRAMES_PER_WINDOW - OVERLAP_FRAMES;
const XML_SHA256: &str = "9df134bf18c66dde7b678be49329299ff6ca13be465f3df5b10ff38a75e5aa34";
const BIN_SHA256: &str = "50856c2bac689bb6fdc43ae21818e2a63c37f35207dc5adea22d52fc601efab3";
const MANIFEST_SHA256: &str = "01b35925daaeb40995f4e49b495e6f1ce9db47c7f41987b19fdc1b5c35f2c1b7";
const SOURCE_SHA256: &str = "2c3c1d144bfa61ad236e92e169c13535c880469a12a047d4e73451f2c059a0ec";

#[derive(Serialize, Debug, PartialEq)]
struct ActivationFrame {
    time: f64,
    note_max: f32,
    onset_max: f32,
    contour_class: usize,
    /// Source-local contour activation; it is not calibrated confidence.
    contour_score: f32,
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
    window_samples: u32,
    window_hop_samples: u32,
    fft_hop_samples: u32,
    overlap_frames: usize,
    padding_samples: u32,
    frames_per_window: usize,
    owned_frames_per_window: usize,
    frames: Vec<ActivationFrame>,
}

fn model_dir(config: &serde_json::Value) -> Result<PathBuf, String> {
    let directory = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| {
            "Basic Pitch requires Runtime Manager-resolved config.model_path".to_string()
        })?;
    if !directory.is_dir() {
        return Err("resolved Basic Pitch model generation is unavailable".to_string());
    }
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

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<PathBuf, String> {
    if audio.is_empty() {
        return Err("Basic Pitch requires non-empty decoded audio".to_string());
    }
    let runtime_manifest = runtime::validate_runtime()?;
    let directory = model_dir(config)?;
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
    let window_count = window_count(audio.len());
    let mut frames = Vec::new();
    let mut frames_per_window = None;
    for window_index in 0..window_count {
        let input = tensor
            .get_data_mut::<f32>()
            .map_err(|error| error.to_string())?;
        fill_padded_window(input, audio, window_index);
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
        if frame_count != FRAMES_PER_WINDOW
            || onsets.len() != frame_count * 88
            || contours.len() != frame_count * 264
            || frames_per_window
                .replace(frame_count)
                .is_some_and(|old| old != frame_count)
        {
            return Err("Basic Pitch returned unexpected output shapes".to_string());
        }
        if !activation_outputs_are_finite(notes, onsets, contours) {
            return Err("Basic Pitch returned non-finite activation evidence".to_string());
        }
        append_window_frames(
            &mut frames,
            notes,
            onsets,
            contours,
            frame_count,
            window_index,
            audio.len(),
        );
        progress(
            (window_index + 1) as f32 / window_count as f32,
            "Running Basic Pitch activation windows",
        );
    }
    let destination = output_dir.join("basic-pitch-activation-evidence.json");
    let temporary = output_dir.join("basic-pitch-activation-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &BasicPitchEvidence {
            schema_version: 3,
            model_id: "basic_pitch",
            source_model_sha256: SOURCE_SHA256,
            model_manifest_sha256: MANIFEST_SHA256,
            model_xml_sha256: XML_SHA256,
            model_bin_sha256: BIN_SHA256,
            runtime_manifest_sha256: &runtime_manifest,
            backend: "openvino_gpu",
            sample_rate: 22_050,
            window_samples: INPUT_SAMPLES as u32,
            window_hop_samples: WINDOW_HOP_SAMPLES as u32,
            fft_hop_samples: FFT_HOP_SAMPLES as u32,
            overlap_frames: OVERLAP_FRAMES,
            padding_samples: PADDING_SAMPLES as u32,
            frames_per_window: frames_per_window.unwrap_or(0),
            owned_frames_per_window: OWNED_FRAMES_PER_WINDOW,
            frames,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    Ok(destination)
}

fn window_count(source_samples: usize) -> usize {
    let padded_samples = source_samples + 2 * PADDING_SAMPLES;
    if padded_samples <= INPUT_SAMPLES {
        1
    } else {
        (padded_samples - INPUT_SAMPLES).div_ceil(WINDOW_HOP_SAMPLES) + 1
    }
}

fn fill_padded_window(input: &mut [f32], audio: &[f32], window_index: usize) {
    input.fill(0.0);
    let padded_start = window_index * WINDOW_HOP_SAMPLES;
    for (local, value) in input.iter_mut().enumerate() {
        let padded_sample = padded_start + local;
        if let Some(source_sample) = padded_sample.checked_sub(PADDING_SAMPLES)
            && let Some(source) = audio.get(source_sample)
        {
            *value = *source;
        }
    }
}

fn append_window_frames(
    frames: &mut Vec<ActivationFrame>,
    notes: &[f32],
    onsets: &[f32],
    contours: &[f32],
    frame_count: usize,
    window_index: usize,
    source_samples: usize,
) {
    debug_assert_eq!(frame_count, FRAMES_PER_WINDOW);
    let target_frames = source_samples / FFT_HOP_SAMPLES;
    for frame in HALF_OVERLAP_FRAMES..frame_count - HALF_OVERLAP_FRAMES {
        let source_frame = window_index * OWNED_FRAMES_PER_WINDOW + frame - HALF_OVERLAP_FRAMES;
        if source_frame >= target_frames {
            break;
        }
        let source_sample = source_frame * FFT_HOP_SAMPLES;
        let contour = &contours[frame * 264..(frame + 1) * 264];
        let (contour_class, contour_score) = contour
            .iter()
            .copied()
            .enumerate()
            .max_by(|left, right| left.1.total_cmp(&right.1))
            .unwrap_or((0, 0.0));
        frames.push(ActivationFrame {
            time: source_sample as f64 / 22_050.0,
            note_max: maximum(&notes[frame * 88..(frame + 1) * 88]),
            onset_max: maximum(&onsets[frame * 88..(frame + 1) * 88]),
            contour_class,
            contour_score,
        });
    }
}

fn activation_outputs_are_finite(notes: &[f32], onsets: &[f32], contours: &[f32]) -> bool {
    notes
        .iter()
        .chain(onsets)
        .chain(contours)
        .all(|value| value.is_finite())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activation_evidence_fails_closed_on_non_finite_values() {
        assert!(activation_outputs_are_finite(&[0.1, 0.2], &[0.3], &[0.4]));
        for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!activation_outputs_are_finite(&[0.1], &[0.2], &[invalid]));
        }
    }

    #[test]
    fn window_stitching_uses_reference_overlap_grid_and_clips_tail() {
        let source_samples = INPUT_SAMPLES + WINDOW_HOP_SAMPLES / 2;
        assert_eq!(window_count(source_samples), 2);
        let notes = vec![0.2; FRAMES_PER_WINDOW * 88];
        let onsets = vec![0.3; FRAMES_PER_WINDOW * 88];
        let contours = vec![0.4; FRAMES_PER_WINDOW * 264];
        let mut frames = Vec::new();
        for window in 0..window_count(source_samples) {
            append_window_frames(
                &mut frames,
                &notes,
                &onsets,
                &contours,
                FRAMES_PER_WINDOW,
                window,
                source_samples,
            );
        }
        assert_eq!(frames.len(), source_samples / FFT_HOP_SAMPLES);
        assert!(frames.windows(2).all(|pair| pair[0].time < pair[1].time));
        for (index, frame) in frames.iter().enumerate() {
            assert_eq!(frame.time, (index * FFT_HOP_SAMPLES) as f64 / 22_050.0);
        }
        assert!(frames.last().unwrap().time < source_samples as f64 / 22_050.0);
        assert_eq!(frames[0].contour_score, 0.4);
    }

    #[test]
    fn first_window_has_reference_half_overlap_padding() {
        let audio = (1..=8).map(|value| value as f32).collect::<Vec<_>>();
        let mut input = vec![-1.0; INPUT_SAMPLES];
        fill_padded_window(&mut input, &audio, 0);
        assert!(input[..PADDING_SAMPLES].iter().all(|value| *value == 0.0));
        assert_eq!(
            &input[PADDING_SAMPLES..PADDING_SAMPLES + audio.len()],
            &audio
        );
        assert!(
            input[PADDING_SAMPLES + audio.len()..]
                .iter()
                .all(|value| *value == 0.0)
        );
    }
}
