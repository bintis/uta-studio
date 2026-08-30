use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use openvino::{Core, ElementType, Shape, Tensor};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use serde::Serialize;

const SAMPLE_RATE: usize = 44_100;
const HOP: usize = 1_024;
const BINS: usize = 384;
const CHANNELS: usize = 6;
const ONSET_THRESHOLD: f32 = 0.32;
const OFFSET_THRESHOLD: f32 = 0.70;
const MIDI_FMIN: f32 = 24.0;

#[derive(Serialize)]
struct Range {
    start: u64,
    end: u64,
}

#[derive(Serialize)]
struct Note {
    range: Range,
    midi: u8,
    onset_score: Option<f32>,
    offset_score: Option<f32>,
    pitch_score: Option<f32>,
}

#[derive(Serialize)]
struct Evidence<'a> {
    schema_version: u32,
    model_id: &'a str,
    upstream_revision: &'a str,
    checkpoint_identity: &'a str,
    config_identity: &'a str,
    conversion_identity: &'a str,
    model_generation: &'a str,
    runtime_identity: &'a str,
    backend: &'a str,
    source_start: u64,
    source_duration: u64,
    mix_audio_identity: &'a str,
    vocal_audio_identity: &'a str,
    separator_model_generation: &'a str,
    vocal_preparation_generation: &'a str,
    frontend_profile: &'a str,
    decode_profile: &'a str,
    onset_threshold: f32,
    offset_threshold: f32,
    notes: Vec<Note>,
}

fn config_text<'a>(config: &'a serde_json::Value, key: &str, fallback: &'a str) -> &'a str {
    config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback)
}

fn model_path(config: &serde_json::Value) -> Result<PathBuf, String> {
    let path = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "JBM555 requires Runtime Manager-resolved config.model_path".to_string())?;
    let path = if path.is_dir() {
        path.join("jbm555-cectc80.onnx")
    } else {
        path
    };
    path.is_file()
        .then_some(path)
        .ok_or_else(|| "JBM555 ONNX model is unavailable".to_string())
}

fn reflected(audio: &[f32], index: isize) -> f32 {
    if audio.len() <= 1 {
        return audio.first().copied().unwrap_or(0.0);
    }
    let period = 2 * (audio.len() - 1) as isize;
    let folded = index.rem_euclid(period);
    audio[if folded < audio.len() as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }]
}

/// Native bounded spectral approximation of the upstream three-scale CQT.
/// It preserves the published frequency grid, hop, channel order, and scales;
/// the profile name makes the native FFT interpolation explicit.
fn append_signal_features(audio: &[f32], frames: usize, output: &mut [f32], base_channel: usize) {
    let fft_sizes = [8_192_usize, 16_384, 32_768];
    let mut planner = FftPlanner::<f32>::new();
    for (scale_index, fft_size) in fft_sizes.into_iter().enumerate() {
        let fft = planner.plan_fft_forward(fft_size);
        let mut buffer = vec![Complex32::new(0.0, 0.0); fft_size];
        for frame in 0..frames {
            let center = frame * HOP;
            for (index, value) in buffer.iter_mut().enumerate() {
                let source = center as isize + index as isize - (fft_size / 2) as isize;
                let window =
                    0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / fft_size as f32).cos();
                *value = Complex32::new(reflected(audio, source) * window, 0.0);
            }
            fft.process(&mut buffer);
            for bin in 0..BINS {
                let midi = MIDI_FMIN + bin as f32 / 4.0;
                let hz = 440.0 * 2.0_f32.powf((midi - 69.0) / 12.0);
                let exact = hz * fft_size as f32 / SAMPLE_RATE as f32;
                let lower = exact.floor() as usize;
                let fraction = exact - lower as f32;
                let left = buffer[lower.min(fft_size / 2)].norm();
                let right = buffer[(lower + 1).min(fft_size / 2)].norm();
                let magnitude = (left + (right - left) * fraction).ln_1p();
                let channel = base_channel + scale_index;
                output[((channel * frames + frame) * BINS) + bin] = magnitude;
            }
        }
    }
}

fn softmax(values: &[f32]) -> Vec<f32> {
    let maximum = values.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    let mut result = values
        .iter()
        .map(|value| (*value - maximum).exp())
        .collect::<Vec<_>>();
    let total = result.iter().sum::<f32>().max(f32::MIN_POSITIVE);
    result.iter_mut().for_each(|value| *value /= total);
    result
}

