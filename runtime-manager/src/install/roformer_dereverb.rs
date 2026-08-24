use std::path::{Path, PathBuf};

use super::{PublishIdentity, publish_file_set, sha256_file};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    OPENVINO_WORKER_RECIPE_SHA256, ROFORMER_DEREVERB_BIN_SHA256, ROFORMER_DEREVERB_CONFIG_SHA256,
    ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256, ROFORMER_DEREVERB_IR_MANIFEST_SHA256,
    ROFORMER_DEREVERB_SOURCE_SHA256, ROFORMER_DEREVERB_XML_SHA256,
};

pub(super) fn import_roformer_dereverb_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let converted = model
        .source
        .converted_artifact
        .as_ref()
        .ok_or_else(|| invalid(resource, "Dereverb converted artifact identity is missing"))?;
    if resource.id != "melband_roformer_dereverb_anvuew"
        || model.source.sha256.as_deref() != Some(ROFORMER_DEREVERB_SOURCE_SHA256)
        || model.source.revision.as_deref() != Some("cef05ad2b5b3145ea5c149d3ad5d1f8439b34d06")
        || converted.format != "openvino_ir_v11_melband_neural_island"
        || converted.manifest_filename != "manifest.json"
        || converted.manifest_sha256 != ROFORMER_DEREVERB_IR_MANIFEST_SHA256
        || converted.conversion_recipe_sha256 != ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256
        || converted.runtime_id != "openvino_2026_3"
        || model.runtime_recipe_digest.as_deref() != Some(OPENVINO_WORKER_RECIPE_SHA256)
    {
        return Err(invalid(
            resource,
            "Dereverb source, conversion and runtime identities are not independently pinned",
        ));
    }
    let expected = [
        ("manifest.json", ROFORMER_DEREVERB_IR_MANIFEST_SHA256),
        ("config.yaml", ROFORMER_DEREVERB_CONFIG_SHA256),
        (
            "melband-roformer-dereverb-neural.xml",
            ROFORMER_DEREVERB_XML_SHA256,
        ),
        (
            "melband-roformer-dereverb-neural.bin",
            ROFORMER_DEREVERB_BIN_SHA256,
        ),
    ];
    let mut files = Vec::with_capacity(expected.len());
    for (name, digest) in expected {
        let path = source.join(name);
        let metadata = std::fs::symlink_metadata(&path).map_err(|error| {
            invalid(
                resource,
                &format!("Dereverb file is unavailable: {name}: {error}"),
            )
        })?;
        if !metadata.file_type().is_file() || sha256_file(&path).ok().as_deref() != Some(digest) {
            return Err(invalid(
                resource,
                &format!("Dereverb file identity mismatch: {name}"),
            ));
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
            source_sha256: Some(ROFORMER_DEREVERB_SOURCE_SHA256.to_string()),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(ROFORMER_DEREVERB_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}

fn invalid(resource: &ResourceRef, message: &str) -> RuntimeManagerError {
    RuntimeManagerError::new("source_identity_mismatch", message).with_resource(resource)
}
