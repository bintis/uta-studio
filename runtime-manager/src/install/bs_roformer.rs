use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io, sha256_file};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    BS_ROFORMER_CONFIG_SHA256, BS_ROFORMER_CONVERSION_RECIPE_SHA256,
    BS_ROFORMER_IR_MANIFEST_SHA256, BS_ROFORMER_SOURCE_SHA256, OPENVINO_WORKER_RECIPE_SHA256,
};

#[derive(Deserialize)]
struct Manifest {
    schema_version: u32,
    resource: String,
    capability: String,
    semantic_output: String,
    source: SourceIdentity,
    conversion_recipe: ConversionIdentity,
    exact_contract: ExactContract,
    topology: Topology,
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
    overlap: usize,
    time_microbatch: usize,
    frequency_microbatch: usize,
    full_time_context_preserved: bool,
}

#[derive(Deserialize)]
struct Topology {
    fallback_forbidden: bool,
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

fn invalid(resource: &ResourceRef, message: impl Into<String>) -> RuntimeManagerError {
    RuntimeManagerError::new("source_identity_mismatch", message.into()).with_resource(resource)
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
    result.push(("final-norm".to_string(), "norm", "CPU", None, None, None));
    for (start, end) in [
        (0, 8),
        (8, 16),
        (16, 24),
        (24, 32),
        (32, 40),
        (40, 48),
        (48, 56),
        (56, 62),
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
            "BS-RoFormer manifest contains an unsafe filename",
        ));
    }
    let path = source.join(&identity.filename);
    let metadata = std::fs::symlink_metadata(&path).map_err(publish_io)?;
    if !metadata.file_type().is_file()
        || metadata.len() != identity.bytes
        || sha256_file(&path).ok().as_deref() != Some(identity.sha256.as_str())
    {
        return Err(invalid(
            resource,
            format!("BS-RoFormer file identity mismatch: {}", identity.filename),
        ));
    }
    Ok((path, PathBuf::from(&identity.filename)))
}

pub(super) fn import_bs_roformer_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let converted = model.source.converted_artifact.as_ref().ok_or_else(|| {
        invalid(
            resource,
            "BS-RoFormer converted artifact identity is missing",
        )
    })?;
    if resource.id != "bs_roformer_vocals_ep317"
        || model.source.sha256.as_deref() != Some(BS_ROFORMER_SOURCE_SHA256)
        || model.source.revision.as_deref() != Some("all_public_uvr_models")
        || converted.format != "openvino_ir_v11_explicit_cpu_gpu_islands"
        || converted.manifest_filename != "manifest.json"
        || converted.manifest_sha256 != BS_ROFORMER_IR_MANIFEST_SHA256
        || converted.conversion_recipe_sha256 != BS_ROFORMER_CONVERSION_RECIPE_SHA256
        || converted.runtime_id != "openvino_2026_3"
        || model.runtime_recipe_digest.as_deref() != Some(OPENVINO_WORKER_RECIPE_SHA256)
    {
        return Err(invalid(
            resource,
            "BS-RoFormer source, conversion and runtime identities are not independently pinned",
        ));
    }
    let manifest_path = source.join("manifest.json");
    if sha256_file(&manifest_path).ok().as_deref() != Some(BS_ROFORMER_IR_MANIFEST_SHA256) {
        return Err(invalid(resource, "BS-RoFormer manifest identity mismatch"));
    }
    let manifest: Manifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(publish_io)?,
    )
    .map_err(|error| {
        invalid(
            resource,
            format!("BS-RoFormer manifest is invalid: {error}"),
        )
    })?;
    let contract = &manifest.exact_contract;
    if manifest.schema_version != 2
        || manifest.resource != "model:bs_roformer_vocals_ep317"
        || manifest.capability != "audio.extract_vocals"
        || manifest.semantic_output != "guide_vocals"
        || manifest.source.checkpoint_sha256 != BS_ROFORMER_SOURCE_SHA256
        || manifest.source.config_sha256 != BS_ROFORMER_CONFIG_SHA256
        || manifest.conversion_recipe.sha256 != BS_ROFORMER_CONVERSION_RECIPE_SHA256
        || contract.sample_rate != 44_100
        || contract.channels != 2
        || contract.chunk_samples != 352_800
        || contract.frames != 801
        || contract.overlap != 4
        || contract.time_microbatch != 8
        || contract.frequency_microbatch != 64
        || !contract.full_time_context_preserved
        || !manifest.topology.fallback_forbidden
    {
        return Err(invalid(
            resource,
            "BS-RoFormer exact-context topology contract is invalid",
        ));
    }
    let expected = expected_islands();
    if manifest.islands.len() != expected.len() {
        return Err(invalid(resource, "BS-RoFormer island count mismatch"));
    }
    let config_path = source.join("config.yaml");
    if sha256_file(&config_path).ok().as_deref() != Some(BS_ROFORMER_CONFIG_SHA256) {
        return Err(invalid(resource, "BS-RoFormer config identity mismatch"));
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
            || island.xml.filename != format!("bs-roformer-{name}.xml")
            || island.bin.filename != format!("bs-roformer-{name}.bin")
        {
            return Err(invalid(
                resource,
                format!("BS-RoFormer island contract mismatch: {}", island.name),
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
            source_sha256: Some(BS_ROFORMER_SOURCE_SHA256.to_string()),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(BS_ROFORMER_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}