fn decode_notes(on_off: &[f32], octave: &[f32], class: &[f32], frames: usize) -> Vec<Note> {
    let onset = (0..frames)
        .map(|frame| on_off[frame * 4 + 1])
        .collect::<Vec<_>>();
    let mut notes = Vec::new();
    let mut active: Option<(usize, f32)> = None;
    let mut pitches = Vec::<(u8, f32)>::new();
    let finish = |end: usize,
                  active: &mut Option<(usize, f32)>,
                  pitches: &mut Vec<(u8, f32)>,
                  notes: &mut Vec<Note>,
                  offset_score: Option<f32>| {
        let Some((start, onset_score)) = active.take() else {
            return;
        };
        if pitches.is_empty() || end <= start {
            pitches.clear();
            return;
        }
        let mut counts = BTreeMap::<u8, (usize, f32)>::new();
        for (midi, score) in pitches.drain(..) {
            let entry = counts.entry(midi).or_default();
            entry.0 += 1;
            entry.1 += score;
        }
        let (midi, (count, score)) = counts
            .into_iter()
            .max_by(|left, right| {
                left.1
                    .0
                    .cmp(&right.1.0)
                    .then_with(|| left.1.1.total_cmp(&right.1.1))
            })
            .unwrap();
        notes.push(Note {
            range: Range {
                start: start as u64 * HOP as u64 * 1_000_000 / SAMPLE_RATE as u64,
                end: end as u64 * HOP as u64 * 1_000_000 / SAMPLE_RATE as u64,
            },
            midi,
            onset_score: Some(onset_score),
            offset_score,
            pitch_score: Some(score / count as f32),
        });
    };
    for frame in 0..frames {
        let backward = frame.saturating_sub(3);
        let forward = (frame + 4).min(frames);
        let is_peak = onset[frame] >= ONSET_THRESHOLD
            && onset[backward..forward]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .is_some_and(|(index, _)| backward + index == frame);
        if is_peak {
            finish(frame, &mut active, &mut pitches, &mut notes, None);
            active = Some((frame, onset[frame]));
        } else if on_off[frame * 4 + 2] >= OFFSET_THRESHOLD {
            finish(
                frame,
                &mut active,
                &mut pitches,
                &mut notes,
                Some(on_off[frame * 4 + 2]),
            );
        }
        if active.is_some() {
            let octave_probs = softmax(&octave[frame * 5..frame * 5 + 5]);
            let class_probs = softmax(&class[frame * 13..frame * 13 + 13]);
            let oct = octave_probs[..4]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .unwrap_or(0);
            let chroma = class_probs[..12]
                .iter()
                .enumerate()
                .max_by(|left, right| left.1.total_cmp(right.1))
                .map(|(index, _)| index)
                .unwrap_or(0);
            pitches.push((
                (36 + oct * 12 + chroma) as u8,
                octave_probs[oct] * class_probs[chroma],
            ));
        }
    }
    finish(frames, &mut active, &mut pitches, &mut notes, None);
    notes
}

