#![allow(clippy::needless_range_loop)] // Tensor and DSP axes are deliberately explicit.

use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, ElementType, PartialShape, Shape, Tensor};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;

const MODEL_ID: &str = "bs_polarformer_public_instrumental";
const CHANNELS: usize = 2;
const SAMPLE_RATE: usize = 44_100;
// Use a 10-second product window: 50% more context than the low-memory
// schedule while staying below the sharp latency and VRAM cliff observed at
// the 588_800- and 882_000-sample upstream windows. Compile this exact static
// shape so the GPU plugin does not reserve for the larger dynamic time bound.
const CHUNK_SAMPLES: usize = 441_000;
const OVERLAP: usize = 2;
const CHUNK_STEP: usize = CHUNK_SAMPLES / OVERLAP;
const FFT_SIZE: usize = 2_048;
const HOP_SIZE: usize = 512;
const FREQUENCIES: usize = FFT_SIZE / 2 + 1;
const MODEL_FREQUENCIES: usize = FREQUENCIES * CHANNELS;
const FEATURE_WIDTH: usize = MODEL_FREQUENCIES * 2;
const MAX_SAMPLES: usize = SAMPLE_RATE * 60 * 60;

fn periodic_hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / size as f32).cos())
        .collect()
}

fn reflect_index(index: usize, samples: usize, pad: usize) -> usize {
    if samples == 1 {
        return 0;
    }
    let relative = index as isize - pad as isize;
    let period = 2 * (samples - 1) as isize;
    let folded = relative.rem_euclid(period);
    if folded < samples as isize {
        folded as usize
    } else {
        (period - folded) as usize
    }
}

fn stft(audio: &[Vec<f32>; CHANNELS]) -> Result<([Vec<Complex32>; CHANNELS], usize), String> {
    let samples = audio[0].len();
    if samples == 0 || audio[1].len() != samples {
        return Err("PolarFormer STFT input is empty or malformed".to_string());
    }
    let frames = samples / HOP_SIZE + 1;
    let pad = FFT_SIZE / 2;
    let window = periodic_hann(FFT_SIZE);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut result = [
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
    ];
    for channel in 0..CHANNELS {
        let mut buffer = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for frame in 0..frames {
            let offset = frame * HOP_SIZE;
            for index in 0..FFT_SIZE {
                let source = reflect_index(offset + index, samples, pad);
                buffer[index] = Complex32::new(audio[channel][source] * window[index], 0.0);
            }
            fft.process(&mut buffer);
            for frequency in 0..FREQUENCIES {
                result[channel][frame * FREQUENCIES + frequency] = buffer[frequency];
            }
        }
    }
    Ok((result, frames))
}

fn istft(
    spectrum: &[Vec<Complex32>; CHANNELS],
    frames: usize,
    samples: usize,
) -> Result<[Vec<f32>; CHANNELS], String> {
    if frames == 0
        || samples == 0
        || spectrum
            .iter()
            .any(|channel| channel.len() != FREQUENCIES * frames)
    {
        return Err("PolarFormer iSTFT spectrum contract mismatch".to_string());
    }
    let window = periodic_hann(FFT_SIZE);
    let padded_samples = (frames - 1) * HOP_SIZE + FFT_SIZE;
    let mut envelope = vec![0.0_f32; padded_samples];
    for frame in 0..frames {
        let offset = frame * HOP_SIZE;
        for index in 0..FFT_SIZE {
            envelope[offset + index] += window[index] * window[index];
        }
    }
    let mut planner = FftPlanner::<f32>::new();
    let inverse = planner.plan_fft_inverse(FFT_SIZE);
    let mut result = [vec![0.0_f32; padded_samples], vec![0.0_f32; padded_samples]];
    for channel in 0..CHANNELS {
        let mut buffer = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for frame in 0..frames {
            for frequency in 0..FREQUENCIES {
                buffer[frequency] = spectrum[channel][frame * FREQUENCIES + frequency];
            }
            for frequency in FREQUENCIES..FFT_SIZE {
                buffer[frequency] = buffer[FFT_SIZE - frequency].conj();
            }
            inverse.process(&mut buffer);
            let offset = frame * HOP_SIZE;
            for index in 0..FFT_SIZE {
                result[channel][offset + index] +=
                    buffer[index].re * window[index] / FFT_SIZE as f32;
            }
        }
    }
    let trim = FFT_SIZE / 2;
    Ok(std::array::from_fn(|channel| {
        (0..samples)
            .map(|index| {
                let padded_index = trim + index;
                result[channel][padded_index] / envelope[padded_index].max(1.0e-11)
            })
            .collect()
    }))
}

