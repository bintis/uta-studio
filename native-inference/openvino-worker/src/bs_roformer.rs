#![allow(clippy::needless_range_loop)] // Explicit DSP and tensor-axis indexing.

use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use rustfft::FftPlanner;
use rustfft::num_complex::Complex32;
use serde::Deserialize;

const MODEL_ID: &str = "bs_roformer_vocals_ep317";
const SAMPLE_RATE: usize = 44_100;
const CHANNELS: usize = 2;
const CHUNK_SAMPLES: usize = 352_800;
const OVERLAP: usize = 4;
const CHUNK_STEP: usize = CHUNK_SAMPLES / OVERLAP;
const FFT_SIZE: usize = 2_048;
const HOP_SIZE: usize = 441;
const FREQUENCIES: usize = FFT_SIZE / 2 + 1;
const MODEL_FREQUENCIES: usize = FREQUENCIES * CHANNELS;
const FRAMES: usize = 801;
const BANDS: usize = 62;
const DIM: usize = 512;
const GATHERED_WIDTH: usize = 4_100;
const TIME_BATCH: usize = 8;
const FREQUENCY_BATCH: usize = 64;
const MASK_GROUPS: [(usize, usize, usize); 8] = [
    (0, 8, 64),
    (8, 16, 64),
    (16, 24, 64),
    (24, 32, 128),
    (32, 40, 256),
    (40, 48, 576),
    (48, 56, 1_152),
    (56, 62, 1_796),
];
const MAX_SAMPLES: usize = SAMPLE_RATE * 60 * 60;

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    resource: String,
    capability: String,
    semantic_output: String,
    exact_contract: ExactContract,
    islands: Vec<IslandIdentity>,
}

#[derive(Deserialize)]
struct ExactContract {
    sample_rate: usize,
    channels: usize,
    chunk_samples: usize,
    frames: usize,
    hop_length: usize,
    overlap: usize,
    bands: usize,
    feature_dim: usize,
    gathered_width: usize,
    time_microbatch: usize,
    frequency_microbatch: usize,
    full_time_context_preserved: bool,
}

#[derive(Deserialize)]
struct IslandIdentity {
    name: String,
    kind: String,
    device: String,
    layer: Option<usize>,
    start: Option<usize>,
    end: Option<usize>,
    xml: FileIdentity,
    bin: FileIdentity,
}

#[derive(Deserialize)]
struct FileIdentity {
    filename: String,
    bytes: u64,
}

struct IslandPaths {
    name: String,
    xml: PathBuf,
    bin: PathBuf,
}

struct MaskIsland {
    paths: IslandPaths,
    start: usize,
    end: usize,
    output_width: usize,
}

struct Pipeline {
    band: IslandPaths,
    layers: Vec<(IslandPaths, IslandPaths)>,
    norm: IslandPaths,
    masks: Vec<MaskIsland>,
}

type ExpectedIsland = (
    String,
    &'static str,
    &'static str,
    Option<usize>,
    Option<usize>,
    Option<usize>,
);

fn expected_islands() -> Vec<ExpectedIsland> {
    let mut result = vec![("band-split".into(), "band", "CPU", None, None, None)];
    for layer in 0..12 {
        result.push((
            format!("layer-{layer:02}-time"),
            "time",
            "GPU",
            Some(layer),
            None,
            None,
        ));
        result.push((
            format!("layer-{layer:02}-freq"),
            "freq",
            "GPU",
            Some(layer),
            None,
            None,
        ));
    }
    result.push(("final-norm".into(), "norm", "CPU", None, None, None));
    for (start, end, _) in MASK_GROUPS {
        result.push((
            format!("mask-{start:02}-{:02}", end - 1),
            "mask",
            "CPU",
            None,
            Some(start),
            Some(end),
        ));
    }
    result
}

fn validate_file(directory: &Path, identity: &FileIdentity) -> Result<PathBuf, String> {
    let filename = Path::new(&identity.filename);
    if filename.file_name().and_then(|value| value.to_str()) != Some(identity.filename.as_str()) {
        return Err("BS-RoFormer island filename is not a local basename".to_string());
    }
    let path = directory.join(filename);
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != identity.bytes {
        return Err(format!(
            "BS-RoFormer island identity mismatch: {}",
            identity.filename
        ));
    }
    Ok(path)
}

