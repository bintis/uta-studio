use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{ROSVOT_CONVERSION_RECIPE_SHA256, STARS_CONVERSION_RECIPE_SHA256};

const OPENVINO_RUNTIME_COMMIT: &str = "8a17657b995fd3b4a52f8484acfcf2bb61214623";
const STARS_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
const STARS_G2P_PROFILE: &str = "stars-chinese-g2p-pypinyin-0.55.0-v1";
const ROSVOT_COMMIT: &str = "3c8332bf43adae35f6e4d64971862f2f6139b310";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    #[serde(default, rename = "source_manifest_sha256")]
    _source_manifest_sha256: Option<String>,
    #[serde(rename = "checkpoint_sha256")]
    _checkpoint_sha256: String,
    #[serde(rename = "config_sha256")]
    _config_sha256: String,
    frame_bucket: usize,
    note_bucket: usize,
    #[serde(default)]
    phoneme_bucket: Option<usize>,
    segmentation: Segmentation,
    shared_frontend: SharedFrontend,
    #[serde(default)]
    g2p_profile: Option<String>,
    #[serde(default, rename = "g2p_asset_sha256")]
    _g2p_asset_sha256: Option<String>,
    #[serde(default)]
    word_boundary_source: Option<String>,
    #[serde(default)]
    rwbd_included: Option<bool>,
    #[serde(default)]
    global_step: Option<u64>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    technique: Option<TechniqueManifest>,
    #[serde(default)]
    style: Option<StyleManifest>,
    graphs: Vec<String>,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TechniqueManifest {
    taxonomy: Vec<String>,
    raw_score_projection: String,
    calibration: String,
    scope: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StyleManifest {
    scope: String,
    heads: Vec<String>,
    calibration: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Segmentation {
    policy: String,
    frame_step_num: u32,
    frame_step_den: u32,
    unconditioned_frames: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SharedFrontend {
    profile: String,
    manifest: String,
    #[serde(rename = "manifest_sha256")]
    _manifest_sha256: String,
    #[serde(rename = "annotation_rmvpe_sha256")]
    _annotation_rmvpe_sha256: String,
}

pub(super) fn import_advanced_note_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let conversion_digest = expected_conversion_identity(resource)?;
    validate_catalog(model, resource)?;
    let manifest_path = source.join("manifest.json");
    require_confined_regular(source, Path::new("manifest.json"), resource)?;
    let bytes = std::fs::read(&manifest_path).map_err(publish_io)?;
    let manifest = validate_manifest(resource, &bytes)?;

    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for relative in manifest.files.keys() {
        let relative = PathBuf::from(relative);
        require_confined_regular(source, &relative, resource)?;
        let path = source.join(&relative);
        files.push((path, relative));
    }
    publish_file_set(
        manager.paths(),
        resource,
        &manager.catalog().catalog_version,
        &files,
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: model.source.sha256.clone(),
            model_recipe_digest: Some(model.recipe_digest.clone()),
            conversion_recipe_digest: Some(conversion_digest.to_string()),
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}

fn expected_conversion_identity(resource: &ResourceRef) -> RuntimeManagerResult<&'static str> {
    match resource.id.as_str() {
        "stars" => Ok(STARS_CONVERSION_RECIPE_SHA256),
        "rosvot" => Ok(ROSVOT_CONVERSION_RECIPE_SHA256),
        _ => Err(invalid(resource, "resource is not an advanced-note expert")),
    }
}

fn validate_catalog(model: &ModelCatalogEntry, resource: &ResourceRef) -> RuntimeManagerResult<()> {
    let converted = model
        .source
        .converted_artifact
        .as_ref()
        .ok_or_else(|| invalid(resource, "converted advanced-note identity is missing"))?;
    let expected_format = if resource.id == "stars" {
        "openvino_ir_v11_conditioned_segmented_p1"
    } else {
        "openvino_ir_v11_conditioned_segmented"
    };
    if converted.format != expected_format
        || converted.manifest_filename != "manifest.json"
        || converted.runtime_id != "openvino_2026_3"
        || converted.runtime_version != "2026.3.0"
        || converted.runtime_commit != OPENVINO_RUNTIME_COMMIT
    {
        return Err(invalid(
            resource,
            "advanced-note source, converted, or runtime identity is incompatible",
        ));
    }
    Ok(())
}

fn validate_manifest(resource: &ResourceRef, bytes: &[u8]) -> RuntimeManagerResult<Manifest> {
    let manifest: Manifest = serde_json::from_slice(bytes).map_err(|error| {
        invalid(
            resource,
            &format!("advanced-note manifest is invalid: {error}"),
        )
    })?;
    let common = manifest.model_id == resource.id
        && manifest.frame_bucket == 256
        && manifest.note_bucket == 32
        && manifest.segmentation.policy == "timed-transcript-fixed-256-v1"
        && manifest.segmentation.frame_step_num == 128
        && manifest.segmentation.frame_step_den == 24_000
        && manifest.segmentation.unconditioned_frames == "no_claim"
        && manifest.shared_frontend.profile == "shared-singing-frontend-24k-v1"
        && manifest.shared_frontend.manifest == "shared/manifest.json";
    let specific = if resource.id == "stars" {
        manifest.schema_version == 2
            && manifest.format == "openvino_ir_v11_conditioned_segmented_p1"
            && manifest.source_revision == STARS_COMMIT
            && manifest.g2p_profile.as_deref() == Some(STARS_G2P_PROFILE)
            && manifest.word_boundary_source.is_none()
            && manifest.rwbd_included.is_none()
            && manifest.phoneme_bucket == Some(256)
            && manifest.global_step == Some(200_000)
            && manifest.capabilities == ["notes.stars", "technique.analyze"]
            && manifest.technique.as_ref().is_some_and(|technique| {
                technique.taxonomy
                    == [
                        "bubble",
                        "breathe",
                        "pharyngeal",
                        "vibrato",
                        "glissando",
                        "mixed",
                        "falsetto",
                        "weak",
                        "strong",
                    ]
                    && technique.raw_score_projection == "sigmoid"
                    && technique.calibration == "source_local_uncalibrated"
                    && technique.scope == "phoneme"
            })
            && manifest.style.as_ref().is_some_and(|style| {
                style.scope == "segment_global"
                    && style.heads
                        == [
                            "technique_group",
                            "language",
                            "gender",
                            "emotion",
                            "method",
                            "pace",
                            "range",
                        ]
                    && style.calibration == "uncalibrated_logits"
            })
            && manifest.graphs == ["stage-a", "stage-b", "stage-c", "stage-d", "stage-e"]
    } else {
        manifest.schema_version == 1
            && manifest.format == "openvino_ir_v11_conditioned_segmented"
            && manifest.phoneme_bucket.is_none()
            && manifest.global_step.is_none()
            && manifest.capabilities.is_empty()
            && manifest.technique.is_none()
            && manifest.style.is_none()
            && manifest.source_revision == ROSVOT_COMMIT
            && manifest.g2p_profile.is_none()
            && manifest.word_boundary_source.as_deref() == Some("timed_transcript")
            && manifest.rwbd_included == Some(false)
            && manifest.graphs == ["frame", "pitch"]
    };
    let expected = expected_files(&resource.id);
    if !common
        || !specific
        || manifest.files.len() != expected.len()
        || !expected
            .iter()
            .all(|name| manifest.files.contains_key(*name))
        || manifest.files.keys().any(|path| !safe_relative(path))
    {
        return Err(invalid(
            resource,
            "advanced-note manifest contract or file set is incompatible",
        ));
    }
    Ok(manifest)
}

fn expected_files(model_id: &str) -> BTreeSet<&'static str> {
    let mut files = BTreeSet::from([
        "shared/annotation-rmvpe-t256.bin",
        "shared/annotation-rmvpe-t256.onnx",
        "shared/annotation-rmvpe-t256.xml",
        "shared/manifest.json",
    ]);
    if model_id == "stars" {
        files.extend([
            "stars-stage-a-t256-n32.bin",
            "stars-stage-a-t256-n32.xml",
            "stars-stage-b-t256-n32.bin",
            "stars-stage-b-t256-n32.xml",
            "stars-stage-c-t256-n32.bin",
            "stars-stage-c-t256-n32.xml",
            "stars-stage-d-t256-n32.bin",
            "stars-stage-d-t256-n32.xml",
            "stars-stage-e-t256-n32.bin",
            "stars-stage-e-t256-n32.xml",
        ]);
    } else {
        files.extend([
            "rosvot-frame-t256-n32.bin",
            "rosvot-frame-t256-n32.xml",
            "rosvot-pitch-t256-n32.bin",
            "rosvot-pitch-t256-n32.xml",
        ]);
    }
    files
}

fn require_confined_regular(
    root: &Path,
    relative: &Path,
    resource: &ResourceRef,
) -> RuntimeManagerResult<()> {
    if !safe_relative_path(relative) {
        return Err(invalid(resource, "advanced-note package path is unsafe"));
    }
    let mut current = root.to_path_buf();
    let count = relative.components().count();
    for (index, component) in relative.components().enumerate() {
        let Component::Normal(component) = component else {
            return Err(invalid(resource, "advanced-note package path is unsafe"));
        };
        current.push(component);
        let metadata = std::fs::symlink_metadata(&current)
            .map_err(|_| invalid(resource, "advanced-note package file is missing"))?;
        if metadata.file_type().is_symlink()
            || (index + 1 == count && !metadata.file_type().is_file())
            || (index + 1 < count && !metadata.file_type().is_dir())
        {
            return Err(invalid(
                resource,
                "advanced-note package contains a symlink or wrong file type",
            ));
        }
    }
    Ok(())
}

fn safe_relative(value: &str) -> bool {
    safe_relative_path(Path::new(value))
}

fn safe_relative_path(value: &Path) -> bool {
    !value.as_os_str().is_empty()
        && !value.is_absolute()
        && value
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn invalid(resource: &ResourceRef, message: &str) -> RuntimeManagerError {
    RuntimeManagerError::new("source_identity_mismatch", message).with_resource(resource)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_paths_are_allowed_but_traversal_is_not() {
        assert!(safe_relative("shared/manifest.json"));
        assert!(safe_relative("model.xml"));
        assert!(!safe_relative("../model.xml"));
        assert!(!safe_relative("/model.xml"));
    }

    #[test]
    fn exact_file_sets_keep_shared_frontend_correlated() {
        assert_eq!(expected_files("stars").len(), 14);
        assert_eq!(expected_files("rosvot").len(), 8);
        assert!(expected_files("stars").contains("shared/manifest.json"));
        assert!(expected_files("rosvot").contains("shared/manifest.json"));
    }
}