fn compile_model(
    model_path: &Path,
    device: crate::runtime::InferenceDevice,
) -> Result<CompiledModel, String> {
    if !model_path.is_file() {
        return Err("PolarFormer ONNX model is unavailable".to_string());
    }
    let _ = crate::runtime::validate_runtime()?;
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    crate::runtime::configure_inference_core(&mut core, device)?;
    let mut graph = core
        .read_model_from_file(
            model_path
                .to_str()
                .ok_or_else(|| "PolarFormer ONNX path is not UTF-8".to_string())?,
            "",
        )
        .map_err(|error| format!("could not read PolarFormer ONNX: {error}"))?;
    if graph.get_inputs_len().map_err(|error| error.to_string())? != 1
        || graph.get_outputs_len().map_err(|error| error.to_string())? != 1
    {
        return Err("PolarFormer ONNX must expose exactly one input and one output".to_string());
    }
    // The source ONNX exposes a bounded dynamic time axis. Intel's GPU plugin
    // otherwise reserves an activation workspace near that axis' maximum even
    // though every product chunk has one known size. Compile the exact shape so
    // attention memory follows the selected training window instead.
    let frames = CHUNK_SAMPLES / HOP_SIZE + 1;
    let input_shape = PartialShape::new_static(3, &[1, frames as i64, FEATURE_WIDTH as i64])
        .map_err(|error| format!("could not describe PolarFormer input shape: {error}"))?;
    graph
        .reshape_single_input(&input_shape)
        .map_err(|error| format!("could not fix PolarFormer input shape: {error}"))?;
    core.compile_model(&graph, device.openvino())
        .map_err(|error| {
            format!(
                "could not compile PolarFormer on {}: {error}",
                device.label()
            )
        })
}

fn infer_mask(
    model: &mut CompiledModel,
    spectrum: &[Vec<Complex32>; CHANNELS],
    frames: usize,
) -> Result<Vec<f32>, String> {
    let mut features = vec![0.0_f32; frames * FEATURE_WIDTH];
    for frame in 0..frames {
        for frequency in 0..FREQUENCIES {
            for channel in 0..CHANNELS {
                let value = spectrum[channel][frame * FREQUENCIES + frequency];
                let model_frequency = frequency * CHANNELS + channel;
                let offset = (frame * MODEL_FREQUENCIES + model_frequency) * 2;
                features[offset] = value.re;
                features[offset + 1] = value.im;
            }
        }
    }
    let shape =
        Shape::new(&[1, frames as i64, FEATURE_WIDTH as i64]).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(&features);
    let mut request = model
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    request
        .set_input_tensor(&tensor)
        .map_err(|error| error.to_string())?;
    request
        .infer()
        .map_err(|error| format!("PolarFormer inference failed: {error}"))?;
    let output = request
        .get_output_tensor()
        .map_err(|error| error.to_string())?;
    let dimensions = output
        .get_shape()
        .map_err(|error| error.to_string())?
        .get_dimensions()
        .to_vec();
    let expected = vec![1, 1, MODEL_FREQUENCIES as i64, frames as i64, 2];
    if dimensions != expected {
        return Err(format!(
            "PolarFormer returned unexpected mask shape {dimensions:?}; expected {expected:?}"
        ));
    }
    let mask = output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if mask.iter().any(|value| !value.is_finite()) {
        return Err("PolarFormer returned non-finite mask values".to_string());
    }
    Ok(mask)
}

