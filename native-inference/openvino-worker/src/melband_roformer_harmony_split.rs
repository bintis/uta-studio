use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::Deserialize;

const MODEL_ID: &str = "melband_roformer_harmony";
const FRAMES: usize = 801;
const CHUNK_SAMPLES: usize = 352_800;
const OVERLAP: usize = 4;
const BANDS: usize = 60;
const DEPTH: usize = 6;
const DIM: usize = 384;
const GATHERED_WIDTH: usize = 7_916;
const TIME_BATCH: usize = 10;
const FREQUENCY_BATCH: usize = 64;
const MASK_GROUPS: [(usize, usize, usize); 8] = [
    (0, 8, 196),
    (8, 16, 196),
    (16, 24, 288),
    (24, 32, 504),
    (32, 40, 852),
    (40, 48, 1_472),
    (48, 56, 2_528),
    (56, 60, 1_880),
];

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
    rolling_gpu_residency: bool,
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

struct MaskIsland {
    paths: LayerIsland,
    start: usize,
    end: usize,
    output_width: usize,
}

struct LayerIsland {
    name: String,
    xml: PathBuf,
    bin: PathBuf,
}

struct Pipeline {
    band: LayerIsland,
    layers: Vec<(LayerIsland, LayerIsland)>,
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
    let mut expected = vec![("band-split".to_string(), "band", "CPU", None, None, None)];
    for layer in 0..DEPTH {
        expected.push((
            format!("layer-{layer:02}-time"),
            "time",
            "GPU",
            Some(layer),
            None,
            None,
        ));
        expected.push((
            format!("layer-{layer:02}-freq"),
            "freq",
            "GPU",
            Some(layer),
            None,
            None,
        ));
    }
    for (start, end, _) in MASK_GROUPS {
        expected.push((
            format!("mask-{start:02}-{:02}", end - 1),
            "mask",
            "CPU",
            None,
            Some(start),
            Some(end),
        ));
    }
    expected
}

fn validate_file(directory: &Path, identity: &FileIdentity) -> Result<PathBuf, String> {
    let filename = Path::new(&identity.filename);
    if filename.file_name().and_then(|value| value.to_str()) != Some(identity.filename.as_str()) {
        return Err("Harmony split island filename is not a local basename".to_string());
    }
    let path = directory.join(filename);
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != identity.bytes {
        return Err(format!(
            "Harmony split island identity mismatch: {}",
            identity.filename
        ));
    }
    Ok(path)
}

fn validate_manifest(directory: &Path) -> Result<Manifest, String> {
    let path = directory.join("manifest.json");
    if !path.is_file() || !directory.join("config.yaml").is_file() {
        return Err("Harmony split generation is incomplete".to_string());
    }
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Harmony split manifest is invalid: {error}"))?;
    let contract = &manifest.exact_contract;
    if manifest.schema_version != 2
        || manifest.resource != format!("model:{MODEL_ID}")
        || manifest.capability != "audio.lead_isolate"
        || manifest.semantic_output != "lead_vocal+backing_vocal_residual"
        || contract.sample_rate != 44_100
        || contract.channels != 2
        || contract.chunk_samples != CHUNK_SAMPLES
        || contract.frames != FRAMES
        || contract.hop_length != 441
        || contract.overlap != OVERLAP
        || contract.bands != BANDS
        || contract.feature_dim != DIM
        || contract.gathered_width != GATHERED_WIDTH
        || contract.time_microbatch != TIME_BATCH
        || contract.frequency_microbatch != FREQUENCY_BATCH
        || !contract.full_time_context_preserved
        || !contract.rolling_gpu_residency
    {
        return Err("Harmony split manifest contract mismatch".to_string());
    }
    let expected = expected_islands();
    if manifest.islands.len() != expected.len() {
        return Err("Harmony split manifest island count mismatch".to_string());
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
                "Harmony split island order mismatch: {}",
                island.name
            ));
        }
        let _ = validate_file(directory, &island.xml)?;
        let _ = validate_file(directory, &island.bin)?;
    }
    Ok(manifest)
}

fn compile_paths(
    core: &mut Core,
    name: &str,
    xml: &Path,
    bin: &Path,
    device: DeviceType<'_>,
) -> Result<CompiledModel, String> {
    let graph = core
        .read_model_from_file(
            xml.to_str()
                .ok_or_else(|| "Harmony split XML path is not UTF-8".to_string())?,
            bin.to_str()
                .ok_or_else(|| "Harmony split BIN path is not UTF-8".to_string())?,
        )
        .map_err(|error| format!("could not read Harmony split {name} IR: {error}"))?;
    let device_name = device.to_string();
    core.compile_model(&graph, device).map_err(|error| {
        format!("could not compile Harmony split {name} on {device_name}: {error}")
    })
}

fn island_paths(directory: &Path, island: &IslandIdentity) -> Result<LayerIsland, String> {
    // validate_manifest has already hashed every declared file in this same
    // immutable directory; avoid a second multi-gigabyte identity pass.
    Ok(LayerIsland {
        name: island.name.clone(),
        xml: directory.join(&island.xml.filename),
        bin: directory.join(&island.bin.filename),
    })
}