fn validate_manifest(directory: &Path) -> Result<Manifest, String> {
    if !directory.is_dir() {
        return Err("resolved BS-RoFormer split generation is unavailable".to_string());
    }
    let path = directory.join("manifest.json");
    if !path.is_file() || !directory.join("config.yaml").is_file() {
        return Err("BS-RoFormer split generation is incomplete".to_string());
    }
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("BS-RoFormer split manifest is invalid: {error}"))?;
    let contract = &manifest.exact_contract;
    if manifest.schema_version != 2
        || manifest.resource != format!("model:{MODEL_ID}")
        || manifest.capability != "audio.extract_vocals"
        || manifest.semantic_output != "guide_vocals"
        || contract.sample_rate != SAMPLE_RATE
        || contract.channels != CHANNELS
        || contract.chunk_samples != CHUNK_SAMPLES
        || contract.frames != FRAMES
        || contract.hop_length != HOP_SIZE
        || contract.overlap != OVERLAP
        || contract.bands != BANDS
        || contract.feature_dim != DIM
        || contract.gathered_width != GATHERED_WIDTH
        || contract.time_microbatch != TIME_BATCH
        || contract.frequency_microbatch != FREQUENCY_BATCH
        || !contract.full_time_context_preserved
    {
        return Err("BS-RoFormer split manifest contract mismatch".to_string());
    }
    let expected = expected_islands();
    if manifest.islands.len() != expected.len() {
        return Err("BS-RoFormer split manifest island count mismatch".to_string());
    }
    for (island, (name, kind, device, layer, start, end)) in manifest.islands.iter().zip(expected) {
        if island.name != name
            || island.kind != kind
            || island.device != device
            || island.layer != layer
            || island.start != start
            || island.end != end
        {
            return Err(format!(
                "BS-RoFormer split island order mismatch: {}",
                island.name
            ));
        }
        validate_file(directory, &island.xml)?;
        validate_file(directory, &island.bin)?;
    }
    Ok(manifest)
}

fn paths(directory: &Path, island: &IslandIdentity) -> IslandPaths {
    IslandPaths {
        name: island.name.clone(),
        xml: directory.join(&island.xml.filename),
        bin: directory.join(&island.bin.filename),
    }
}

fn compile_paths(
    core: &mut Core,
    paths: &IslandPaths,
    device: DeviceType<'_>,
) -> Result<CompiledModel, String> {
    let graph = core
        .read_model_from_file(
            paths
                .xml
                .to_str()
                .ok_or_else(|| "BS-RoFormer XML path is not UTF-8".to_string())?,
            paths
                .bin
                .to_str()
                .ok_or_else(|| "BS-RoFormer BIN path is not UTF-8".to_string())?,
        )
        .map_err(|error| format!("could not read BS-RoFormer {} IR: {error}", paths.name))?;
    let device_name = device.to_string();
    core.compile_model(&graph, device).map_err(|error| {
        format!(
            "could not compile BS-RoFormer {} on {device_name}: {error}",
            paths.name
        )
    })
}

fn configured_core(device: DeviceType<'_>) -> Result<Core, String> {
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    let devices = core
        .available_devices()
        .map_err(|error| error.to_string())?;
    if !devices.contains(&device) {
        return Err(format!(
            "BS-RoFormer requires explicit OpenVINO {device}; fallback is forbidden"
        ));
    }
    core.set_properties(
        &device,
        [
            (RwPropertyKey::HintInferencePrecision, "f32"),
            (RwPropertyKey::HintExecutionMode, "ACCURACY"),
        ],
    )
    .map_err(|error| format!("could not configure OpenVINO {device}: {error}"))?;
    if device == DeviceType::GPU {
        crate::runtime::configure_low_impact_gpu_queue(&mut core)?;
    }
    Ok(core)
}