fn apply_mask(
    spectrum: &[Vec<Complex32>; CHANNELS],
    mask: &[f32],
    frames: usize,
) -> Result<[Vec<Complex32>; CHANNELS], String> {
    if mask.len() != MODEL_FREQUENCIES * frames * 2 {
        return Err("PolarFormer mask length is invalid".to_string());
    }
    let mut masked = [
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
    ];
    for frequency in 1..FREQUENCIES {
        for channel in 0..CHANNELS {
            let model_frequency = frequency * CHANNELS + channel;
            for frame in 0..frames {
                let offset = (model_frequency * frames + frame) * 2;
                let value = Complex32::new(mask[offset], mask[offset + 1]);
                masked[channel][frame * FREQUENCIES + frequency] =
                    spectrum[channel][frame * FREQUENCIES + frequency] * value;
            }
        }
    }
    Ok(masked)
}

fn process_audio(
    audio: &[Vec<f32>; CHANNELS],
    model: &mut CompiledModel,
    vocal_mode: bool,
    mut progress: impl FnMut(f32, &str),
) -> Result<[Vec<f32>; CHANNELS], String> {
    let samples = audio[0].len();
    if samples == 0 || samples > MAX_SAMPLES || audio[1].len() != samples {
        return Err("PolarFormer input is empty, malformed, or exceeds one hour".to_string());
    }
    let chunks = samples.div_ceil(CHUNK_STEP);
    let mut vocals = [vec![0.0_f32; samples], vec![0.0_f32; samples]];
    let mut counts = vec![0_u16; samples];
    let message = if vocal_mode {
        "Running native PolarFormer vocal separation"
    } else {
        "Running native PolarFormer instrumental separation"
    };
    for chunk_index in 0..chunks {
        let start = chunk_index * CHUNK_STEP;
        let end = (start + CHUNK_SAMPLES).min(samples);
        let valid = end - start;
        let chunk = std::array::from_fn(|channel| {
            let mut values = vec![0.0_f32; CHUNK_SAMPLES];
            values[..valid].copy_from_slice(&audio[channel][start..end]);
            values
        });
        progress(chunk_index as f32 / chunks as f32, message);
        let (spectrum, frames) = stft(&chunk)?;
        let mask = infer_mask(model, &spectrum, frames)?;
        let separated = istft(
            &apply_mask(&spectrum, &mask, frames)?,
            frames,
            CHUNK_SAMPLES,
        )?;
        for index in 0..valid {
            for channel in 0..CHANNELS {
                vocals[channel][start + index] += separated[channel][index];
            }
            counts[start + index] = counts[start + index].saturating_add(1);
        }
    }
    // The checkpoint's single trained stem is vocals (config.yaml's
    // `training.target_instrument: vocals`), not instrumental. Instrumental
    // is therefore mix-minus-vocals; the raw stem itself is a real, clean
    // vocal estimate in its own right when that role is what was requested.
    Ok(std::array::from_fn(|channel| {
        (0..samples)
            .map(|index| {
                let vocal = vocals[channel][index] / f32::from(counts[index].max(1));
                if vocal_mode {
                    vocal
                } else {
                    audio[channel][index] - vocal
                }
            })
            .collect()
    }))
}

