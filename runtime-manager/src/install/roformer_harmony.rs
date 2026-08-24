use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    ROFORMER_HARMONY_CONVERSION_RECIPE_SHA256, ROFORMER_HARMONY_SOURCE_SHA256,
};

const DEPTH: usize = 6;

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    resource: String,
    capability: String,
    semantic_output: String,
    semantic_contract: SemanticContract,
    backend: String,
    source: SourceIdentity,
    exact_contract: ExactContract,
    islands: Vec<IslandIdentity>,
}

#[derive(Deserialize)]
struct SemanticContract {
    input: String,
    primary: String,
    residual: String,
    residual_formula: String,
}

#[derive(Deserialize)]
struct SourceIdentity {
    #[serde(rename = "checkpoint_sha256")]
    _checkpoint_sha256: String,
    #[serde(rename = "config_sha256")]
    _config_sha256: String,
    checkpoint_license: String,
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
    #[serde(rename = "sha256")]
    _sha256: String,
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
    let mut result = vec![("band-split".to_string(), "band", "CPU", None, None, None)];
    for layer in 0..DEPTH {
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
    for (start, end) in [
        (0, 8),
        (8, 16),
        (16, 24),
        (24, 32),
        (32, 40),
        (40, 48),
        (48, 56),
        (56, 60),
    ] {
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

fn verified_file(
    resource: &ResourceRef,
    source: &Path,
    identity: &FileIdentity,
) -> RuntimeManagerResult<(PathBuf, PathBuf)> {
    if Path::new(&identity.filename)
        .file_name()
        .and_then(|value| value.to_str())
        != Some(identity.filename.as_str())
    {
        return Err(invalid(
            resource,
            "Harmony manifest contains an unsafe filename",
        ));
    }
    let path = source.join(&identity.filename);
    let metadata = std::fs::symlink_metadata(&path).map_err(publish_io)?;
    if !metadata.file_type().is_file() || metadata.len() != identity.bytes {
        return Err(invalid(
            resource,
            format!("Harmony file identity mismatch: {}", identity.filename),
        ));
    }
    Ok((path, PathBuf::from(&identity.filename)))
}

pub(super) fn import_roformer_harmony_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let converted = model
        .source
        .converted_artifact
        .as_ref()
        .ok_or_else(|| invalid(resource, "Harmony converted artifact identity is missing"))?;
    if resource.id != "melband_roformer_harmony"
        || model.source.revision.as_deref() != Some("all_public_uvr_models")
        || converted.format != "openvino_ir_v11_explicit_cpu_gpu_islands_dual_residual"
        || converted.manifest_filename != "manifest.json"
        || converted.runtime_id != "openvino_2026_3"
    {
        return Err(invalid(
            resource,
            "Harmony source, conversion and runtime identities are not independently pinned",
        ));
    }

    let manifest_path = source.join("manifest.json");
    let manifest: Manifest =
        serde_json::from_slice(&std::fs::read(&manifest_path).map_err(publish_io)?)
            .map_err(|error| invalid(resource, format!("Harmony manifest is invalid: {error}")))?;
    let contract = &manifest.exact_contract;
    if manifest.schema_version != 2
        || manifest.resource != "model:melband_roformer_harmony"
        || manifest.capability != "audio.lead_isolate"
        || manifest.semantic_output != "lead_vocal+backing_vocal_residual"
        || manifest.semantic_contract.input != "all_vocals"
        || manifest.semantic_contract.primary != "lead_vocal"
        || manifest.semantic_contract.residual != "vocal_residual"
        || manifest.semantic_contract.residual_formula != "all_vocals_minus_lead_vocal"
        || manifest.backend != "OpenVINO FP32 explicit CPU/GPU split IR"
        || manifest.source.checkpoint_license != "unresolved"
        || contract.sample_rate != 44_100
        || contract.channels != 2
        || contract.chunk_samples != 352_800
        || contract.frames != 801
        || contract.hop_length != 441
        || contract.overlap != 4
        || contract.bands != 60
        || contract.feature_dim != 384
        || contract.gathered_width != 7_916
        || contract.time_microbatch != 10
        || contract.frequency_microbatch != 64
        || !contract.full_time_context_preserved
        || !contract.rolling_gpu_residency
    {
        return Err(invalid(
            resource,
            "Harmony exact-context split topology contract is invalid",
        ));
    }

    let expected = expected_islands();
    if manifest.islands.len() != expected.len() {
        return Err(invalid(resource, "Harmony split island count mismatch"));
    }
    let config_path = source.join("config.yaml");
    if !config_path.is_file() {
        return Err(invalid(resource, "Harmony config is unavailable"));
    }
    let mut files = vec![
        (manifest_path, PathBuf::from("manifest.json")),
        (config_path, PathBuf::from("config.yaml")),
    ];
    for (island, (name, kind, device, layer, start, end)) in manifest.islands.iter().zip(expected) {
        if island.name != name
            || island.kind != kind
            || island.device != device
            || island.layer != layer
            || island.start != start
            || island.end != end
            || island.xml.filename != format!("harmony-{name}.xml")
            || island.bin.filename != format!("harmony-{name}.bin")
        {
            return Err(invalid(
                resource,
                format!("Harmony island contract mismatch: {}", island.name),
            ));
        }
        files.push(verified_file(resource, source, &island.xml)?);
        files.push(verified_file(resource, source, &island.bin)?);
    }

    publish_file_set(
        manager.paths(),
        resource,
        &manager.catalog().catalog_version,
        &files,
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: Some(ROFORMER_HARMONY_SOURCE_SHA256.to_string()),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(ROFORMER_HARMONY_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}

fn invalid(resource: &ResourceRef, message: impl Into<String>) -> RuntimeManagerError {
    RuntimeManagerError::new("source_identity_mismatch", message.into()).with_resource(resource)
}
