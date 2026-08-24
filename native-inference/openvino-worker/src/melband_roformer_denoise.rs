#![allow(clippy::needless_range_loop)] // DSP tensor axes are indexed explicitly.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use openvino::{Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use serde::Deserialize;

const MODEL_ID: &str = "melband_roformer_denoise_aufr33";
const DEREVERB_MODEL_ID: &str = "melband_roformer_dereverb_anvuew";
const HARMONY_MODEL_ID: &str = "melband_roformer_harmony";
#[cfg(test)]
const SAMPLE_RATE: usize = 44_100;
const CHANNELS: usize = 2;
const CHUNK_SAMPLES: usize = 352_800;
const FFT_SIZE: usize = 2_048;
const HOP_SIZE: usize = 441;
const FREQUENCIES: usize = FFT_SIZE / 2 + 1;
const PACKED_FREQUENCIES: usize = FREQUENCIES * CHANNELS;
const INFERENCE_FRAMES: usize = 801;
const GATHERED_FREQUENCIES: usize = 3_958;
const GATHERED_WIDTH: usize = GATHERED_FREQUENCIES * 2;

#[derive(Deserialize)]
struct ArtifactManifest {
    schema_version: u32,
    resource: String,
    capability: String,
    semantic_output: String,
    conversion_recipe: ConversionIdentity,
    io: IoContract,
    files: BTreeMap<String, FileIdentity>,
}

#[derive(Deserialize)]
struct ConversionIdentity {
    graph_boundary: String,
    dynamic_time_axis: bool,
    semantic_time_chunking: bool,
}

#[derive(Deserialize)]
struct IoContract {
    input: TensorContract,
    output: TensorContract,
}

#[derive(Deserialize)]
struct TensorContract {
    name: String,
    dtype: String,
    exact_validation_shape: Vec<usize>,
}

#[derive(Deserialize)]
struct FileIdentity {
    bytes: u64,
}

#[derive(Deserialize)]
struct Layout {
    schema_version: u32,
    model_id: String,
    frequency_indices: Vec<usize>,
    bands_per_frequency: Vec<usize>,
    frequencies_per_band: Vec<usize>,
}

struct ModelArtifact {
    xml: PathBuf,
    bin: PathBuf,
    overlap: usize,
    inference_frames: usize,
    chunk_samples: usize,
}

