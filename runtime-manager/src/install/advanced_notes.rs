use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

use super::{PublishIdentity, publish_file_set, publish_io, sha256_file};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{
    OPENVINO_WORKER_RECIPE_SHA256, ROSVOT_CONVERSION_RECIPE_SHA256, ROSVOT_IR_MANIFEST_SHA256,
    STARS_CONVERSION_RECIPE_SHA256, STARS_IR_MANIFEST_SHA256,
};

const OPENVINO_RUNTIME_COMMIT: &str = "8a17657b995fd3b4a52f8484acfcf2bb61214623";
const SHARED_MANIFEST_SHA256: &str =
    "986327618f2055873a98fca481893db83ffff2e386b6c522532a5272a1597a2c";
const ANNOTATION_RMVPE_SHA256: &str =
    "19dc1809cf4cdb0a18db93441816bc327e14e5644b72eeaae5220560c6736fe2";
const STARS_COMMIT: &str = "f0e43e96cfe953f71a6cf9efd8b908b2c9d7e167";
const STARS_CHECKPOINT: &str = "9159dd37516918448b0815ed86e1e3976d39c3044117da78db0ef65d1941db3c";
const STARS_CONFIG: &str = "01e8a495ba2e47b47b21fccda8db2605c85ec76cdaae258768d10a459e4e7e91";
const STARS_G2P_PROFILE: &str = "stars-chinese-g2p-pypinyin-0.55.0-v1";
const STARS_G2P_SHA256: &str = "289fcbcddfa8e5a1a911419af48ef36ddc08736aef7818e2c9321bdb331a94cc";
const ROSVOT_COMMIT: &str = "3c8332bf43adae35f6e4d64971862f2f6139b310";
const ROSVOT_SOURCE_MANIFEST: &str =
    "5ee3fe4d8f166da11ab0f1fbbc67fbd37e4ab906544d504876c7ebb60b0b32c8";
const ROSVOT_CHECKPOINT: &str = "7501fb5f913d971c2f51bcb3063b930027b03206581820a4d2bfdc394c9c3fcb";
const ROSVOT_CONFIG: &str = "2ad2cb756623418c471b7dc2f56175cce88b69a70b4a2c354fa1a78525aa54e2";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    #[serde(default)]
    source_manifest_sha256: Option<String>,
    checkpoint_sha256: String,
    config_sha256: String,
    frame_bucket: usize,
    note_bucket: usize,
    segmentation: Segmentation,
    shared_frontend: SharedFrontend,
    #[serde(default)]
    g2p_profile: Option<String>,
    #[serde(default)]
    g2p_asset_sha256: Option<String>,
    #[serde(default)]
    word_boundary_source: Option<String>,
    #[serde(default)]
    rwbd_included: Option<bool>,
    graphs: Vec<String>,
    files: BTreeMap<String, String>,
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
    manifest_sha256: String,
    annotation_rmvpe_sha256: String,
}

