use std::path::{Path, PathBuf};

use openvino::{CompiledModel, Core, DeviceType, ElementType, RwPropertyKey, Shape, Tensor};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const MODEL_ID: &str = "melband_roformer_inst_v2";
const MANIFEST_SHA256: &str = "683c16d852ec16ebc68679656622c2b6bfe75e55dd0201d9e2ccab8fb979d40c";
const CONFIG_SHA256: &str = "4b902a7360a930c178edb4846b30e4e326aa1219d1b2daf660d46a311e0cd50b";
const SOURCE_SHA256: &str = "bd19766620f7d6f58fdf7aaada7e89907fe41bc64490ce3faa9a6dab15d6e1f2";
const RECIPE_SHA256: &str = "1dfb93131898bbfb9197f0c0efb87314285aee27d03e3d94c83d1d8f1def5033";
const LAYOUT_INDICES_SHA256: &str =
    "c087bfc8e1a110a16a7aa998de5fe43b025ea08de0e4606c7b80e258b1ed5ecc";
const LAYOUT_COUNTS_SHA256: &str =
    "41947c540f2511f98bb2530176d9a9a3576e5a954135dbf5ef207247e0933683";
const FRAMES: usize = 1_101;
const CHUNK_SAMPLES: usize = 485_100;
const OVERLAP: usize = 2;
const BANDS: usize = 60;
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
    source: SourceIdentity,
    conversion_recipe: ConversionIdentity,
    exact_contract: ExactContract,
    layout: LayoutIdentity,
    islands: Vec<IslandIdentity>,
}

#[derive(Deserialize)]
struct SourceIdentity {
    checkpoint_sha256: String,
    config_sha256: String,
}

#[derive(Deserialize)]
struct ConversionIdentity {
    sha256: String,
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
struct LayoutIdentity {
    frequency_indices_sha256: String,
    bands_per_frequency_sha256: String,
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
    sha256: String,
}

struct MaskIsland {
    model: CompiledModel,
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
    core: Core,
    band: CompiledModel,
    layers: Vec<(LayerIsland, LayerIsland)>,
    masks: Vec<MaskIsland>,
}

fn sha256(path: &Path) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|error| error.to_string())?;
    let mut digest = Sha256::new();
    std::io::copy(&mut file, &mut digest).map_err(|error| error.to_string())?;
    Ok(format!("{:x}", digest.finalize()))
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
    for layer in 0..12 {
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
        return Err("Inst V2 island filename is not a local basename".to_string());
    }
    let path = directory.join(filename);
    let metadata = std::fs::metadata(&path).map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != identity.bytes || sha256(&path)? != identity.sha256
    {
        return Err(format!(
            "Inst V2 island identity mismatch: {}",
            identity.filename
        ));
    }
    Ok(path)
}

fn validate_manifest(directory: &Path) -> Result<Manifest, String> {
    let path = directory.join("manifest.json");
    if sha256(&path)? != MANIFEST_SHA256 || sha256(&directory.join("config.yaml"))? != CONFIG_SHA256
    {
        return Err("Inst V2 split generation identity is invalid".to_string());
    }
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Inst V2 split manifest is invalid: {error}"))?;
    let contract = &manifest.exact_contract;
    if manifest.schema_version != 2
        || manifest.resource != format!("model:{MODEL_ID}")
        || manifest.capability != "audio.extract_instrumental"
        || manifest.semantic_output != "instrumental"
        || manifest.source.checkpoint_sha256 != SOURCE_SHA256
        || manifest.source.config_sha256 != CONFIG_SHA256
        || manifest.conversion_recipe.sha256 != RECIPE_SHA256
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
        || manifest.layout.frequency_indices_sha256 != LAYOUT_INDICES_SHA256
        || manifest.layout.bands_per_frequency_sha256 != LAYOUT_COUNTS_SHA256
    {
        return Err("Inst V2 split manifest contract mismatch".to_string());
    }
    let expected = expected_islands();
    if manifest.islands.len() != expected.len() {
        return Err("Inst V2 split manifest island count mismatch".to_string());
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
                "Inst V2 split island order mismatch: {}",
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
                .ok_or_else(|| "Inst V2 XML path is not UTF-8".to_string())?,
            bin.to_str()
                .ok_or_else(|| "Inst V2 BIN path is not UTF-8".to_string())?,
        )
        .map_err(|error| format!("could not read Inst V2 {name} IR: {error}"))?;
    let device_name = device.to_string();
    core.compile_model(&graph, device)
        .map_err(|error| format!("could not compile Inst V2 {name} on {device_name}: {error}"))
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

fn compile_pipeline(directory: &Path, manifest: &Manifest) -> Result<Pipeline, String> {
    let _runtime_manifest_sha256 = crate::runtime::validate_runtime()?;
    let mut core = Core::new().map_err(|error| format!("OpenVINO is unavailable: {error}"))?;
    let devices = core
        .available_devices()
        .map_err(|error| error.to_string())?;
    for required in [DeviceType::CPU, DeviceType::GPU] {
        if !devices.contains(&required) {
            return Err(format!(
                "Inst V2 requires explicit OpenVINO {required}; fallback is forbidden"
            ));
        }
    }
    for device in [DeviceType::CPU, DeviceType::GPU] {
        core.set_properties(
            &device,
            [
                (RwPropertyKey::HintInferencePrecision, "f32"),
                (RwPropertyKey::HintExecutionMode, "ACCURACY"),
            ],
        )
        .map_err(|error| format!("could not configure OpenVINO {device}: {error}"))?;
    }
    crate::runtime::configure_low_impact_gpu_queue(&mut core)?;
    let band_paths = island_paths(directory, &manifest.islands[0])?;
    let band = compile_paths(
        &mut core,
        &band_paths.name,
        &band_paths.xml,
        &band_paths.bin,
        DeviceType::CPU,
    )?;
    let mut layers = Vec::with_capacity(12);
    for layer in 0..12 {
        let offset = 1 + layer * 2;
        layers.push((
            island_paths(directory, &manifest.islands[offset])?,
            island_paths(directory, &manifest.islands[offset + 1])?,
        ));
    }
    let mut masks = Vec::with_capacity(MASK_GROUPS.len());
    for (index, (start, end, output_width)) in MASK_GROUPS.into_iter().enumerate() {
        let paths = island_paths(directory, &manifest.islands[25 + index])?;
        masks.push(MaskIsland {
            model: compile_paths(
                &mut core,
                &paths.name,
                &paths.xml,
                &paths.bin,
                DeviceType::CPU,
            )?,
            start,
            end,
            output_width,
        });
    }
    Ok(Pipeline {
        core,
        band,
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
            "Inst V2 island returned unexpected shape: {dimensions:?}"
        ));
    }
    let values = output
        .get_data::<f32>()
        .map_err(|error| error.to_string())?
        .to_vec();
    if values.iter().any(|value| !value.is_finite()) {
        return Err("Inst V2 island returned non-finite values".to_string());
    }
    Ok(values)
}