fn prepare_pipeline(
    directory: &Path,
    manifest: &Manifest,
    compute_device: DeviceType<'_>,
) -> Result<Pipeline, String> {
    let _ = crate::runtime::validate_runtime()?;
    // Probe only devices selected by the explicit route. CPU-only execution
    // must not initialize the GPU plugin or create a GPU context.
    drop(configured_core(DeviceType::CPU)?);
    if compute_device == DeviceType::GPU {
        drop(configured_core(DeviceType::GPU)?);
    }
    let mut layers = Vec::with_capacity(12);
    for layer in 0..12 {
        let offset = 1 + layer * 2;
        layers.push((
            paths(directory, &manifest.islands[offset]),
            paths(directory, &manifest.islands[offset + 1]),
        ));
    }
    let mut masks = Vec::with_capacity(MASK_GROUPS.len());
    for (index, (start, end, output_width)) in MASK_GROUPS.into_iter().enumerate() {
        masks.push(MaskIsland {
            paths: paths(directory, &manifest.islands[26 + index]),
            start,
            end,
            output_width,
        });
    }
    Ok(Pipeline {
        band: paths(directory, &manifest.islands[0]),
        layers,
        norm: paths(directory, &manifest.islands[25]),
        masks,
    })
}

fn run_model(
    model: &mut CompiledModel,
    input: &[f32],
    shape: &[i64],
    expected: &[i64],
) -> Result<Vec<f32>, String> {
    let shape = Shape::new(shape).map_err(|error| error.to_string())?;
    let mut tensor = Tensor::new(ElementType::F32, &shape).map_err(|error| error.to_string())?;
    tensor
        .get_data_mut::<f32>()
        .map_err(|error| error.to_string())?
        .copy_from_slice(input);
    let mut request = model
        .create_infer_request()
        .map_err(|error| error.to_string())?;
    request
        .set_input_tensor(&tensor)
        .map_err(|error| error.to_string())?;
    request.infer().map_err(|error| error.to_string())?;
    let output = request
        .get_output_tensor()
        .map_err(|error| error.to_string())?;
    let dimensions = output
        .get_shape()
        .map_err(|error| error.to_string())?
        .get_dimensions()
        .to_vec();
    if dimensions != expected {
        return Err(format!(
            "BS-RoFormer island returned unexpected shape: {dimensions:?}"
        ));
    }
    let values = output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if values.iter().any(|value| !value.is_finite()) {
        return Err("BS-RoFormer island returned non-finite values".to_string());
    }
    Ok(values)
}

fn repeat_last_valid_batch_lane(
    values: &mut [f32],
    valid_lanes: usize,
    total_lanes: usize,
    lane_width: usize,
) -> Result<(), String> {
    let expected_length = total_lanes
        .checked_mul(lane_width)
        .ok_or_else(|| "BS-RoFormer batch padding size overflowed".to_string())?;
    if valid_lanes == 0
        || valid_lanes > total_lanes
        || lane_width == 0
        || values.len() != expected_length
    {
        return Err("BS-RoFormer batch padding contract mismatch".to_string());
    }
    if valid_lanes == total_lanes {
        return Ok(());
    }
    let source = (valid_lanes - 1) * lane_width;
    for lane in valid_lanes..total_lanes {
        values.copy_within(source..source + lane_width, lane * lane_width);
    }
    Ok(())
}