pub(super) fn import_advanced_note_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    let (manifest_digest, conversion_digest) = expected_catalog_identity(resource)?;
    validate_catalog(model, manifest_digest, conversion_digest, resource)?;
    let manifest_path = source.join("manifest.json");
    require_confined_regular(source, Path::new("manifest.json"), resource)?;
    if sha256_file(&manifest_path).ok().as_deref() != Some(manifest_digest) {
        return Err(invalid(
            resource,
            "advanced-note manifest hash is not pinned",
        ));
    }
    let bytes = std::fs::read(&manifest_path).map_err(publish_io)?;
    let manifest = validate_manifest(resource, &bytes)?;

    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for (relative, digest) in &manifest.files {
        let relative = PathBuf::from(relative);
        require_confined_regular(source, &relative, resource)?;
        let path = source.join(&relative);
        if sha256_file(&path).ok().as_deref() != Some(digest) {
            return Err(invalid(resource, "advanced-note IR file hash mismatch"));
        }
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

fn expected_catalog_identity(
    resource: &ResourceRef,
) -> RuntimeManagerResult<(&'static str, &'static str)> {
    match resource.id.as_str() {
        "stars" => Ok((STARS_IR_MANIFEST_SHA256, STARS_CONVERSION_RECIPE_SHA256)),
        "rosvot" => Ok((ROSVOT_IR_MANIFEST_SHA256, ROSVOT_CONVERSION_RECIPE_SHA256)),
        _ => Err(invalid(resource, "resource is not an advanced-note expert")),
    }
}

fn validate_catalog(
    model: &ModelCatalogEntry,
    manifest_digest: &str,
    conversion_digest: &str,
    resource: &ResourceRef,
) -> RuntimeManagerResult<()> {
    let converted = model
        .source
        .converted_artifact
        .as_ref()
        .ok_or_else(|| invalid(resource, "converted advanced-note identity is missing"))?;
    if converted.format != "openvino_ir_v11_conditioned_segmented"
        || converted.manifest_filename != "manifest.json"
        || converted.manifest_sha256 != manifest_digest
        || converted.conversion_recipe_sha256 != conversion_digest
        || converted.runtime_id != "openvino_2026_3"
        || converted.runtime_version != "2026.3.0"
        || converted.runtime_commit != OPENVINO_RUNTIME_COMMIT
        || model.runtime_recipe_digest.as_deref() != Some(OPENVINO_WORKER_RECIPE_SHA256)
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
    let common = manifest.schema_version == 1
        && manifest.model_id == resource.id
        && manifest.format == "openvino_ir_v11_conditioned_segmented"
        && manifest.frame_bucket == 256
        && manifest.note_bucket == 32
        && manifest.segmentation.policy == "timed-transcript-fixed-256-v1"
        && manifest.segmentation.frame_step_num == 128
        && manifest.segmentation.frame_step_den == 24_000
        && manifest.segmentation.unconditioned_frames == "no_claim"
        && manifest.shared_frontend.profile == "shared-singing-frontend-24k-v1"
        && manifest.shared_frontend.manifest == "shared/manifest.json"
        && manifest.shared_frontend.manifest_sha256 == SHARED_MANIFEST_SHA256
        && manifest.shared_frontend.annotation_rmvpe_sha256 == ANNOTATION_RMVPE_SHA256;
    let specific = if resource.id == "stars" {
        manifest.source_revision == STARS_COMMIT
            && manifest.source_manifest_sha256.is_none()
            && manifest.checkpoint_sha256 == STARS_CHECKPOINT
            && manifest.config_sha256 == STARS_CONFIG
            && manifest.g2p_profile.as_deref() == Some(STARS_G2P_PROFILE)
            && manifest.g2p_asset_sha256.as_deref() == Some(STARS_G2P_SHA256)
            && manifest.word_boundary_source.is_none()
            && manifest.rwbd_included.is_none()
            && manifest.graphs == ["stage-a", "stage-b", "stage-c"]
    } else {
        manifest.source_revision == ROSVOT_COMMIT
            && manifest.source_manifest_sha256.as_deref() == Some(ROSVOT_SOURCE_MANIFEST)
            && manifest.checkpoint_sha256 == ROSVOT_CHECKPOINT
            && manifest.config_sha256 == ROSVOT_CONFIG
            && manifest.g2p_profile.is_none()
            && manifest.g2p_asset_sha256.is_none()
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
        || manifest
            .files
            .iter()
            .any(|(path, digest)| !safe_relative(path) || !valid_sha256(digest))
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

fn valid_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
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
        assert_eq!(expected_files("stars").len(), 10);
        assert_eq!(expected_files("rosvot").len(), 8);
        assert!(expected_files("stars").contains("shared/manifest.json"));
        assert!(expected_files("rosvot").contains("shared/manifest.json"));
    }
}