fn run_pipeline(pipeline: &mut Pipeline, gathered: &[f32]) -> Result<Vec<f32>, String> {
    if gathered.len() != FRAMES * GATHERED_WIDTH {
        return Err("Inst V2 gathered STFT shape is invalid".to_string());
    }
    let mut features = run_model(
        &mut pipeline.band,
        gathered,
        &[1, FRAMES as i64, GATHERED_WIDTH as i64],
        &[1, FRAMES as i64, BANDS as i64, DIM as i64],
    )?;
    for (time_paths, frequency_paths) in &pipeline.layers {
        let mut time_model = compile_paths(
            &mut pipeline.core,
            &time_paths.name,
            &time_paths.xml,
            &time_paths.bin,
            DeviceType::GPU,
        )?;
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
        drop(time_model);
        let mut frequency_model = compile_paths(
            &mut pipeline.core,
            &frequency_paths.name,
            &frequency_paths.xml,
            &frequency_paths.bin,
            DeviceType::GPU,
        )?;
        let mut frequency_output = vec![0.0; features.len()];
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
        drop(frequency_model);
        features = frequency_output;
    }
    let mut gathered_mask = vec![0.0; FRAMES * GATHERED_WIDTH];
    let mut width_offset = 0;
    for mask in &mut pipeline.masks {
        let bands = mask.end - mask.start;
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
            &mut mask.model,
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
        width_offset += mask.output_width;
    }
    if width_offset != GATHERED_WIDTH {
        return Err("Inst V2 mask groups do not cover the gathered spectrum".to_string());
    }
    Ok(gathered_mask)
}

pub fn infer(
    audio: &[f32],
    output_dir: &Path,
    config: &serde_json::Value,
    mut progress: impl FnMut(f32, &str),
) -> Result<PathBuf, String> {
    if config.get("backend").and_then(|value| value.as_str()) != Some("openvino_gpu")
        || config
            .get("semantic_output")
            .and_then(|value| value.as_str())
            != Some("instrumental")
    {
        return Err(
            "Inst V2 requires explicit OpenVINO GPU and instrumental semantics".to_string(),
        );
    }
    let directory = config
        .get("model_path")
        .and_then(|value| value.as_str())
        .map(PathBuf::from)
        .ok_or_else(|| "Inst V2 split generation path is missing".to_string())?;
    progress(0.01, "Validating exact-context Inst V2 split generation");
    let manifest = validate_manifest(&directory)?;
    progress(0.02, "Compiling explicit Inst V2 CPU/GPU IR topology");
    let mut pipeline = compile_pipeline(&directory, &manifest)?;
    let output = super::melband_roformer_denoise::process_audio(
        audio,
        FRAMES,
        CHUNK_SAMPLES,
        OVERLAP,
        |gathered| run_pipeline(&mut pipeline, gathered),
        |fraction, message| progress(0.04 + fraction * 0.92, message),
    )?;
    progress(0.97, "Atomically encoding exact-context instrumental stem");
    crate::audio::encode_stereo_flac(&output, output_dir, "instrumental.flac")
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
        assert_eq!(expected_islands().len(), 33);
    }
}