fn run_pipeline(
    pipeline: &Pipeline,
    gathered_chunks: Vec<Vec<f32>>,
    compute_device: DeviceType<'_>,
) -> Result<Vec<Vec<f32>>, String> {
    if gathered_chunks.is_empty()
        || gathered_chunks
            .iter()
            .any(|gathered| gathered.len() != FRAMES * GATHERED_WIDTH)
    {
        return Err("BS-RoFormer gathered STFT shapes are invalid".to_string());
    }

    // CPU pre-phase: compile and run only band split, then release the CPU
    // model and Core before any GPU model is compiled.
    let mut cpu_core = configured_core(DeviceType::CPU)?;
    let mut band_model = compile_paths(&mut cpu_core, &pipeline.band, DeviceType::CPU)?;
    let mut feature_chunks = Vec::with_capacity(gathered_chunks.len());
    for gathered in gathered_chunks {
        feature_chunks.push(run_model(
            &mut band_model,
            &gathered,
            &[1, FRAMES as i64, GATHERED_WIDTH as i64],
            &[1, FRAMES as i64, BANDS as i64, DIM as i64],
        )?);
    }
    drop(band_model);
    drop(cpu_core);

    // Compute phase: one rolling transformer island is resident at a time.
    // In CPU-only mode this never initializes or probes the GPU plugin.
    let compute_is_gpu = compute_device == DeviceType::GPU;
    let mut compute_core = configured_core(if compute_is_gpu {
        DeviceType::GPU
    } else {
        DeviceType::CPU
    })?;
    for (layer, (time_paths, frequency_paths)) in pipeline.layers.iter().enumerate() {
        eprintln!(
            "[uta-openvino-worker] BS-RoFormer stage-major layer {}/12 time",
            layer + 1
        );
        let mut time_model = compile_paths(
            &mut compute_core,
            time_paths,
            if compute_is_gpu {
                DeviceType::GPU
            } else {
                DeviceType::CPU
            },
        )?;
        let mut time_chunks = Vec::with_capacity(feature_chunks.len());
        for features in std::mem::take(&mut feature_chunks) {
            let mut time_output = vec![0.0; features.len()];
            for band_start in (0..BANDS).step_by(TIME_BATCH) {
                let valid = (BANDS - band_start).min(TIME_BATCH);
                let mut input = vec![0.0; TIME_BATCH * FRAMES * DIM];
                for band in 0..valid {
                    for frame in 0..FRAMES {
                        let source = (frame * BANDS + band_start + band) * DIM;
                        let destination = (band * FRAMES + frame) * DIM;
                        input[destination..destination + DIM]
                            .copy_from_slice(&features[source..source + DIM]);
                    }
                }
                // Avoid all-zero padding lanes: Intel GPU low-precision transformer
                // kernels can return NaN for those lanes even though callers discard them.
                repeat_last_valid_batch_lane(&mut input, valid, TIME_BATCH, FRAMES * DIM)?;
                let output = run_model(
                    &mut time_model,
                    &input,
                    &[TIME_BATCH as i64, FRAMES as i64, DIM as i64],
                    &[TIME_BATCH as i64, FRAMES as i64, DIM as i64],
                )?;
                for band in 0..valid {
                    for frame in 0..FRAMES {
                        let source = (band * FRAMES + frame) * DIM;
                        let destination = (frame * BANDS + band_start + band) * DIM;
                        time_output[destination..destination + DIM]
                            .copy_from_slice(&output[source..source + DIM]);
                    }
                }
            }
            time_chunks.push(time_output);
        }
        drop(time_model);

        eprintln!(
            "[uta-openvino-worker] BS-RoFormer stage-major layer {}/12 frequency",
            layer + 1
        );
        let mut frequency_model = compile_paths(
            &mut compute_core,
            frequency_paths,
            if compute_is_gpu {
                DeviceType::GPU
            } else {
                DeviceType::CPU
            },
        )?;
        feature_chunks.reserve(time_chunks.len());
        for time_output in time_chunks {
            let mut frequency_output = vec![0.0; time_output.len()];
            for frame_start in (0..FRAMES).step_by(FREQUENCY_BATCH) {
                let valid = (FRAMES - frame_start).min(FREQUENCY_BATCH);
                let count = valid * BANDS * DIM;
                let source = frame_start * BANDS * DIM;
                let mut input = vec![0.0; FREQUENCY_BATCH * BANDS * DIM];
                input[..count].copy_from_slice(&time_output[source..source + count]);
                repeat_last_valid_batch_lane(&mut input, valid, FREQUENCY_BATCH, BANDS * DIM)?;
                let output = run_model(
                    &mut frequency_model,
                    &input,
                    &[FREQUENCY_BATCH as i64, BANDS as i64, DIM as i64],
                    &[FREQUENCY_BATCH as i64, BANDS as i64, DIM as i64],
                )?;
                frequency_output[source..source + count].copy_from_slice(&output[..count]);
            }
            feature_chunks.push(frequency_output);
        }
        drop(frequency_model);
    }
    drop(compute_core);

    // CPU post-phase: norm and each mask group are compiled and released
    // individually, avoiding both CPU/GPU co-residency and aggregate mask
    // residency.
    let mut cpu_core = configured_core(DeviceType::CPU)?;
    let mut norm_model = compile_paths(&mut cpu_core, &pipeline.norm, DeviceType::CPU)?;
    for features in &mut feature_chunks {
        *features = run_model(
            &mut norm_model,
            features,
            &[1, FRAMES as i64, BANDS as i64, DIM as i64],
            &[1, FRAMES as i64, BANDS as i64, DIM as i64],
        )?;
    }
    drop(norm_model);

    let mut gathered_masks = vec![vec![0.0; FRAMES * GATHERED_WIDTH]; feature_chunks.len()];
    let mut width_offset = 0;
    for mask in &pipeline.masks {
        let mut mask_model = compile_paths(&mut cpu_core, &mask.paths, DeviceType::CPU)?;
        let bands = mask.end - mask.start;
        for (features, gathered_mask) in feature_chunks.iter().zip(&mut gathered_masks) {
            let mut input = vec![0.0; FRAMES * bands * DIM];
            for frame in 0..FRAMES {
                for band in 0..bands {
                    let source = (frame * BANDS + mask.start + band) * DIM;
                    let destination = (frame * bands + band) * DIM;
                    input[destination..destination + DIM]
                        .copy_from_slice(&features[source..source + DIM]);
                }
            }
            let output = run_model(
                &mut mask_model,
                &input,
                &[1, FRAMES as i64, bands as i64, DIM as i64],
                &[1, FRAMES as i64, mask.output_width as i64],
            )?;
            for frame in 0..FRAMES {
                let source = frame * mask.output_width;
                let destination = frame * GATHERED_WIDTH + width_offset;
                gathered_mask[destination..destination + mask.output_width]
                    .copy_from_slice(&output[source..source + mask.output_width]);
            }
        }
        drop(mask_model);
        width_offset += mask.output_width;
    }
    if width_offset != GATHERED_WIDTH {
        return Err("BS-RoFormer mask groups do not cover the gathered spectrum".to_string());
    }
    Ok(gathered_masks)
}