fn layout() -> Result<&'static Layout, String> {
    static LAYOUT: OnceLock<Result<Layout, String>> = OnceLock::new();
    LAYOUT
        .get_or_init(|| {
            let value: Layout =
                serde_json::from_str(include_str!("../melband-roformer-denoise-layout.json"))
                    .map_err(|error| format!("Denoise mel-band layout is invalid: {error}"))?;
            if value.schema_version != 1
                || value.model_id != MODEL_ID
                || value.frequency_indices.len() != GATHERED_FREQUENCIES
                || value.bands_per_frequency.len() != FREQUENCIES
                || value.frequencies_per_band.len() != 60
                || value
                    .frequency_indices
                    .iter()
                    .any(|index| *index >= PACKED_FREQUENCIES)
                || value
                    .bands_per_frequency
                    .iter()
                    .any(|count| !(1..=2).contains(count))
                || value.frequencies_per_band.iter().sum::<usize>() * CHANNELS
                    != GATHERED_FREQUENCIES
            {
                return Err("Denoise mel-band layout identity is invalid".to_string());
            }
            Ok(value)
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn model_artifact(model_id: &str, config: &serde_json::Value) -> Result<ModelArtifact, String> {
    let _ = crate::runtime::inference_device(config)?;
    let directory = config
        .get("model_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "MelBand OpenVINO generation path is missing".to_string())?;
    if !directory.is_dir() {
        return Err("MelBand OpenVINO generation is unavailable".to_string());
    }
    if model_id == HARMONY_MODEL_ID {
        if config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("lead_vocal+backing_vocal_residual")
        {
            return Err(
                "Lead-isolation worker requires explicit lead/residual semantics".to_string(),
            );
        }
        let manifest_path = directory.join("manifest.json");
        let config_path = directory.join("config.yaml");
        let xml = directory.join("melband-roformer-harmony-neural.xml");
        let bin = directory.join("melband-roformer-harmony-neural.bin");
        if !manifest_path.is_file() || !config_path.is_file() || !xml.is_file() || !bin.is_file() {
            return Err("Harmony OpenVINO generation is incomplete".to_string());
        }
        let manifest: ArtifactManifest = serde_json::from_slice(
            &std::fs::read(&manifest_path).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("Harmony artifact manifest is invalid: {error}"))?;
        let input_shape = [1, INFERENCE_FRAMES, GATHERED_WIDTH];
        let valid_tensor = |tensor: &TensorContract, name: &str| {
            tensor.name == name
                && tensor.dtype == "f32"
                && tensor.exact_validation_shape == input_shape
        };
        let declared = |name: &str, path: &Path| -> Result<bool, String> {
            let Some(file) = manifest.files.get(name) else {
                return Ok(false);
            };
            let metadata = path.metadata().map_err(|error| error.to_string())?;
            Ok(metadata.is_file() && metadata.len() == file.bytes)
        };
        if manifest.schema_version != 1
            || manifest.resource != "model:melband_roformer_harmony"
            || manifest.capability != "audio.lead_isolate"
            || manifest.semantic_output != "lead_vocal+backing_vocal_residual"
            || manifest.conversion_recipe.graph_boundary != "band_split+transformers+mask_estimator"
            || !manifest.conversion_recipe.dynamic_time_axis
            || manifest.conversion_recipe.semantic_time_chunking
            || !valid_tensor(&manifest.io.input, "gathered_stft")
            || !valid_tensor(&manifest.io.output, "gathered_mask")
            || !declared("config.yaml", &config_path)?
            || !declared("melband-roformer-harmony-neural.xml", &xml)?
            || !declared("melband-roformer-harmony-neural.bin", &bin)?
        {
            return Err("Harmony OpenVINO semantic or tensor contract is invalid".to_string());
        }
        let _ = layout()?;
        return Ok(ModelArtifact {
            xml,
            bin,
            overlap: 4,
            inference_frames: INFERENCE_FRAMES,
            chunk_samples: CHUNK_SAMPLES,
        });
    }
    if model_id == DEREVERB_MODEL_ID {
        if config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("noreverb")
        {
            return Err(
                "Dereverb worker requires the explicit noreverb semantic output".to_string(),
            );
        }
        let manifest = directory.join("manifest.json");
        let source_config = directory.join("config.yaml");
        let xml = directory.join("melband-roformer-dereverb-neural.xml");
        let bin = directory.join("melband-roformer-dereverb-neural.bin");
        if !manifest.is_file() || !source_config.is_file() || !xml.is_file() || !bin.is_file() {
            return Err("Dereverb OpenVINO generation is incomplete".to_string());
        }
        let manifest_text =
            std::fs::read_to_string(&manifest).map_err(|error| error.to_string())?;
        for required in ["audio.dereverb", "melband_roformer_dereverb_anvuew"] {
            if !manifest_text.contains(required) {
                return Err("Dereverb manifest provenance is incomplete".to_string());
            }
        }
        let _ = layout()?;
        return Ok(ModelArtifact {
            xml,
            bin,
            overlap: 2,
            inference_frames: INFERENCE_FRAMES,
            chunk_samples: CHUNK_SAMPLES,
        });
    }
    if model_id != MODEL_ID
        || config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("dry")
    {
        return Err("Denoise worker requires the explicit dry semantic output".to_string());
    }
    let manifest_path = directory.join("manifest.json");
    let config_path = directory.join("config.yaml");
    let xml = directory.join("melband-roformer-denoise-neural.xml");
    let bin = directory.join("melband-roformer-denoise-neural.bin");
    if !manifest_path.is_file() || !config_path.is_file() || !xml.is_file() || !bin.is_file() {
        return Err("Denoise OpenVINO generation is incomplete".to_string());
    }
    let manifest: ArtifactManifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Denoise artifact manifest is invalid: {error}"))?;
    let input_shape = [1, INFERENCE_FRAMES, GATHERED_WIDTH];
    let valid_tensor = |tensor: &TensorContract, name: &str| {
        tensor.name == name && tensor.dtype == "f32" && tensor.exact_validation_shape == input_shape
    };
    let declared = |name: &str, path: &Path| -> Result<bool, String> {
        let Some(file) = manifest.files.get(name) else {
            return Ok(false);
        };
        let metadata = path.metadata().map_err(|error| error.to_string())?;
        Ok(metadata.is_file() && metadata.len() == file.bytes)
    };
    if manifest.schema_version != 1
        || manifest.resource != "model:melband_roformer_denoise_aufr33"
        || manifest.capability != "audio.denoise"
        || manifest.semantic_output != "dry"
        || manifest.conversion_recipe.graph_boundary != "band_split+transformers+mask_estimator"
        || !manifest.conversion_recipe.dynamic_time_axis
        || manifest.conversion_recipe.semantic_time_chunking
        || !valid_tensor(&manifest.io.input, "gathered_stft")
        || !valid_tensor(&manifest.io.output, "gathered_mask")
        || !declared("melband-roformer-denoise-neural.xml", &xml)?
        || !declared("melband-roformer-denoise-neural.bin", &bin)?
    {
        return Err("Denoise OpenVINO generation identity is invalid".to_string());
    }
    let _ = layout()?;
    Ok(ModelArtifact {
        xml,
        bin,
        overlap: 4,
        inference_frames: INFERENCE_FRAMES,
        chunk_samples: CHUNK_SAMPLES,
    })
}

fn reflected_index(index: isize, length: usize) -> usize {
    if length <= 1 {
        return 0;
    }
    let period = (2 * (length - 1)) as isize;
    let wrapped = index.rem_euclid(period) as usize;
    if wrapped < length {
        wrapped
    } else {
        2 * (length - 1) - wrapped
    }
}

fn hann(length: usize) -> Vec<f32> {
    (0..length)
        .map(|sample| 0.5 - 0.5 * (std::f32::consts::TAU * sample as f32 / length as f32).cos())
        .collect()
}

fn stft(
    audio: &[Vec<f32>],
    chunk_samples: usize,
    inference_frames: usize,
) -> Result<Vec<Complex32>, String> {
    if audio.len() != CHANNELS || audio.iter().any(|channel| channel.len() != chunk_samples) {
        return Err("MelBand STFT requires one exact stereo chunk".to_string());
    }
    let window = hann(FFT_SIZE);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut spectrum = vec![Complex32::new(0.0, 0.0); PACKED_FREQUENCIES * inference_frames];
    let mut buffer = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
    let center = (FFT_SIZE / 2) as isize;
    for (channel, samples) in audio.iter().enumerate() {
        for frame in 0..inference_frames {
            let start = frame as isize * HOP_SIZE as isize - center;
            for sample in 0..FFT_SIZE {
                let source = reflected_index(start + sample as isize, chunk_samples);
                buffer[sample] = Complex32::new(samples[source] * window[sample], 0.0);
            }
            fft.process(&mut buffer);
            for frequency in 0..FREQUENCIES {
                let packed = frequency * CHANNELS + channel;
                spectrum[packed * inference_frames + frame] = buffer[frequency];
            }
        }
    }
    Ok(spectrum)
}

fn gather(spectrum: &[Complex32], layout: &Layout, inference_frames: usize) -> Vec<f32> {
    let mut gathered = vec![0.0; inference_frames * GATHERED_WIDTH];
    for frame in 0..inference_frames {
        for (position, packed) in layout.frequency_indices.iter().copied().enumerate() {
            let value = spectrum[packed * inference_frames + frame];
            let destination = frame * GATHERED_WIDTH + position * 2;
            gathered[destination] = value.re;
            gathered[destination + 1] = value.im;
        }
    }
    gathered
}

fn apply_mask(
    spectrum: &[Complex32],
    gathered_mask: &[f32],
    layout: &Layout,
    inference_frames: usize,
) -> Result<Vec<Complex32>, String> {
    if gathered_mask.len() != inference_frames * GATHERED_WIDTH
        || gathered_mask.iter().any(|value| !value.is_finite())
    {
        return Err("MelBand neural mask is malformed or non-finite".to_string());
    }
    let mut masks = vec![Complex32::new(0.0, 0.0); spectrum.len()];
    for frame in 0..inference_frames {
        for (position, packed) in layout.frequency_indices.iter().copied().enumerate() {
            let source = frame * GATHERED_WIDTH + position * 2;
            masks[packed * inference_frames + frame] +=
                Complex32::new(gathered_mask[source], gathered_mask[source + 1]);
        }
    }
    for packed in 0..PACKED_FREQUENCIES {
        let count = layout.bands_per_frequency[packed / CHANNELS] as f32;
        for frame in 0..inference_frames {
            let index = packed * inference_frames + frame;
            masks[index] /= count;
            masks[index] *= spectrum[index];
        }
    }
    Ok(masks)
}

fn istft(
    spectrum: &[Complex32],
    chunk_samples: usize,
    inference_frames: usize,
) -> Result<Vec<Vec<f32>>, String> {
    if spectrum.len() != PACKED_FREQUENCIES * inference_frames {
        return Err("MelBand masked spectrum shape is invalid".to_string());
    }
    let window = hann(FFT_SIZE);
    let padded_length = chunk_samples + FFT_SIZE;
    let mut planner = FftPlanner::<f32>::new();
    let inverse = planner.plan_fft_inverse(FFT_SIZE);
    let mut output = vec![vec![0.0; chunk_samples]; CHANNELS];
    let mut overlap = vec![0.0; padded_length];
    for frame in 0..inference_frames {
        let start = frame * HOP_SIZE;
        for sample in 0..FFT_SIZE {
            overlap[start + sample] += window[sample] * window[sample];
        }
    }
    for channel in 0..CHANNELS {
        let mut padded = vec![0.0; padded_length];
        let mut buffer = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for frame in 0..inference_frames {
            for frequency in 0..FREQUENCIES {
                let packed = frequency * CHANNELS + channel;
                buffer[frequency] = spectrum[packed * inference_frames + frame];
            }
            for frequency in FREQUENCIES..FFT_SIZE {
                buffer[frequency] = buffer[FFT_SIZE - frequency].conj();
            }
            inverse.process(&mut buffer);
            let start = frame * HOP_SIZE;
            for sample in 0..FFT_SIZE {
                padded[start + sample] += buffer[sample].re * window[sample] / FFT_SIZE as f32;
            }
        }
        let center = FFT_SIZE / 2;
        for sample in 0..chunk_samples {
            let position = center + sample;
            output[channel][sample] = if overlap[position] > 1e-11 {
                padded[position] / overlap[position]
            } else {
                0.0
            };
        }
    }
    Ok(output)
}

fn padded_audio(audio: &[f32], frames: usize, chunk_samples: usize) -> Vec<Vec<f32>> {
    let pad = chunk_samples / 2;
    let mut padded = vec![vec![0.0; frames + chunk_samples]; CHANNELS];
    for channel in 0..CHANNELS {
        for position in 0..padded[channel].len() {
            let source_frame = reflected_index(position as isize - pad as isize, frames);
            padded[channel][position] = audio[source_frame * CHANNELS + channel];
        }
    }
    padded
}

pub(crate) fn process_audio(
    audio: &[f32],
    inference_frames: usize,
    chunk_samples: usize,
    overlap: usize,
    mut infer_mask: impl FnMut(&[f32]) -> Result<Vec<f32>, String>,
    mut progress: impl FnMut(f32, &str),
) -> Result<Vec<f32>, String> {
    if audio.is_empty() || !audio.len().is_multiple_of(CHANNELS) {
        return Err("Denoise input PCM is empty or malformed".to_string());
    }
    if inference_frames < 2
        || chunk_samples != (inference_frames - 1) * HOP_SIZE
        || overlap == 0
        || !chunk_samples.is_multiple_of(overlap)
    {
        return Err("MelBand overlap contract is invalid".to_string());
    }
    let chunk_step = chunk_samples / overlap;
    let frames = audio.len() / CHANNELS;
    let padded = padded_audio(audio, frames, chunk_samples);
    let chunk_count = (padded[0].len() - chunk_samples) / chunk_step + 1;
    let window = hann(chunk_samples)
        .into_iter()
        .map(|value| value + 1e-8)
        .collect::<Vec<_>>();
    let mut accumulated = vec![vec![0.0; padded[0].len()]; CHANNELS];
    let mut weights = vec![0.0; padded[0].len()];
    let layout = layout()?;
    for chunk in 0..chunk_count {
        let start = chunk * chunk_step;
        let channels = (0..CHANNELS)
            .map(|channel| padded[channel][start..start + chunk_samples].to_vec())
            .collect::<Vec<_>>();
        progress(
            chunk as f32 / chunk_count as f32,
            "Computing exact MelBand STFT and gather",
        );
        let spectrum = stft(&channels, chunk_samples, inference_frames)?;
        let gathered = gather(&spectrum, layout, inference_frames);
        progress(
            (chunk as f32 + 0.4) / chunk_count as f32,
            "Running MelBand cleanup neural island on OpenVINO GPU",
        );
        let mask = infer_mask(&gathered)?;
        let reconstructed = istft(
            &apply_mask(&spectrum, &mask, layout, inference_frames)?,
            chunk_samples,
            inference_frames,
        )?;
        for sample in 0..chunk_samples {
            let position = start + sample;
            weights[position] += window[sample];
            for channel in 0..CHANNELS {
                accumulated[channel][position] += reconstructed[channel][sample] * window[sample];
            }
        }
    }
    let pad = chunk_samples / 2;
    let mut output = Vec::with_capacity(audio.len());
    for frame in 0..frames {
        let position = pad + frame;
        let weight = weights[position].max(1e-8);
        for channel in 0..CHANNELS {
            let value = accumulated[channel][position] / weight;
            if !value.is_finite() {
                return Err("Denoise reconstructed audio is non-finite".to_string());
            }
            output.push(value);
        }
    }
    progress(1.0, "MelBand cleanup stem reconstruction complete");
    Ok(output)
}

pub(crate) fn process_audio_staged(
    audio: &[f32],
    inference_frames: usize,
    chunk_samples: usize,
    overlap: usize,
    mut infer_masks: impl FnMut(Vec<Vec<f32>>) -> Result<Vec<Vec<f32>>, String>,
    mut progress: impl FnMut(f32, &str),
) -> Result<Vec<f32>, String> {
    if audio.is_empty() || !audio.len().is_multiple_of(CHANNELS) {
        return Err("MelBand staged input PCM is empty or malformed".to_string());
    }
    if inference_frames < 2
        || chunk_samples != (inference_frames - 1) * HOP_SIZE
        || overlap == 0
        || !chunk_samples.is_multiple_of(overlap)
    {
        return Err("MelBand staged overlap contract is invalid".to_string());
    }
    let chunk_step = chunk_samples / overlap;
    let frames = audio.len() / CHANNELS;
    let padded = padded_audio(audio, frames, chunk_samples);
    let chunk_count = (padded[0].len() - chunk_samples) / chunk_step + 1;
    let layout = layout()?;
    let mut spectra = Vec::with_capacity(chunk_count);
    let mut gathered_chunks = Vec::with_capacity(chunk_count);
    for chunk in 0..chunk_count {
        let start = chunk * chunk_step;
        let channels = (0..CHANNELS)
            .map(|channel| padded[channel][start..start + chunk_samples].to_vec())
            .collect::<Vec<_>>();
        progress(
            0.2 * chunk as f32 / chunk_count as f32,
            "Computing staged MelBand STFT and gather",
        );
        let spectrum = stft(&channels, chunk_samples, inference_frames)?;
        gathered_chunks.push(gather(&spectrum, layout, inference_frames));
        spectra.push(spectrum);
    }
    progress(0.2, "Running rolling-residency MelBand neural islands");
    let masks = infer_masks(gathered_chunks)?;
    if masks.len() != chunk_count {
        return Err("MelBand staged neural output count is invalid".to_string());
    }

    let window = hann(chunk_samples)
        .into_iter()
        .map(|value| value + 1e-8)
        .collect::<Vec<_>>();
    let mut accumulated = vec![vec![0.0; padded[0].len()]; CHANNELS];
    let mut weights = vec![0.0; padded[0].len()];
    for (chunk, (spectrum, mask)) in spectra.into_iter().zip(masks).enumerate() {
        progress(
            0.8 + 0.15 * chunk as f32 / chunk_count as f32,
            "Reconstructing staged MelBand overlap-add chunk",
        );
        let reconstructed = istft(
            &apply_mask(&spectrum, &mask, layout, inference_frames)?,
            chunk_samples,
            inference_frames,
        )?;
        let start = chunk * chunk_step;
        for sample in 0..chunk_samples {
            let position = start + sample;
            weights[position] += window[sample];
            for channel in 0..CHANNELS {
                accumulated[channel][position] += reconstructed[channel][sample] * window[sample];
            }
        }
    }
    let pad = chunk_samples / 2;
    let mut output = Vec::with_capacity(audio.len());
    for frame in 0..frames {
        let position = pad + frame;
        let weight = weights[position].max(1e-8);
        for channel in 0..CHANNELS {
            let value = accumulated[channel][position] / weight;
            if !value.is_finite() {
                return Err("MelBand staged reconstructed audio is non-finite".to_string());
            }
            output.push(value);
        }
    }
    progress(1.0, "Staged MelBand stem reconstruction complete");
    Ok(output)
}

pub(crate) fn infer_pcm(
    model_id: &str,
    audio: &[f32],
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<Vec<f32>, String> {
    let artifact = model_artifact(model_id, config)?;
    let device = crate::runtime::inference_device(config)?;
    let openvino_device = device.openvino();
    progress(0.01, "Validating source-built OpenVINO runtime");
    let _runtime_manifest_sha256 = crate::runtime::validate_runtime()?;
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    crate::runtime::configure_inference_core(&mut core, device)?;
    if device == crate::runtime::InferenceDevice::Gpu {
        core.set_properties(
            &DeviceType::GPU,
            [(
                RwPropertyKey::Other("GPU_ENABLE_LOOP_UNROLLING".into()),
                "NO",
            )],
        )
        .map_err(|error| format!("could not configure MelBand GPU graph mode: {error}"))?;
    }
    let xml = artifact
        .xml
        .to_str()
        .ok_or_else(|| "Denoise XML path is not valid UTF-8".to_string())?;
    let bin = artifact
        .bin
        .to_str()
        .ok_or_else(|| "Denoise BIN path is not valid UTF-8".to_string())?;
    progress(
        0.03,
        "Compiling exact MelBand cleanup neural island on OpenVINO",
    );
    let graph = core
        .read_model_from_file(xml, bin)
        .map_err(|error| format!("could not read Denoise OpenVINO IR: {error}"))?;
    let mut compiled = core
        .compile_model(&graph, openvino_device)
        .map_err(|error| {
            format!(
                "could not compile MelBand OpenVINO IR for {}: {error}",
                device.label()
            )
        })?;
    let output = process_audio(
        audio,
        artifact.inference_frames,
        artifact.chunk_samples,
        artifact.overlap,
        |gathered| {
            let shape = Shape::new(&[1, artifact.inference_frames as i64, GATHERED_WIDTH as i64])
                .map_err(|error| error.to_string())?;
            let mut tensor =
                Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
            tensor
                .get_data_mut::<f32>()
                .map_err(|error| error.to_string())?
                .copy_from_slice(gathered);
            let mut request = compiled
                .create_infer_request()
                .map_err(|error| format!("could not create Denoise inference request: {error}"))?;
            request
                .set_input_tensor(&tensor)
                .map_err(|error| format!("could not bind Denoise input: {error}"))?;
            request.infer().map_err(|error| {
                format!(
                    "MelBand OpenVINO {} inference failed: {error}",
                    device.label()
                )
            })?;
            let output = request
                .get_output_tensor()
                .map_err(|error| format!("could not read Denoise mask: {error}"))?;
            let dimensions = output
                .get_shape()
                .map_err(|error| error.to_string())?
                .get_dimensions()
                .to_vec();
            if dimensions != [1, artifact.inference_frames as i64, GATHERED_WIDTH as i64] {
                return Err(format!("unexpected Denoise mask shape: {dimensions:?}"));
            }
            Ok(output
                .get_data::<f32>()
                .map_err(|error| format!("Denoise mask is not float32: {error}"))?
                .to_vec())
        },
        |fraction, message| progress(0.05 + fraction * 0.9, message),
    )?;
    progress(0.96, "MelBand neural stem reconstruction complete");
    Ok(output)
}

pub fn infer(
    model_id: &str,
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    let output_filename = match model_id {
        MODEL_ID => "clean-lead-vocal.flac",
        DEREVERB_MODEL_ID => "noreverb-vocal.flac",
        _ => return Err("unsupported single-output MelBand cleanup model".to_string()),
    };
    let output = infer_pcm(model_id, audio, config, |fraction, message| {
        progress(fraction * 0.98, message);
    })?;
    progress(0.99, "Atomically encoding lossless MelBand cleanup stem");
    crate::audio::encode_stereo_flac(&output, output_dir, output_filename)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_stft_and_istft_round_trip_stereo_timeline() {
        let mut audio = vec![vec![0.0; CHUNK_SAMPLES]; CHANNELS];
        for sample in 0..CHUNK_SAMPLES {
            let time = sample as f32 / SAMPLE_RATE as f32;
            audio[0][sample] = 0.25 * (std::f32::consts::TAU * 220.0 * time).sin();
            audio[1][sample] = 0.2 * (std::f32::consts::TAU * 330.0 * time).sin();
        }
        let spectrum = stft(&audio, CHUNK_SAMPLES, INFERENCE_FRAMES).unwrap();
        assert_eq!(spectrum.len(), PACKED_FREQUENCIES * INFERENCE_FRAMES);
        // Selected values are from torch.stft with the exact accepted source
        // configuration and deterministic stereo sine fixture.
        for (packed, frame, expected_real, expected_imaginary) in [
            (0, 0, 16.087_448_f32, 0.0_f32),
            (0, 1, 9.733_864_f32, 0.0_f32),
            (1, 0, 8.514_681_f32, 0.0_f32),
            (200, 440, -0.000_118_052_94_f32, 0.000_042_052_125_f32),
            (2_048, 400, 0.000_084_527_375_f32, 0.0_f32),
            (2_049, 800, -0.001_798_152_9_f32, 0.0_f32),
        ] {
            let actual = spectrum[packed * INFERENCE_FRAMES + frame];
            assert!(
                (actual.re - expected_real).abs() < 2e-4,
                "{packed}/{frame}: {actual:?}"
            );
            assert!(
                (actual.im - expected_imaginary).abs() < 2e-4,
                "{packed}/{frame}: {actual:?}"
            );
        }
        let gathered = gather(&spectrum, layout().unwrap(), INFERENCE_FRAMES);
        for (frame, feature, expected) in [
            (0, 0, 16.087_448_f32),
            (0, 2, 8.514_681_f32),
            (400, 1_234, -0.000_587_072_f32),
            (800, 7_914, -0.001_798_152_9_f32),
        ] {
            assert!((gathered[frame * GATHERED_WIDTH + feature] - expected).abs() < 2e-4);
        }
        let reconstructed = istft(&spectrum, CHUNK_SAMPLES, INFERENCE_FRAMES).unwrap();
        let maximum_error = audio
            .iter()
            .flatten()
            .zip(reconstructed.iter().flatten())
            .map(|(expected, actual)| (expected - actual).abs())
            .fold(0.0_f32, f32::max);
        assert!(maximum_error < 2e-5, "maximum error {maximum_error}");
    }

    #[test]
    fn staged_chunk_scheduling_preserves_overlap_add_output() {
        let frames = 5;
        let chunk_samples = HOP_SIZE * (frames - 1);
        let sample_count = chunk_samples + HOP_SIZE * 3;
        let mut interleaved = vec![0.0; sample_count * CHANNELS];
        for sample in 0..sample_count {
            let time = sample as f32 / SAMPLE_RATE as f32;
            interleaved[sample * CHANNELS] = 0.2 * (std::f32::consts::TAU * 220.0 * time).sin();
            interleaved[sample * CHANNELS + 1] =
                0.15 * (std::f32::consts::TAU * 330.0 * time).sin();
        }
        let identity_mask = || {
            let mut mask = vec![0.0; frames * GATHERED_WIDTH];
            for complex in mask.chunks_exact_mut(2) {
                complex[0] = 1.0;
            }
            mask
        };
        let sequential = process_audio(
            &interleaved,
            frames,
            chunk_samples,
            2,
            |_| Ok(identity_mask()),
            |_, _| {},
        )
        .unwrap();
        let staged = process_audio_staged(
            &interleaved,
            frames,
            chunk_samples,
            2,
            |chunks| Ok(chunks.into_iter().map(|_| identity_mask()).collect()),
            |_, _| {},
        )
        .unwrap();
        assert_eq!(staged, sequential);
    }

    #[test]
    fn gather_and_identity_scatter_preserve_exact_packed_spectrum() {
        let layout = layout().unwrap();
        let spectrum = vec![Complex32::new(0.25, -0.5); PACKED_FREQUENCIES * INFERENCE_FRAMES];
        // A complex identity mask is (1, 0).
        let mut identity_mask = vec![0.0; INFERENCE_FRAMES * GATHERED_WIDTH];
        for pair in identity_mask.chunks_exact_mut(2) {
            pair[0] = 1.0;
        }
        let masked = apply_mask(&spectrum, &identity_mask, layout, INFERENCE_FRAMES).unwrap();
        assert_eq!(masked, spectrum);
        assert_eq!(
            gather(&spectrum, layout, INFERENCE_FRAMES).len(),
            INFERENCE_FRAMES * GATHERED_WIDTH
        );
    }
}