fn configured_core(device: DeviceType<'_>) -> Result<Core, String> {
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    let devices = core
        .available_devices()
        .map_err(|error| error.to_string())?;
    if !devices.contains(&device) {
        return Err(format!(
            "Harmony split requires explicit OpenVINO {device}; fallback is forbidden"
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
    let _runtime_manifest_sha256 = crate::runtime::validate_runtime()?;
    // CPU-only execution must not probe or initialize the GPU plugin.
    drop(configured_core(DeviceType::CPU)?);
    if compute_device == DeviceType::GPU {
        drop(configured_core(DeviceType::GPU)?);
    }
    let mut layers = Vec::with_capacity(DEPTH);
    for layer in 0..DEPTH {
        let offset = 1 + layer * 2;
        layers.push((
            island_paths(directory, &manifest.islands[offset])?,
            island_paths(directory, &manifest.islands[offset + 1])?,
        ));
    }
    let mut masks = Vec::with_capacity(MASK_GROUPS.len());
    for (index, (start, end, output_width)) in MASK_GROUPS.into_iter().enumerate() {
        masks.push(MaskIsland {
            paths: island_paths(directory, &manifest.islands[1 + DEPTH * 2 + index])?,
            start,
            end,
            output_width,
        });
    }
    Ok(Pipeline {
        band: island_paths(directory, &manifest.islands[0])?,
        layers,
        masks,
    })
}

fn run_model(
    model: &mut CompiledModel,
    input: &[f32],
    shape: &[i64],
    expected_shape: &[i64],
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
    if dimensions != expected_shape {
        return Err(format!(
            "Harmony split island returned unexpected shape: {dimensions:?}"
        ));
    }
    let values = output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if values.iter().any(|value| !value.is_finite()) {
        return Err("Harmony split island returned non-finite values".to_string());
    }
    Ok(values)
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
        return Err("Harmony split gathered STFT shapes are invalid".to_string());
    }

    let mut cpu_core = configured_core(DeviceType::CPU)?;
    let mut band_model = compile_paths(
        &mut cpu_core,
        &pipeline.band.name,
        &pipeline.band.xml,
        &pipeline.band.bin,
        DeviceType::CPU,
    )?;
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

    let compute_is_gpu = compute_device == DeviceType::GPU;
    let mut compute_core = configured_core(if compute_is_gpu {
        DeviceType::GPU
    } else {
        DeviceType::CPU
    })?;
    for (layer, (time_paths, frequency_paths)) in pipeline.layers.iter().enumerate() {
        eprintln!(
            "[uta-openvino-worker] Harmony stage-major layer {}/{} time",
            layer + 1,
            DEPTH
        );
        let mut time_model = compile_paths(
            &mut compute_core,
            &time_paths.name,
            &time_paths.xml,
            &time_paths.bin,
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
                let mut input = vec![0.0; TIME_BATCH * FRAMES * DIM];
                for band in 0..TIME_BATCH {
                    for frame in 0..FRAMES {
                        let source = (frame * BANDS + band_start + band) * DIM;
                        let destination = (band * FRAMES + frame) * DIM;
                        input[destination..destination + DIM]
                            .copy_from_slice(&features[source..source + DIM]);
                    }
                }
                let output = run_model(
                    &mut time_model,
                    &input,
                    &[TIME_BATCH as i64, FRAMES as i64, DIM as i64],
                    &[TIME_BATCH as i64, FRAMES as i64, DIM as i64],
                )?;
                for band in 0..TIME_BATCH {
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
            "[uta-openvino-worker] Harmony stage-major layer {}/{} frequency",
            layer + 1,
            DEPTH
        );
        let mut frequency_model = compile_paths(
            &mut compute_core,
            &frequency_paths.name,
            &frequency_paths.xml,
            &frequency_paths.bin,
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
                let mut input = vec![0.0; FREQUENCY_BATCH * BANDS * DIM];
                let count = valid * BANDS * DIM;
                let source = frame_start * BANDS * DIM;
                input[..count].copy_from_slice(&time_output[source..source + count]);
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

    let mut cpu_core = configured_core(DeviceType::CPU)?;
    let mut gathered_masks = vec![vec![0.0; FRAMES * GATHERED_WIDTH]; feature_chunks.len()];
    let mut width_offset = 0;
    for mask in &pipeline.masks {
        let mut mask_model = compile_paths(
            &mut cpu_core,
            &mask.paths.name,
            &mask.paths.xml,
            &mask.paths.bin,
            DeviceType::CPU,
        )?;
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
        return Err("Harmony split mask groups do not cover the gathered spectrum".to_string());
    }
    Ok(gathered_masks)
}

pub(crate) fn infer_pcm(
    audio: &[f32],
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<Vec<f32>, String> {
    let device = crate::runtime::inference_device(config)?;
    if config
        .get("input_semantics")
        .and_then(|value| value.as_str())
        != Some("all_vocals")
        || config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("lead_vocal+backing_vocal_residual")
    {
        return Err("Harmony split requires explicit lead/residual semantics".to_string());
    }
    let directory = config
        .get("model_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "Harmony split generation path is missing".to_string())?;
    progress(0.01, "Validating exact-context Harmony split generation");
    let manifest = validate_manifest(&directory)?;
    progress(0.02, "Preparing phase-separated Harmony IR topology");
    let pipeline = prepare_pipeline(&directory, &manifest, device.openvino())?;
    super::melband_roformer_denoise::process_audio_staged(
        audio,
        FRAMES,
        CHUNK_SAMPLES,
        OVERLAP,
        |gathered_chunks| run_pipeline(&pipeline, gathered_chunks, device.openvino()),
        |fraction, message| progress(0.04 + fraction * 0.92, message),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_topology_preserves_full_time_context() {
        assert_eq!(CHUNK_SAMPLES, (FRAMES - 1) * 441);
        assert_eq!(BANDS % TIME_BATCH, 0);
        assert_eq!(
            MASK_GROUPS.iter().map(|(_, _, width)| width).sum::<usize>(),
            GATHERED_WIDTH
        );
        assert_eq!(expected_islands().len(), 21);
    }
}
