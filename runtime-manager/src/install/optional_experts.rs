use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

#[cfg(test)]
use super::sha256_file;
use super::{PublishIdentity, publish_file_set, publish_io};
use crate::catalog::ModelCatalogEntry;
use crate::error::{RuntimeManagerError, RuntimeManagerResult};
use crate::resolver::RuntimeManager;
use crate::resource::ResourceRef;
use crate::runtime_lock::{BASIC_PITCH_SOURCE_SHA256, FCPE_SOURCE_SHA256};

const OPENVINO_RUNTIME_COMMIT: &str = "8a17657b995fd3b4a52f8484acfcf2bb61214623";
const FIRERED_REVISION: &str =
    "42ailab/FireRedASR2-AED-ONNX@13f950858934f7b6a0d3ce52bae65af0dc022258";
const FCPE_REVISION: &str = "gzivdo/fcpe-onnx@5800a2b1944967f55bb0bfeb9718cb749f809310";
const BASIC_PITCH_REVISION: &str =
    "AEmotionStudio/basic-pitch-onnx-models@327fd8ccd2f0bb84cbe56b4a0e9d318398ddf763";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FireRedManifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    #[serde(rename = "source_hashes")]
    _source_hashes: FireRedSourceHashes,
    fixture_contract: FireRedFixtureContract,
    files: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FireRedSourceHashes {
    #[serde(rename = "encoder")]
    _encoder: String,
    #[serde(rename = "decoder")]
    _decoder: String,
    #[serde(rename = "ctc")]
    _ctc: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FireRedFixtureContract {
    feature_frames: usize,
    encoder_frames: usize,
    decoder_cache_max: usize,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FixedWindowManifest {
    schema_version: u32,
    model_id: String,
    format: String,
    source_revision: String,
    #[serde(rename = "source_onnx_sha256")]
    _source_onnx_sha256: String,
    input_shape: [usize; 3],
    sample_rate: u32,
    files: BTreeMap<String, String>,
}

struct ValidatedManifest {
    files: BTreeMap<String, String>,
    source_sha256: Option<String>,
}

pub(super) fn import_optional_expert_ir_directory(
    manager: &RuntimeManager,
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
    source: &Path,
) -> RuntimeManagerResult<String> {
    validate_catalog_identity(resource, model)?;
    let converted = model.source.converted_artifact.as_ref().ok_or_else(|| {
        invalid_identity(resource, "converted OpenVINO artifact identity is missing")
    })?;
    let manifest_path = source.join(&converted.manifest_filename);
    require_regular_file(resource, &manifest_path, "IR manifest")?;
    let bytes = std::fs::read(&manifest_path).map_err(publish_io)?;
    let validated = match resource.id.as_str() {
        "firered_asr2_aed" => validate_firered_manifest(resource, &bytes)?,
        "fcpe" => validate_fixed_window_manifest(
            resource,
            &bytes,
            FCPE_REVISION,
            FCPE_SOURCE_SHA256,
            [1, 32_000, 1],
            16_000,
            &["fcpe.xml", "fcpe.bin"],
        )?,
        "basic_pitch" => validate_fixed_window_manifest(
            resource,
            &bytes,
            BASIC_PITCH_REVISION,
            BASIC_PITCH_SOURCE_SHA256,
            [1, 43_844, 1],
            22_050,
            &["basic-pitch.xml", "basic-pitch.bin"],
        )?,
        _ => {
            return Err(invalid_identity(
                resource,
                "resource is not an optional OpenVINO expert",
            ));
        }
    };

    let mut files = vec![(manifest_path, PathBuf::from("manifest.json"))];
    for name in validated.files.keys() {
        let relative = PathBuf::from(name);
        if relative.components().count() != 1 {
            return Err(invalid_identity(
                resource,
                "IR manifest contains an unsafe file path",
            ));
        }
        let path = source.join(&relative);
        require_regular_file(resource, &path, name)?;
        files.push((path, relative));
    }

    publish_file_set(
        manager.paths(),
        resource,
        &manager.catalog().catalog_version,
        &files,
        PublishIdentity {
            source: Some(model.source.clone()),
            source_sha256: validated.source_sha256,
            model_recipe_digest: Some(model.recipe_digest.clone()),
            // These historical fixed-window manifests do not record a
            // reproducible conversion recipe. Do not substitute another hash.
            conversion_recipe_digest: None,
            runtime_recipe_digest: model.runtime_recipe_digest.clone(),
        },
    )
}

fn validate_catalog_identity(
    resource: &ResourceRef,
    model: &ModelCatalogEntry,
) -> RuntimeManagerResult<()> {
    let converted = model.source.converted_artifact.as_ref().ok_or_else(|| {
        invalid_identity(resource, "converted OpenVINO artifact identity is missing")
    })?;
    let expected_format = match resource.id.as_str() {
        "firered_asr2_aed" => "openvino_ir_v11_smoke_buckets",
        "fcpe" | "basic_pitch" => "openvino_ir_v11",
        _ => return Err(invalid_identity(resource, "unsupported optional expert")),
    };
    if converted.format != expected_format
        || converted.manifest_filename != "manifest.json"
        || converted.runtime_id != "openvino_2026_3"
        || converted.runtime_version != "2026.3.0"
        || converted.runtime_commit != OPENVINO_RUNTIME_COMMIT
    {
        return Err(invalid_identity(
            resource,
            "source, converted artifact, and runtime identities are not independently pinned",
        ));
    }
    Ok(())
}

fn validate_firered_manifest(
    resource: &ResourceRef,
    bytes: &[u8],
) -> RuntimeManagerResult<ValidatedManifest> {
    let manifest: FireRedManifest = serde_json::from_slice(bytes).map_err(|error| {
        invalid_identity(
            resource,
            &format!("FireRed IR manifest is invalid: {error}"),
        )
    })?;
    let expected_files = firered_file_names();
    if manifest.schema_version != 1
        || manifest.model_id != "firered_asr2_aed"
        || manifest.format != "openvino_ir_v11_smoke_buckets"
        || manifest.source_revision != FIRERED_REVISION
        || manifest.fixture_contract.feature_frames != 230
        || manifest.fixture_contract.encoder_frames != 58
        || manifest.fixture_contract.decoder_cache_max != 10
        || !exact_file_set(&manifest.files, &expected_files)
    {
        return Err(invalid_identity(
            resource,
            "FireRed source lineage or fixed-window IR contract is incompatible",
        ));
    }
    Ok(ValidatedManifest {
        files: manifest.files,
        source_sha256: None,
    })
}

#[allow(clippy::too_many_arguments)]
fn validate_fixed_window_manifest(
    resource: &ResourceRef,
    bytes: &[u8],
    source_revision: &str,
    source_sha256: &str,
    input_shape: [usize; 3],
    sample_rate: u32,
    expected_files: &[&str],
) -> RuntimeManagerResult<ValidatedManifest> {
    let manifest: FixedWindowManifest = serde_json::from_slice(bytes).map_err(|error| {
        invalid_identity(
            resource,
            &format!("fixed-window IR manifest is invalid: {error}"),
        )
    })?;
    if manifest.schema_version != 1
        || manifest.model_id != resource.id
        || manifest.format != "openvino_ir_v11"
        || manifest.source_revision != source_revision
        || manifest.input_shape != input_shape
        || manifest.sample_rate != sample_rate
        || !exact_file_set(&manifest.files, expected_files)
    {
        return Err(invalid_identity(
            resource,
            "source lineage or fixed-window IR contract is incompatible",
        ));
    }
    Ok(ValidatedManifest {
        files: manifest.files,
        source_sha256: Some(source_sha256.to_string()),
    })
}

fn exact_file_set(files: &BTreeMap<String, String>, expected: &[&str]) -> bool {
    files.len() == expected.len() && expected.iter().all(|name| files.contains_key(*name))
}

fn firered_file_names() -> Vec<&'static str> {
    let mut names = vec![
        "ctc.bin",
        "ctc.xml",
        "decoder.bin",
        "encoder.bin",
        "encoder.xml",
        "cmvn.ark",
        "tokens.txt",
    ];
    names.extend([
        "decoder-00.xml",
        "decoder-01.xml",
        "decoder-02.xml",
        "decoder-03.xml",
        "decoder-04.xml",
        "decoder-05.xml",
        "decoder-06.xml",
        "decoder-07.xml",
        "decoder-08.xml",
        "decoder-09.xml",
        "decoder-10.xml",
    ]);
    names
}

fn require_regular_file(
    resource: &ResourceRef,
    path: &Path,
    label: &str,
) -> RuntimeManagerResult<()> {
    if !std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.file_type().is_file()) {
        return Err(invalid_identity(
            resource,
            &format!("{label} is missing or is not a regular file"),
        ));
    }
    Ok(())
}