pub fn infer(
    interleaved: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    let device = crate::runtime::inference_device(config)?;
    if config
        .get("semantic_output")
        .and_then(|value| value.as_str())
        != Some("guide_vocals")
    {
        return Err("BS-RoFormer requires explicit GuideVocals semantics".to_string());
    }
    if interleaved.is_empty() || !interleaved.len().is_multiple_of(CHANNELS) {
        return Err("BS-RoFormer stereo PCM is empty or malformed".to_string());
    }
    let model_dir = config
        .get("model_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "BS-RoFormer model path is missing".to_string())?;
    let manifest = validate_manifest(&model_dir)?;
    let samples = interleaved.len() / CHANNELS;
    let audio = std::array::from_fn(|channel| {
        (0..samples)
            .map(|frame| interleaved[frame * CHANNELS + channel])
            .collect::<Vec<_>>()
    });
    eprintln!(
        "[uta-openvino-worker] model={MODEL_ID} backend={} samples={samples} exact_frames={FRAMES}",
        device.evidence_backend()
    );
    progress(0.01, "Preparing explicit BS-RoFormer execution phases");
    let pipeline = prepare_pipeline(&model_dir, &manifest, device.openvino())?;
    progress(
        0.03,
        "Running phase-separated exact-context BS-RoFormer islands",
    );
    let result = process_audio_staged(&audio, &pipeline, device.openvino())?;
    if result.iter().flatten().any(|sample| !sample.is_finite()) {
        return Err("BS-RoFormer returned non-finite vocal audio".to_string());
    }
    let mut output = Vec::with_capacity(interleaved.len());
    for frame in 0..samples {
        output.push(result[0][frame]);
        output.push(result[1][frame]);
    }
    progress(0.97, "Atomically encoding lossless GuideVocals stem");
    crate::audio::encode_stereo_flac(&output, output_dir, "guide-vocals.flac")
}

fn process_audio_staged(
    audio: &[Vec<f32>; CHANNELS],
    pipeline: &Pipeline,
    compute_device: DeviceType<'_>,
) -> Result<[Vec<f32>; CHANNELS], String> {
    let samples = audio[0].len();
    if samples == 0 || samples > MAX_SAMPLES || audio[1].len() != samples {
        return Err("BS-RoFormer input is empty, malformed, or exceeds one hour".to_string());
    }
    let pad = CHUNK_SAMPLES / 2;
    let padded_samples = samples + 2 * pad;
    let chunks = (padded_samples - CHUNK_SAMPLES) / CHUNK_STEP + 1;
    let mut spectra = Vec::with_capacity(chunks);
    let mut gathered_chunks = Vec::with_capacity(chunks);
    for chunk_index in 0..chunks {
        let offset = chunk_index * CHUNK_STEP;
        let chunk = std::array::from_fn(|channel| {
            (0..CHUNK_SAMPLES)
                .map(|index| {
                    let padded_index = offset + index;
                    audio[channel][reflect_padded_index(padded_index, samples, pad)]
                })
                .collect::<Vec<_>>()
        });
        eprintln!(
            "[uta-openvino-worker] BS-RoFormer prepare chunk {}/{}",
            chunk_index + 1,
            chunks
        );
        let spectrum = stft(&chunk)?;
        let mut gathered = vec![0.0; FRAMES * GATHERED_WIDTH];
        for frame in 0..FRAMES {
            for frequency in 0..FREQUENCIES {
                for channel in 0..CHANNELS {
                    let value = spectrum[channel][frame * FREQUENCIES + frequency];
                    let model_frequency = frequency * CHANNELS + channel;
                    let gathered_offset = (frame * MODEL_FREQUENCIES + model_frequency) * 2;
                    gathered[gathered_offset] = value.re;
                    gathered[gathered_offset + 1] = value.im;
                }
            }
        }
        spectra.push(spectrum);
        gathered_chunks.push(gathered);
    }

    let masks = run_pipeline(pipeline, gathered_chunks, compute_device)?;
    if masks.len() != chunks
        || masks.iter().any(|mask| {
            mask.len() != FRAMES * GATHERED_WIDTH || mask.iter().any(|value| !value.is_finite())
        })
    {
        return Err("BS-RoFormer returned malformed or non-finite masks".to_string());
    }

    let window = periodic_hann(CHUNK_SAMPLES)
        .into_iter()
        .map(|value| value + 1.0e-8)
        .collect::<Vec<_>>();
    let mut mixed = [vec![0.0_f32; padded_samples], vec![0.0_f32; padded_samples]];
    let mut weights = vec![0.0_f32; padded_samples];
    for (chunk_index, (spectrum, mask)) in spectra.into_iter().zip(masks).enumerate() {
        let mut masked = [
            vec![Complex32::new(0.0, 0.0); FREQUENCIES * FRAMES],
            vec![Complex32::new(0.0, 0.0); FREQUENCIES * FRAMES],
        ];
        for frequency in 0..FREQUENCIES {
            for channel in 0..CHANNELS {
                let model_frequency = frequency * CHANNELS + channel;
                for frame in 0..FRAMES {
                    let gathered_offset = (frame * MODEL_FREQUENCIES + model_frequency) * 2;
                    let value = Complex32::new(mask[gathered_offset], mask[gathered_offset + 1]);
                    masked[channel][frame * FREQUENCIES + frequency] =
                        spectrum[channel][frame * FREQUENCIES + frequency] * value;
                }
            }
        }
        let separated = istft(&masked)?;
        let offset = chunk_index * CHUNK_STEP;
        for index in 0..CHUNK_SAMPLES {
            let weight = window[index];
            for channel in 0..CHANNELS {
                mixed[channel][offset + index] += separated[channel][index] * weight;
            }
            weights[offset + index] += weight;
        }
    }

    Ok(std::array::from_fn(|channel| {
        (0..samples)
            .map(|index| {
                let padded_index = pad + index;
                mixed[channel][padded_index] / weights[padded_index].max(1.0e-8)
            })
            .collect()
    }))
}

#[cfg(test)]
fn overlap_add(
    audio: &[Vec<f32>; CHANNELS],
    mut process: impl FnMut(&[Vec<f32>; CHANNELS]) -> Result<[Vec<f32>; CHANNELS], String>,
) -> Result<[Vec<f32>; CHANNELS], String> {
    let samples = audio[0].len();
    if samples == 0 || samples > MAX_SAMPLES || audio[1].len() != samples {
        return Err("BS-RoFormer input is empty, malformed, or exceeds one hour".to_string());
    }
    let pad = CHUNK_SAMPLES / 2;
    let padded_samples = samples + 2 * pad;
    let chunks = (padded_samples - CHUNK_SAMPLES) / CHUNK_STEP + 1;
    let window = periodic_hann(CHUNK_SAMPLES)
        .into_iter()
        .map(|value| value + 1.0e-8)
        .collect::<Vec<_>>();
    let mut mixed = [vec![0.0_f32; padded_samples], vec![0.0_f32; padded_samples]];
    let mut weights = vec![0.0_f32; padded_samples];
    for chunk_index in 0..chunks {
        let offset = chunk_index * CHUNK_STEP;
        let chunk = std::array::from_fn(|channel| {
            (0..CHUNK_SAMPLES)
                .map(|index| {
                    let padded_index = offset + index;
                    audio[channel][reflect_padded_index(padded_index, samples, pad)]
                })
                .collect::<Vec<_>>()
        });
        eprintln!(
            "[uta-openvino-worker] BS-RoFormer chunk {}/{}",
            chunk_index + 1,
            chunks
        );
        let separated = process(&chunk)?;
        if separated
            .iter()
            .any(|channel| channel.len() != CHUNK_SAMPLES)
        {
            return Err("BS-RoFormer chunk output length mismatch".to_string());
        }
        for index in 0..CHUNK_SAMPLES {
            let weight = window[index];
            for channel in 0..CHANNELS {
                mixed[channel][offset + index] += separated[channel][index] * weight;
            }
            weights[offset + index] += weight;
        }
    }
    Ok(std::array::from_fn(|channel| {
        (0..samples)
            .map(|index| {
                let padded_index = pad + index;
                mixed[channel][padded_index] / weights[padded_index].max(1.0e-8)
            })
            .collect()
    }))
}

fn reflect_padded_index(index: usize, samples: usize, pad: usize) -> usize {
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

fn periodic_hann(size: usize) -> Vec<f32> {
    (0..size)
        .map(|index| 0.5 - 0.5 * (2.0 * std::f32::consts::PI * index as f32 / size as f32).cos())
        .collect()
}

fn stft(audio: &[Vec<f32>; CHANNELS]) -> Result<[Vec<Complex32>; CHANNELS], String> {
    if audio.iter().any(|channel| channel.len() != CHUNK_SAMPLES) {
        return Err("BS-RoFormer STFT requires one exact configured chunk".to_string());
    }
    let pad = FFT_SIZE / 2;
    let window = periodic_hann(FFT_SIZE);
    let mut planner = FftPlanner::<f32>::new();
    let fft = planner.plan_fft_forward(FFT_SIZE);
    let mut result = [
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * FRAMES],
        vec![Complex32::new(0.0, 0.0); FREQUENCIES * FRAMES],
    ];
    for channel in 0..CHANNELS {
        let mut buffer = vec![Complex32::new(0.0, 0.0); FFT_SIZE];
        for frame in 0..FRAMES {
            let offset = frame * HOP_SIZE;
            for index in 0..FFT_SIZE {
                let source = reflect_padded_index(offset + index, CHUNK_SAMPLES, pad);
                buffer[index] = Complex32::new(audio[channel][source] * window[index], 0.0);
            }
            fft.process(&mut buffer);
            for frequency in 0..FREQUENCIES {
                result[channel][frame * FREQUENCIES + frequency] = buffer[frequency];
            }
        }
    }
    Ok(result)
}

fn istft(spectrum: &[Vec<Complex32>; CHANNELS]) -> Result<[Vec<f32>; CHANNELS], String> {
    if spectrum
        .iter()
        .any(|channel| channel.len() != FREQUENCIES * FRAMES)
    {
        return Err("BS-RoFormer iSTFT spectrum contract mismatch".to_string());
    }
    let window = periodic_hann(FFT_SIZE);
    let padded_samples = (FRAMES - 1) * HOP_SIZE + FFT_SIZE;
    let mut envelope = vec![0.0_f32; padded_samples];
    for frame in 0..FRAMES {
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
        for frame in 0..FRAMES {
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
        (0..CHUNK_SAMPLES)
            .map(|index| {
                let padded_index = trim + index;
                result[channel][padded_index] / envelope[padded_index].max(1.0e-11)
            })
            .collect()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_split_topology_preserves_full_attention_context() {
        let islands = expected_islands();
        assert_eq!(islands.len(), 34);
        assert_eq!(
            islands
                .iter()
                .filter(|(_, _, device, ..)| *device == "GPU")
                .count(),
            24
        );
        assert_eq!(
            islands
                .iter()
                .filter(|(_, _, device, ..)| *device == "CPU")
                .count(),
            10
        );
        assert_eq!(
            MASK_GROUPS.iter().map(|(_, _, width)| width).sum::<usize>(),
            GATHERED_WIDTH
        );
        assert_eq!(
            (FRAMES, BANDS, DIM, TIME_BATCH, FREQUENCY_BATCH),
            (801, 62, 512, 8, 64)
        );
    }

    #[test]
    fn partial_transformer_batches_repeat_the_last_valid_lane() {
        for (total_lanes, valid_lanes) in [
            (TIME_BATCH, BANDS % TIME_BATCH),
            (FREQUENCY_BATCH, FRAMES % FREQUENCY_BATCH),
        ] {
            assert!(valid_lanes > 0);
            let lane_width = 2;
            let mut values = vec![0.0; total_lanes * lane_width];
            for lane in 0..valid_lanes {
                values[lane * lane_width..(lane + 1) * lane_width]
                    .copy_from_slice(&[lane as f32 + 0.25, lane as f32 + 0.75]);
            }
            let expected =
                values[(valid_lanes - 1) * lane_width..valid_lanes * lane_width].to_vec();

            repeat_last_valid_batch_lane(&mut values, valid_lanes, total_lanes, lane_width)
                .unwrap();

            for lane in valid_lanes..total_lanes {
                assert_eq!(
                    &values[lane * lane_width..(lane + 1) * lane_width],
                    expected.as_slice()
                );
            }
        }
    }

    #[test]
    fn full_transformer_batch_is_not_modified_by_padding() {
        let mut values = vec![1.0, 2.0, 3.0, 4.0];
        let expected = values.clone();
        repeat_last_valid_batch_lane(&mut values, 2, 2, 2).unwrap();
        assert_eq!(values, expected);
    }

    #[test]
    fn stft_round_trip_preserves_configured_chunk() {
        let audio = std::array::from_fn(|channel| {
            (0..CHUNK_SAMPLES)
                .map(|index| {
                    let frequency = if channel == 0 { 220.0 } else { 330.0 };
                    (2.0 * std::f32::consts::PI * frequency * index as f32 / SAMPLE_RATE as f32)
                        .sin()
                        * 0.2
                })
                .collect::<Vec<_>>()
        });
        let spectrum = stft(&audio).unwrap();
        let reconstructed = istft(&spectrum).unwrap();
        for channel in 0..CHANNELS {
            let maximum = audio[channel]
                .iter()
                .zip(&reconstructed[channel])
                .map(|(expected, actual)| (expected - actual).abs())
                .fold(0.0_f32, f32::max);
            assert!(maximum < 2.0e-4, "channel {channel} max error {maximum}");
        }
    }

    #[test]
    fn overlap_add_preserves_timeline_and_stereo_with_identity_processor() {
        let audio = [vec![0.25; 97_123], vec![-0.125; 97_123]];
        let output = overlap_add(&audio, |chunk| Ok(chunk.clone())).unwrap();
        assert_eq!(output[0].len(), audio[0].len());
        assert_eq!(output[1].len(), audio[1].len());
        for channel in 0..CHANNELS {
            let maximum = audio[channel]
                .iter()
                .zip(&output[channel])
                .map(|(expected, actual)| (expected - actual).abs())
                .fold(0.0_f32, f32::max);
            assert!(maximum < 1.0e-6, "channel {channel} max error {maximum}");
        }
    }

    #[test]
    fn reflect_padding_matches_torch_for_normal_chunk_edges() {
        let samples = 8;
        let pad = 3;
        let values = (0..samples).collect::<Vec<_>>();
        let padded = (0..samples + 2 * pad)
            .map(|index| values[reflect_padded_index(index, samples, pad)])
            .collect::<Vec<_>>();
        assert_eq!(padded, vec![3, 2, 1, 0, 1, 2, 3, 4, 5, 6, 7, 6, 5, 4]);
    }
}
