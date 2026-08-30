use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    ROFORMER_DENOISE_CONVERSION_RECIPE_SHA256, ROFORMER_DENOISE_SOURCE_SHA256,
};

const XML: &str = "melband-roformer-denoise-neural.xml";
const BIN: &str = "melband-roformer-denoise-neural.bin";

#[derive(Deserialize)]
struct ArtifactManifest {
    schema_version: u32,
    resource: String,
    capability: String,
    semantic_output: String,
    #[serde(rename = "config_sha256")]
    _config_sha256: String,
    source: SourceIdentity,
    conversion_recipe: ConversionIdentity,
    io: IoContract,
    files: BTreeMap<String, FileIdentity>,
}

#[derive(Deserialize)]
struct SourceIdentity {
    repository: String,
    revision: String,
    #[serde(rename = "sha256")]
    _sha256: String,
    #[serde(rename = "checkpoint_license")]
    _checkpoint_license: String,
}

#[derive(Deserialize)]
struct ConversionIdentity {
    #[serde(rename = "sha256")]
    _sha256: String,
    graph_boundary: String,
    dynamic_time_axis: bool,
    semantic_time_chunking: bool,
    precision: String,
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
    #[serde(rename = "sha256")]
    _sha256: String,
}

pub(super) fn import_roformer_denoise_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let converted = model.source.converted_artifact.as_ref().ok_or_else(|| {
        RuntimeManagerError::invalid_catalog("Denoise converted artifact identity is missing")
            .with_resource(resource)
    })?;
    if model.source.revision.as_deref() != Some("4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9")
        || converted.format != "openvino_ir_v11_melband_neural_island"
        || converted.manifest_filename != "manifest.json"
        || converted.runtime_id != "openvino_2026_3"
    {
        return Err(RuntimeManagerError::invalid_catalog(
            "Denoise source, conversion, and runtime identities are not independently pinned",
        )
        .with_resource(resource));
    }
    let manifest_path = source.join("manifest.json");
    let manifest: ArtifactManifest = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(publish_io)?,
    )
    .map_err(|error| {
        RuntimeManagerError::new(
            "source_identity_mismatch",
            format!("Denoise IR manifest is invalid: {error}"),
        )
        .with_resource(resource)
    })?;
    let exact_shape = [1, 801, 7_916];
    let tensor_matches = |tensor: &TensorContract, name: &str| {
        tensor.name == name && tensor.dtype == "f32" && tensor.exact_validation_shape == exact_shape
    };
    if manifest.schema_version != 1
        || manifest.resource != "model:melband_roformer_denoise_aufr33"
        || manifest.capability != "audio.denoise"
        || manifest.semantic_output != "dry"
        || manifest.source.repository != "poiqazwsx/melband-roformer-denoise"
        || manifest.source.revision != "4e39bc34a36dda8e73254cd8f5d44f15de2bd7b9"
        || manifest.conversion_recipe.graph_boundary != "band_split+transformers+mask_estimator"
        || !manifest.conversion_recipe.dynamic_time_axis
        || manifest.conversion_recipe.semantic_time_chunking
        || manifest.conversion_recipe.precision != "fp32"
        || !tensor_matches(&manifest.io.input, "gathered_stft")
        || !tensor_matches(&manifest.io.output, "gathered_mask")
    {
        return Err(RuntimeManagerError::new(
            "source_identity_mismatch",
            "Denoise IR semantic, tensor, or provenance contract is invalid",
        )
        .with_resource(resource));
    }

    let expected = ["config.yaml", XML, BIN];
    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for name in expected {
        let identity = manifest.files.get(name).ok_or_else(|| {
            RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("Denoise IR manifest omitted {name}"),
            )
            .with_resource(resource)
        })?;
        let path = source.join(name);
        let metadata = path.metadata().map_err(publish_io)?;
        if identity.bytes != metadata.len() || !metadata.is_file() {
            return Err(RuntimeManagerError::new(
                "source_identity_mismatch",
                format!("Denoise IR file is invalid: {name}"),
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
            source_sha256: Some(ROFORMER_DENOISE_SOURCE_SHA256.to_string()),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(ROFORMER_DENOISE_CONVERSION_RECIPE_SHA256.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}