fn invalid_identity(resource: &ResourceRef, message: &str) -> RuntimeManagerError {
    RuntimeManagerError::new("source_identity_mismatch", message).with_resource(resource)
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::json;
    use sha2::{Digest, Sha256};

    use super::*;
    use crate::catalog::ResourceCatalog;
    use crate::install::MutationOptions;
    use crate::state::{InstallState, RuntimePolicy};
    use crate::store::{CurrentPointer, StorePaths};

    #[test]
    fn all_optional_experts_import_verified_immutable_generations() {
        for model_id in ["firered_asr2_aed", "fcpe", "basic_pitch"] {
            let fixture = Fixture::new(model_id);
            let source_before = fixture.source_hashes();
            let manager = fixture.manager();
            let resource = ResourceRef::model(model_id).unwrap();

            manager
                .import_resource(&resource, &fixture.source, &confirmed())
                .unwrap();
            let first = fixture.current_generation();
            let status = manager.status(&resource, RuntimePolicy::Benchmark).unwrap();
            assert_eq!(status.install_state, InstallState::Installed, "{model_id}");
            assert!(status.integrity_verified, "{model_id}");
            assert!(status.usable, "{model_id}: {status:?}");
            assert!(
                manager
                    .status(&resource, RuntimePolicy::Production)
                    .unwrap()
                    .usable,
                "every promoted optional expert must be Production usable: {model_id}"
            );

            manager
                .import_resource(&resource, &fixture.source, &confirmed())
                .unwrap();
            let second = fixture.current_generation();
            assert_ne!(first, second, "{model_id}");
            assert!(fixture.generation_dir(&first).is_dir(), "{model_id}");
            assert_eq!(source_before, fixture.source_hashes(), "{model_id}");
        }
    }

    #[test]
    fn optional_expert_import_rejects_changed_manifest_without_republishing() {
        for model_id in ["firered_asr2_aed", "fcpe", "basic_pitch"] {
            let fixture = Fixture::new(model_id);
            let manager = fixture.manager();
            let resource = ResourceRef::model(model_id).unwrap();
            manager
                .import_resource(&resource, &fixture.source, &confirmed())
                .unwrap();
            let before = fixture.current_generation();
            std::fs::write(fixture.source.join("manifest.json"), b"{}").unwrap();
            let error = manager
                .import_resource(&resource, &fixture.source, &confirmed())
                .unwrap_err();
            assert_eq!(error.code, "source_identity_mismatch", "{model_id}");
            assert_eq!(before, fixture.current_generation(), "{model_id}");
        }
    }

    struct Fixture {
        root: PathBuf,
        store: PathBuf,
        source: PathBuf,
        catalog: ResourceCatalog,
        model_id: String,
    }

    impl Fixture {
        fn new(model_id: &str) -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "uta-optional-import-{}-{model_id}-{stamp}",
                std::process::id()
            ));
            let store = root.join("store");
            let source = root.join("source");
            std::fs::create_dir_all(&source).unwrap();
            let files = fixture_files(model_id);
            let mut hashes = BTreeMap::new();
            for name in files {
                let bytes = format!("fixture:{model_id}:{name}");
                std::fs::write(source.join(name), bytes.as_bytes()).unwrap();
                hashes.insert(name.to_string(), digest(bytes.as_bytes()));
            }
            let manifest = fixture_manifest(model_id, hashes.clone());
            let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
            std::fs::write(source.join("manifest.json"), &manifest_bytes).unwrap();
            let mut catalog = ResourceCatalog::default_catalog().unwrap();
            let model = catalog.models.get_mut(model_id).unwrap();
            if model_id == "firered_asr2_aed" {
                for artifact in &mut model.source.artifacts {
                    if let Some(hash) = hashes.get(&artifact.filename) {
                        artifact.sha256.clone_from(hash);
                    }
                }
            }
            model
                .source
                .converted_artifact
                .as_mut()
                .unwrap()
                .manifest_sha256 = digest(&manifest_bytes);
            Self {
                root,
                store,
                source,
                catalog,
                model_id: model_id.to_string(),
            }
        }

        fn manager(&self) -> RuntimeManager {
            let worker = self.root.join("openvino-worker");
            std::fs::write(&worker, b"fixture worker").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&worker, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            RuntimeManager::new(
                self.catalog.clone(),
                StorePaths::default()
                    .with_store_root(&self.store)
                    .with_runtime_override("openvino_2026_3", worker),
            )
        }

        fn current_generation(&self) -> String {
            let bytes = std::fs::read(
                self.store
                    .join("models")
                    .join(&self.model_id)
                    .join("current.json"),
            )
            .unwrap();
            serde_json::from_slice::<CurrentPointer>(&bytes)
                .unwrap()
                .generation
        }

        fn generation_dir(&self, generation: &str) -> PathBuf {
            self.store
                .join("models")
                .join(&self.model_id)
                .join("generations")
                .join(generation)
        }

        fn source_hashes(&self) -> BTreeMap<String, String> {
            std::fs::read_dir(&self.source)
                .unwrap()
                .map(|entry| {
                    let path = entry.unwrap().path();
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    (name, sha256_file(&path).unwrap())
                })
                .collect()
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn fixture_files(model_id: &str) -> Vec<&'static str> {
        match model_id {
            "firered_asr2_aed" => firered_file_names(),
            "fcpe" => vec!["fcpe.xml", "fcpe.bin"],
            "basic_pitch" => vec!["basic-pitch.xml", "basic-pitch.bin"],
            _ => unreachable!(),
        }
    }

    fn fixture_manifest(model_id: &str, files: BTreeMap<String, String>) -> serde_json::Value {
        match model_id {
            "firered_asr2_aed" => json!({
                "schema_version": 1,
                "model_id": model_id,
                "format": "openvino_ir_v11_smoke_buckets",
                "source_revision": FIRERED_REVISION,
                "source_hashes": {
                    "encoder": "0fe4038f5e5cd340171535b7b5f2e184482e90e22aeb2ed0f7abe81af10783f9",
                    "decoder": "aeef22670d95aa90d78a1927242c2a6e4fbb8b44c1af8d3ae988c46fd67ae833",
                    "ctc": "8881d31c17bca30a7972299d5395daaa6424da6328a818ba496719c3118c32b4"
                },
                "fixture_contract": {"feature_frames":230,"encoder_frames":58,"decoder_cache_max":10},
                "files": files
            }),
            "fcpe" => json!({
                "schema_version":1,"model_id":model_id,"format":"openvino_ir_v11",
                "source_revision":FCPE_REVISION,"source_onnx_sha256":FCPE_SOURCE_SHA256,
                "input_shape":[1,32000,1],"sample_rate":16000,"files":files
            }),
            "basic_pitch" => json!({
                "schema_version":1,"model_id":model_id,"format":"openvino_ir_v11",
                "source_revision":BASIC_PITCH_REVISION,"source_onnx_sha256":BASIC_PITCH_SOURCE_SHA256,
                "input_shape":[1,43844,1],"sample_rate":22050,"files":files
            }),
            _ => unreachable!(),
        }
    }

    fn confirmed() -> MutationOptions {
        MutationOptions {
            confirmed: true,
            accepted_licenses: BTreeSet::new(),
        }
    }

    fn digest(bytes: &[u8]) -> String {
        format!("{:x}", Sha256::digest(bytes))
    }
}