pub fn infer(
    mix: &[f32],
    vocal: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &'static str),
) -> Result<PathBuf, String> {
    if mix.is_empty() || vocal.is_empty() {
        return Err("JBM555 requires non-empty mix and prepared-vocal inputs".to_string());
    }
    let samples = mix.len().min(vocal.len());
    let frames = samples.div_ceil(HOP).max(1);
    let mut features = vec![0.0_f32; CHANNELS * frames * BINS];
    progress(0.05, "Building native dual-input JBM555 spectral features");
    append_signal_features(&mix[..samples], frames, &mut features, 0);
    append_signal_features(&vocal[..samples], frames, &mut features, 3);

    let runtime_identity = crate::runtime::validate_runtime()?;
    let device = crate::runtime::inference_device(config)?;
    let mut core = Core::new().map_err(|error| error.to_string())?;
    crate::runtime::configure_inference_core(&mut core, device)?;
    let path = model_path(config)?;
    let graph = core
        .read_model_from_file(path.to_string_lossy().as_ref(), "")
        .map_err(|error| format!("could not load JBM555 ONNX: {error}"))?;
    let mut compiled = core
        .compile_model(&graph, device.openvino())
        .map_err(|error| format!("could not compile JBM555 on {}: {error}", device.label()))?;
    let shape = Shape::new(&[1, CHANNELS as i64, frames as i64, BINS as i64])
        .map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&features);
    let mut request = compiled
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    request
        .set_input_tensor(&tensor)
        .map_err(|error| error.to_string())?;
    progress(0.55, "Running native JBM555 CE-CTC inference");
    request
        .infer()
        .map_err(|error| format!("JBM555 inference failed: {error}"))?;
    let on_off = request
        .get_output_tensor_by_index(0)
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let octave = request
        .get_output_tensor_by_index(1)
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    let class = request
        .get_output_tensor_by_index(2)
        .map_err(|error| error.to_string())?
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if on_off.len() != frames * 4
        || octave.len() != frames * 5
        || class.len() != frames * 13
        || on_off
            .iter()
            .chain(&octave)
            .chain(&class)
            .any(|value| !value.is_finite())
    {
        return Err("JBM555 returned malformed output tensors".to_string());
    }
    let mut notes = decode_notes(&on_off, &octave, &class, frames);
    let source_start = config
        .get("source_start")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0);
    for note in &mut notes {
        note.range.start = note.range.start.saturating_add(source_start);
        note.range.end = note.range.end.saturating_add(source_start);
    }
    let source_duration = config
        .get("source_duration")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(samples as u64 * 1_000_000 / SAMPLE_RATE as u64);
    let source_end = source_start.saturating_add(source_duration);
    for note in &mut notes {
        note.range.end = note.range.end.min(source_end);
    }
    notes.retain(|note| note.range.start < note.range.end && note.range.start < source_end);
    let destination = output_dir.join("jbm555-note-evidence.json");
    let temporary = output_dir.join("jbm555-note-evidence.json.tmp");
    let mut file = std::fs::File::create(&temporary).map_err(|error| error.to_string())?;
    serde_json::to_writer(
        &mut file,
        &Evidence {
            schema_version: 1,
            model_id: "jbm555_cectc_80",
            upstream_revision: config_text(config, "upstream_revision", "jbm555-public"),
            checkpoint_identity: config_text(config, "checkpoint_identity", "jbm555_80"),
            config_identity: config_text(config, "config_identity", "cectc80-public"),
            conversion_identity: config_text(config, "conversion_identity", "onnx-opset18"),
            model_generation: config_text(config, "model_generation", "runtime-managed"),
            runtime_identity: &runtime_identity,
            backend: device.evidence_backend(),
            source_start,
            source_duration,
            mix_audio_identity: config_text(config, "mix_audio_identity", "task-mix"),
            vocal_audio_identity: config_text(config, "vocal_audio_identity", "task-vocal"),
            separator_model_generation: config_text(
                config,
                "separator_model_generation",
                "leap-xe90",
            ),
            vocal_preparation_generation: config_text(
                config,
                "vocal_preparation_generation",
                "native-44k1",
            ),
            frontend_profile: "jbm555-native-logfft-44k1-hop1024-midi24-384x48-scales0.5-1-2-v1",
            decode_profile: "jbm555-cectc-onset0.32-offset0.70-v1",
            onset_threshold: ONSET_THRESHOLD,
            offset_threshold: OFFSET_THRESHOLD,
            notes,
        },
    )
    .map_err(|error| error.to_string())?;
    file.write_all(b"\n").map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &destination).map_err(|error| error.to_string())?;
    progress(1.0, "JBM555 note evidence complete");
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_decoder_preserves_repeated_same_pitch_attacks() {
        let frames = 12;
        let mut on_off = vec![0.0; frames * 4];
        let mut octave = vec![0.0; frames * 5];
        let mut class = vec![0.0; frames * 13];
        for frame in 0..frames {
            on_off[frame * 4 + 2] = 0.1;
            octave[frame * 5 + 2] = 4.0;
            class[frame * 13 + 9] = 4.0;
        }
        on_off[1 * 4 + 1] = 0.8;
        on_off[7 * 4 + 1] = 0.9;
        let notes = decode_notes(&on_off, &octave, &class, frames);
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].midi, notes[1].midi);
    }
}
