use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io, sha256_file};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    RMVPE_CONVERSION_RECIPE_SHA256, RMVPE_IR_MANIFEST_SHA256, RMVPE_SOURCE_SHA256,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RmvpeIrManifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_onnx_sha256: String,
    runtime_recipe_sha256: String,
    input_frame_buckets: RmvpeFrameBuckets,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RmvpeFrameBuckets {
    minimum: usize,
    maximum: usize,
    step: usize,
    overlap: usize,
}

pub(super) fn import_rmvpe_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let converted = model.source.converted_artifact.as_ref().ok_or_else(|| {
        RuntimeManagerError::invalid_catalog("RMVPE converted artifact identity is missing")
            .with_resource(resource)
    })?;
    if model.source.filename.as_deref() != Some("rmvpe.onnx")
        || model.source.sha256.as_deref() != Some(RMVPE_SOURCE_SHA256)
        || model.source.source_format.as_deref() != Some("onnx")
        || converted.format != "openvino_ir_v11_bucketed"
        || converted.manifest_filename != "manifest.json"
        || converted.manifest_sha256 != RMVPE_IR_MANIFEST_SHA256
        || converted.conversion_recipe_sha256 != RMVPE_CONVERSION_RECIPE_SHA256
        || converted.runtime_id != "openvino_2026_3"
    {
        return Err(RuntimeManagerError::invalid_catalog(
            "RMVPE source and converted identities are not independently pinned",
        )
        .with_resource(resource));
    }
    let manifest_path = source.join(&converted.manifest_filename);
    let manifest_sha = sha256_file(&manifest_path).map_err(|error| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            format!("could not hash RMVPE IR manifest: {error}"),
        )
        .with_resource(resource)
    })?;
    if manifest_sha != RMVPE_IR_MANIFEST_SHA256 {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "RMVPE IR manifest does not match the pinned converted artifact",
        )
        .with_resource(resource));
    }
    let manifest: RmvpeIrManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(publish_io)?,
    )
    .map_err(|error| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            format!("RMVPE IR manifest is invalid: {error}"),
        )
        .with_resource(resource)
    })?;
    let buckets = &manifest.input_frame_buckets;
    if manifest.schema_version != 2
        || manifest.model_id != "rmvpe"
        || manifest.format != "openvino_ir_v11_bucketed"
        || manifest.source_onnx_sha256 != RMVPE_SOURCE_SHA256
        || manifest.runtime_recipe_sha256 != RMVPE_CONVERSION_RECIPE_SHA256
        || buckets.minimum != 32
        || buckets.maximum != 1_024
        || buckets.step != 32
        || buckets.overlap != 128
        || manifest.files.len() != 33
    {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "RMVPE IR conversion identity or bucket contract is invalid",
        )
        .with_resource(resource));
    }
    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for frames in (32..=1_024).step_by(32) {
        let name = format!("rmvpe-{frames:04}.xml");
        let expected = manifest.files.get(&name).ok_or_else(|| {
            RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("RMVPE IR manifest omitted {name}"),
            )
            .with_resource(resource)
        })?;
        let path = source.join(&name);
        if sha256_file(&path).ok().as_deref() != Some(expected) {
            return Err(RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("RMVPE IR file hash mismatch: {name}"),
            )
            .with_resource(resource));
        }
        files.push((path, PathBuf::from(name)));
    }
    let weights = source.join("rmvpe.bin");
    let expected_weights = manifest.files.get("rmvpe.bin").ok_or_else(|| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            "RMVPE IR manifest omitted rmvpe.bin",
        )
        .with_resource(resource)
    })?;
    if sha256_file(&weights).ok().as_deref() != Some(expected_weights) {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "RMVPE IR weights hash mismatch",
        )
        .with_resource(resource));
    }
    files.push((weights, PathBuf::from("rmvpe.bin")));
    publish_file_set(
        manager.paths(),
        resource,
        &manager.catalog().catalog_version,
        &files,
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: Some(RMVPE_SOURCE_SHA256.to_string()),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(RMVPE_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}
