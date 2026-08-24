use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io, sha256_file};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    GAME_CONVERSION_RECIPE_SHA256, GAME_IR_MANIFEST_SHA256, GAME_SOURCE_ASSET_SHA256,
};

const GAME_SOURCE_COMMIT: &str = "475a8ee781fe8cca980b3b12fbe6c80c768a813a";
const GAME_SOURCE_ASSET: &str = "GAME-1.0.3-medium-onnx.zip";
const GAME_VARIANT: &str = "GAME-1.0.3-medium-onnx";
const GAME_LICENSE: &str = "CC-BY-NC-SA-4.0";
// Immutable GAME IR generation identity. The current worker recipe may add
// unrelated model routes without changing this already-validated conversion.
const GAME_IR_RUNTIME_RECIPE_SHA256: &str =
    "bd349389e6d0d0b742ae103892c1e5774599dd8733460aec80cb74bcf20ddab6";
const ESTIMATOR_NOTE_BUCKETS: [usize; 6] = [32, 64, 128, 256, 512, 1_024];
const EXPECTED_FILES: [&str; 12] = [
    "config.json",
    "encoder.xml",
    "encoder.bin",
    "segmenter.xml",
    "segmenter.bin",
    "estimator.bin",
    "estimator-0032.xml",
    "estimator-0064.xml",
    "estimator-0128.xml",
    "estimator-0256.xml",
    "estimator-0512.xml",
    "estimator-1024.xml",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GameIrManifest {
    schema_version: u32,
    model_id: String,
    variant: String,
    format: String,
    source_repository: String,
    source_commit: String,
    source_release: String,
    source_asset: String,
    source_asset_url: String,
    source_asset_sha256: String,
    model_license: String,
    runtime_recipe_sha256: String,
    sample_rate: u32,
    timestep_seconds: f64,
    chunk_samples: usize,
    chunk_frames: usize,
    chunk_overlap_samples: usize,
    d3pm_steps: usize,
    boundary_threshold: f32,
    boundary_radius_frames: usize,
    note_presence_threshold: f32,
    estimator_note_buckets: Vec<usize>,
    files: BTreeMap<String, String>,
}

pub(super) fn import_game_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let manifest_path = source.join("manifest.json");
    let manifest_sha = sha256_file(&manifest_path).map_err(|error| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            format!("could not hash GAME IR manifest: {error}"),
        )
        .with_resource(resource)
    })?;
    if manifest_sha != GAME_IR_MANIFEST_SHA256 {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "GAME IR manifest does not match the pinned converted artifact",
        )
        .with_resource(resource));
    }
    let manifest: GameIrManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(publish_io)?,
    )
    .map_err(|error| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            format!("GAME IR manifest is invalid: {error}"),
        )
        .with_resource(resource)
    })?;
    if manifest.schema_version != 2
        || manifest.model_id != "game"
        || manifest.variant != GAME_VARIANT
        || manifest.format != "openvino_ir_v11_static_chunked_estimator_buckets"
        || manifest.source_repository != "https://github.com/openvpi/GAME.git"
        || manifest.source_commit != GAME_SOURCE_COMMIT
        || manifest.source_release != "v1.0.3"
        || manifest.source_asset != GAME_SOURCE_ASSET
        || manifest.source_asset_url
            != "https://github.com/openvpi/GAME/releases/download/v1.0.3/GAME-1.0.3-medium-onnx.zip"
        || manifest.source_asset_sha256 != GAME_SOURCE_ASSET_SHA256
        || manifest.model_license != GAME_LICENSE
        || manifest.runtime_recipe_sha256 != GAME_IR_RUNTIME_RECIPE_SHA256
        || manifest.sample_rate != 44_100
        || manifest.timestep_seconds != 0.01
        || manifest.chunk_samples != 1_323_000
        || manifest.chunk_frames != 3_000
        || manifest.chunk_overlap_samples != 88_200
        || manifest.d3pm_steps != 8
        || manifest.boundary_threshold != 0.2
        || manifest.boundary_radius_frames != 2
        || manifest.note_presence_threshold != 0.2
        || manifest.estimator_note_buckets != ESTIMATOR_NOTE_BUCKETS
        || manifest.files.len() != EXPECTED_FILES.len()
    {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "GAME IR conversion identity or inference contract is invalid",
        )
        .with_resource(resource));
    }

    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for name in EXPECTED_FILES {
        let expected = manifest.files.get(name).ok_or_else(|| {
            RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("GAME IR manifest omitted {name}"),
            )
            .with_resource(resource)
        })?;
        let path = source.join(name);
        if sha256_file(&path).ok().as_deref() != Some(expected) {
            return Err(RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("GAME IR file hash mismatch: {name}"),
            )
            .with_resource(resource));
        }
        files.push((path, PathBuf::from(name)));
    }

    publish_file_set(
        manager.paths(),
        resource,
        &manager.catalog().catalog_version,
        &files,
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: Some(manifest_sha),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(GAME_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}