pub fn infer(
    interleaved: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    let vocal_mode = match config
        .get("semantic_output")
        .and_then(serde_json::Value::as_str)
    {
        Some("instrumental") => false,
        Some("guide_vocals") => true,
        _ => {
            return Err(format!(
                "{MODEL_ID} PolarFormer requires explicit Instrumental or GuideVocals semantics"
            ));
        }
    };
    if interleaved.is_empty() || !interleaved.len().is_multiple_of(CHANNELS) {
        return Err("PolarFormer stereo PCM is empty or malformed".to_string());
    }
    let model_path = config
        .get("model_path")
        .and_then(serde_json::Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| "PolarFormer model path is missing".to_string())?;
    let samples = interleaved.len() / CHANNELS;
    let audio = std::array::from_fn(|channel| {
        (0..samples)
            .map(|frame| interleaved[frame * CHANNELS + channel])
            .collect::<Vec<_>>()
    });
    let device = crate::runtime::inference_device(config)?;
    let compiling = format!(
        "Compiling FP16-weight PolarFormer on {} with FP32 compute",
        device.label()
    );
    progress(0.01, &compiling);
    let mut model = compile_model(&model_path, device)?;
    let separated = process_audio(&audio, &mut model, vocal_mode, |fraction, message| {
        progress(0.03 + fraction * 0.94, message)
    })?;
    if separated.iter().flatten().any(|sample| !sample.is_finite()) {
        return Err(format!(
            "PolarFormer returned non-finite {} audio",
            if vocal_mode {
                "GuideVocals"
            } else {
                "Instrumental"
            }
        ));
    }
    let mut output = Vec::with_capacity(interleaved.len());
    for frame in 0..samples {
        output.push(separated[0][frame]);
        output.push(separated[1][frame]);
    }
    let (label, filename) = if vocal_mode {
        ("GuideVocals", "polarformer-guide-vocals.flac")
    } else {
        ("Instrumental", "polarformer-instrumental.flac")
    };
    progress(
        0.98,
        &format!("Atomically encoding lossless PolarFormer {label}"),
    );
    crate::audio::encode_stereo_flac(&output, output_dir, filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stft_round_trip_preserves_finite_stereo_audio() {
        let samples = 16_384;
        let audio = std::array::from_fn(|channel| {
            (0..samples)
                .map(|index| {
                    let phase = 2.0
                        * std::f32::consts::PI
                        * (220.0 + 220.0 * channel as f32)
                        * index as f32
                        / SAMPLE_RATE as f32;
                    phase.sin() * 0.25
                })
                .collect::<Vec<_>>()
        });
        let (spectrum, frames) = stft(&audio).unwrap();
        let reconstructed = istft(&spectrum, frames, samples).unwrap();
        let maximum_error = audio
            .iter()
            .flatten()
            .zip(reconstructed.iter().flatten())
            .map(|(expected, actual)| (expected - actual).abs())
            .fold(0.0_f32, f32::max);
        assert!(maximum_error < 1.0e-4, "maximum error {maximum_error}");
    }

    #[test]
    fn mask_layout_preserves_channel_frequency_and_frame_axes() {
        let frames = 3;
        let mut spectrum = [
            vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
            vec![Complex32::new(0.0, 0.0); FREQUENCIES * frames],
        ];
        spectrum[1][2 * FREQUENCIES + 7] = Complex32::new(2.0, 3.0);
        let mut mask = vec![0.0; MODEL_FREQUENCIES * frames * 2];
        let model_frequency = 7 * CHANNELS + 1;
        let offset = (model_frequency * frames + 2) * 2;
        mask[offset] = 4.0;
        mask[offset + 1] = 5.0;
        let masked = apply_mask(&spectrum, &mask, frames).unwrap();
        assert_eq!(masked[1][2 * FREQUENCIES + 7], Complex32::new(-7.0, 22.0));
    }

    #[test]
    fn production_chunk_schedule_covers_every_sample() {
        for samples in [1, CHUNK_STEP, CHUNK_STEP + 1, CHUNK_SAMPLES + 17] {
            let chunks = samples.div_ceil(CHUNK_STEP);
            let mut counts = vec![0_u8; samples];
            for chunk in 0..chunks {
                let start = chunk * CHUNK_STEP;
                let end = (start + CHUNK_SAMPLES).min(samples);
                for count in &mut counts[start..end] {
                    *count += 1;
                }
            }
            assert!(counts.iter().all(|count| matches!(count, 1 | 2)));
        }
    }

    #[test]
    fn semantic_output_is_not_inferred_from_the_model_name() {
        let error = infer(
            &[0.0, 0.0],
            Path::new("."),
            &serde_json::json!({"semantic_output": "vocals"}),
            |_, _| {},
        )
        .unwrap_err();
        assert!(error.contains("Instrumental or GuideVocals semantics"));
        assert_eq!(MODEL_ID, "bs_polarformer_public_instrumental");
        assert_eq!(OVERLAP, 2);
    }
}
