use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::catalog::{AcquisitionMethod, ResourceCatalog};
use crate::manifest::read_install_manifest;
use crate::store::read_current_pointer;

#[test]
fn import_is_confirmed_and_atomically_published_without_hash_rejection() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let source = fixture.write("source.gguf", b"audited fixture");

    let error = manager
        .import_resource(&resource, &source, &MutationOptions::default())
        .unwrap_err();
    assert_eq!(error.code, "confirmation_required");
    assert!(!fixture.root.join("models").exists());

    let result = manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();
    assert_eq!(result.changed, vec![resource.clone()]);
    let status = manager.status(&resource, RuntimePolicy::Benchmark).unwrap();
    assert_eq!(status.install_state, InstallState::Installed);
    assert!(status.usable);

    let current = std::fs::read(fixture.root.join("models/qwen3_asr_1_7b/current.json")).unwrap();
    let wrong = fixture.write("wrong.gguf", b"wrong identity");
    manager
        .import_resource(&resource, &wrong, &confirmed())
        .unwrap();
    assert_ne!(
        current,
        std::fs::read(fixture.root.join("models/qwen3_asr_1_7b/current.json")).unwrap()
    );
}

#[test]
fn roformer_gguf_directory_import_uses_current_vulkan_artifact_identity() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let resource = ResourceRef::model("bs_roformer_leap_xe90_vocals").unwrap();
    let source = fixture.root.join("roformer-import");
    std::fs::create_dir(&source).unwrap();
    fixture.write(
        "roformer-import/bs_leap_xe_voc-F32.gguf",
        b"roformer gguf fixture",
    );

    manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();

    let pointer = read_current_pointer(
        &fixture
            .root
            .join("models/bs_roformer_leap_xe90_vocals/current.json"),
    )
    .unwrap();
    let manifest = read_install_manifest(
        &fixture
            .root
            .join("models/bs_roformer_leap_xe90_vocals/generations")
            .join(pointer.generation),
    )
    .unwrap();
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].path, Path::new("bs_leap_xe_voc-F32.gguf"));
    assert_eq!(manifest.source_sha256, None);
    assert_eq!(
        manifest.runtime_recipe_digest.as_deref(),
        Some(crate::runtime_lock::GGML_RUNTIME_RECIPE_SHA256)
    );
}

#[test]
fn rmvpe_gguf_import_publishes_the_vulkan_filename_and_provenance() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let resource = ResourceRef::model("rmvpe").unwrap();
    let source = fixture.write("rmvpe-f32.gguf", b"rmvpe gguf fixture");

    manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();

    let pointer = read_current_pointer(&fixture.root.join("models/rmvpe/current.json")).unwrap();
    let manifest = read_install_manifest(
        &fixture
            .root
            .join("models/rmvpe/generations")
            .join(pointer.generation),
    )
    .unwrap();
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(manifest.files[0].path, Path::new("rmvpe-f32.gguf"));
    assert_eq!(
        manifest.source_sha256.as_deref(),
        Some("1b4095d1b57818f5e812b1986ea5a7d7e6d64ccd9e1b1d7b71f4091304513fd2")
    );
    assert_eq!(
        manifest.conversion_recipe_digest.as_deref(),
        Some(crate::runtime_lock::RMVPE_GGUF_CONVERSION_RECIPE_SHA256)
    );
    assert_eq!(
        manifest.runtime_recipe_digest.as_deref(),
        Some(crate::runtime_lock::GGML_RUNTIME_RECIPE_SHA256)
    );
}

#[test]
fn task_23_managed_downloads_resolve_to_exact_hugging_face_revisions() {
    let catalog = ResourceCatalog::default_catalog().unwrap();
    assert_eq!(
        managed_download_url(catalog.model("bs_roformer_leap_xe90_vocals").unwrap()).unwrap(),
        "https://huggingface.co/scragnog/HOT-Step-CPP-SuperSep/resolve/440487b8300dcd61453cc52ec244a38150b03456/bs_leap_xe_voc-F32.gguf"
    );
    assert_eq!(
        managed_download_url(catalog.model("bs_polarformer_public_instrumental").unwrap()).unwrap(),
        "https://huggingface.co/bgkb/bs_polarformer/resolve/9158719ee2173edd480a735764627526506fe4af/bs_polarformer_fp16.onnx"
    );
}

#[test]
fn qwen_aligner_local_import_receipt_separates_source_and_converted_identity() {
    let fixture = Fixture::new();
    let mut catalog = ResourceCatalog::default_catalog().unwrap();
    let model = catalog.models.get_mut("qwen3_forced_aligner_0_6b").unwrap();
    let expected_import_sha256 = format!("{:x}", Sha256::digest(b"aligner gguf fixture"));
    let expected_conversion_recipe = {
        let converted = model.source.converted_artifact.as_mut().unwrap();
        converted.manifest_sha256 = expected_import_sha256.clone();
        converted.conversion_recipe_sha256.clone()
    };
    let source_identity = model.source.clone();
    let expected_runtime_recipe = model.runtime_recipe_digest.clone();
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default().with_store_root(&fixture.root),
    );
    let resource = ResourceRef::model("qwen3_forced_aligner_0_6b").unwrap();
    let source = fixture.write("aligner.gguf", b"aligner gguf fixture");

    manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();

    let pointer = read_current_pointer(
        &fixture
            .root
            .join("models/qwen3_forced_aligner_0_6b/current.json"),
    )
    .unwrap();
    let manifest = read_install_manifest(
        &fixture
            .root
            .join("models/qwen3_forced_aligner_0_6b/generations")
            .join(pointer.generation),
    )
    .unwrap();
    assert_eq!(manifest.source.as_ref(), Some(&source_identity));
    assert_eq!(
        manifest.source.as_ref().unwrap().sha256.as_deref(),
        Some("00568245ceca5af1991d28562a75fe1ddc9bfeb041c27fda66947ea05c47fb86")
    );
    assert_eq!(
        manifest.source_sha256.as_deref(),
        Some(expected_import_sha256.as_str())
    );
    assert_eq!(
        manifest.conversion_recipe_digest.as_deref(),
        Some(expected_conversion_recipe.as_str())
    );
    assert_eq!(manifest.runtime_recipe_digest, expected_runtime_recipe);
    assert_eq!(manifest.files.len(), 1);
    assert_eq!(
        manifest.files[0].path,
        Path::new("qwen3-forced-aligner-predict-woo-f16.gguf")
    );
}

#[test]
fn remove_refuses_active_leases_and_unmanaged_files() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let source = fixture.write("source.gguf", b"audited fixture");
    manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();

    let resolved = manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    let error = manager
        .remove(std::slice::from_ref(&resource), &confirmed())
        .unwrap_err();
    assert_eq!(error.code, "resource_in_use");
    drop(resolved);

    std::fs::write(
        fixture.root.join("models/qwen3_asr_1_7b/unmanaged.txt"),
        b"do not delete",
    )
    .unwrap();
    let error = manager
        .remove(std::slice::from_ref(&resource), &confirmed())
        .unwrap_err();
    assert_eq!(error.code, "unmanaged_files_present");
    assert!(
        fixture
            .root
            .join("models/qwen3_asr_1_7b/unmanaged.txt")
            .is_file()
    );
}

#[test]
fn managed_install_and_reinstall_publish_without_mutating_old_generation() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let transport = FixtureTransport::success(b"audited fixture");
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
    let first = manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    let first_bytes = std::fs::read(&first.model_path).unwrap();
    manager
        .reinstall_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    let second = manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    assert_ne!(first.generation, second.generation);
    assert_eq!(std::fs::read(&first.model_path).unwrap(), first_bytes);
    assert!(first.model_path.is_file());
    assert_eq!(transport.calls.load(Ordering::SeqCst), 2);
    manager
        .rollback(&resource, &first.generation, &confirmed())
        .unwrap();
    let rolled_back = manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    assert_eq!(rolled_back.generation, first.generation);
    drop(rolled_back);
    drop(second);
    let error = manager
        .remove(std::slice::from_ref(&resource), &confirmed())
        .unwrap_err();
    assert_eq!(error.code, "resource_in_use");
    drop(first);
}

#[test]
fn source_hash_metadata_change_does_not_invalidate_installed_generation() {
    let fixture = Fixture::new();
    let old_manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    old_manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let old = old_manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    let old_bytes = std::fs::read(&old.model_path).unwrap();

    let mut upgraded_catalog = fixture.catalog();
    let upgraded_model = upgraded_catalog.models.get_mut("qwen3_asr_1_7b").unwrap();
    upgraded_model.source.sha256 = Some(format!("{:x}", Sha256::digest(b"new audited fixture")));
    upgraded_model.acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::ManagedDownload,
        label: "upgraded source download".to_string(),
        license_id: None,
    }];
    let upgraded_worker = fixture.write_executable("source-upgrade-qwen-worker");
    let upgraded_manager = RuntimeManager::new(
        upgraded_catalog,
        StorePaths::default()
            .with_store_root(&fixture.root)
            .with_runtime_override("qwen_asr_runtime", upgraded_worker),
    );
    assert_eq!(
        upgraded_manager
            .status(&resource, RuntimePolicy::Benchmark)
            .unwrap()
            .install_state,
        InstallState::Installed
    );
    let upgraded = upgraded_manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    assert_eq!(upgraded.generation, old.generation);
    assert_eq!(std::fs::read(&old.model_path).unwrap(), old_bytes);
}

#[test]
fn recipe_digest_metadata_change_does_not_replace_current_generation() {
    let fixture = Fixture::new();
    let old_manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let transport = FixtureTransport::success(b"audited fixture");
    old_manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    let old = old_manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    let old_bytes = std::fs::read(&old.model_path).unwrap();

    let mut catalog = fixture.catalog();
    let model = catalog.models.get_mut("qwen3_asr_1_7b").unwrap();
    model.recipe_digest = "new-catalog-recipe".to_string();
    model.acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::ManagedDownload,
        label: "fixture download".to_string(),
        license_id: None,
    }];
    let worker = fixture.write_executable("new-recipe-qwen-worker");
    let new_manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&fixture.root)
            .with_runtime_override("qwen_asr_runtime", worker),
    );
    let status = new_manager
        .status(&resource, RuntimePolicy::Benchmark)
        .unwrap();
    assert_eq!(status.install_state, InstallState::Installed);
    assert_eq!(status.origin, ResourceOrigin::Managed);
    assert!(status.usable);
    new_manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    let new = new_manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();
    assert_eq!(old.generation, new.generation);
    assert_eq!(new.model_recipe_digest, old.model_recipe_digest);
    assert_eq!(std::fs::read(&old.model_path).unwrap(), old_bytes);
    drop(new);
    let error = new_manager
        .remove(std::slice::from_ref(&resource), &confirmed())
        .unwrap_err();
    assert_eq!(error.code, "resource_in_use");
    drop(old);
}

#[test]
fn managed_repair_replaces_a_corrupt_current_without_deleting_it_first() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let transport = FixtureTransport::success(b"audited fixture");
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    let current_path = fixture.root.join("models/qwen3_asr_1_7b/current.json");
    let old_pointer: CurrentPointer =
        serde_json::from_slice(&std::fs::read(&current_path).unwrap()).unwrap();
    let old_generation = fixture
        .root
        .join("models/qwen3_asr_1_7b/generations")
        .join(&old_pointer.generation);
    std::fs::write(old_generation.join("fixture.gguf"), b"corrupt").unwrap();
    assert_eq!(
        manager
            .status(&resource, RuntimePolicy::Benchmark)
            .unwrap()
            .install_state,
        InstallState::Corrupt
    );
    manager
        .repair_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    let new_pointer: CurrentPointer =
        serde_json::from_slice(&std::fs::read(current_path).unwrap()).unwrap();
    assert_ne!(old_pointer.generation, new_pointer.generation);
    assert!(old_generation.is_dir());
    assert_eq!(
        manager
            .status(&resource, RuntimePolicy::Benchmark)
            .unwrap()
            .install_state,
        InstallState::Installed
    );
}

#[test]
fn one_roformer_generation_does_not_install_the_family_bundle() {
    let fixture = Fixture::new();
    let mut catalog = fixture.catalog();
    let model = catalog
        .models
        .get_mut("bs_roformer_leap_xe90_vocals")
        .unwrap();
    model.source.repository = Some("fixture/roformer".to_string());
    model.source.revision = Some("pinned".to_string());
    model.source.filename = Some("vocals.gguf".to_string());
    model.source.sha256 = Some(format!("{:x}", Sha256::digest(b"audited fixture")));
    model.source.converted_artifact = None;
    model.dependencies = vec![ResourceRef::runtime("openvino_2026_3").unwrap()];
    model.pinned_backend = Some(crate::catalog::NativeBackend::OpenVino);
    model.runtime_recipe_digest = None;
    model.acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::ManagedDownload,
        label: "fixture download".to_string(),
        license_id: None,
    }];
    let worker = fixture.write_executable("openvino-worker");
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&fixture.root)
            .with_runtime_override("openvino_2026_3", worker),
    );
    let resource = ResourceRef::model("bs_roformer_leap_xe90_vocals").unwrap();
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    assert_eq!(
        manager
            .status(&resource, RuntimePolicy::Benchmark)
            .unwrap()
            .install_state,
        InstallState::Installed
    );
    assert_eq!(
        manager
            .status(
                &ResourceRef::model("melband_roformer_inst_v2").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap()
            .install_state,
        InstallState::Absent
    );
    assert!(
        !manager
            .status(
                &ResourceRef::bundle("roformer").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap()
            .usable
    );
}

#[test]
fn managed_download_reinstall_does_not_reject_payload_by_hash() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let current_path = fixture.root.join("models/qwen3_asr_1_7b/current.json");
    let before = std::fs::read(&current_path).unwrap();
    manager
        .reinstall_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"different bytes"),
        )
        .unwrap();
    assert_ne!(std::fs::read(current_path).unwrap(), before);
    assert_eq!(
        std::fs::read_dir(fixture.root.join("downloads"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn license_metadata_never_blocks_confirmed_installation() {
    let fixture = Fixture::new();
    let mut catalog = fixture.catalog();
    catalog
        .models
        .get_mut("qwen3_asr_1_7b")
        .unwrap()
        .acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::ManagedDownload,
        label: "fixture download".to_string(),
        license_id: Some("license:qwen-fixture".to_string()),
    }];
    let worker = fixture.write_executable("licensed-qwen-worker");
    let manager = RuntimeManager::new(
        catalog,
        StorePaths::default()
            .with_store_root(&fixture.root)
            .with_runtime_override("qwen_asr_runtime", worker),
    );
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let transport = FixtureTransport::success(b"audited fixture");
    let result = manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &transport,
        )
        .unwrap();
    assert_eq!(result.changed, vec![resource]);
    assert_eq!(transport.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn reinstall_keeps_the_previous_generation_while_its_resolution_lease_is_active() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let old_generation =
        read_current_pointer(&fixture.root.join("models/qwen3_asr_1_7b/current.json"))
            .unwrap()
            .generation;
    let resolved = manager
        .resolve_model("qwen3_asr_1_7b", RuntimePolicy::Benchmark)
        .unwrap();

    let mut upgraded_catalog = fixture.catalog();
    let upgraded_model = upgraded_catalog.models.get_mut("qwen3_asr_1_7b").unwrap();
    upgraded_model.recipe_digest = "b".repeat(64);
    upgraded_model.acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::ManagedDownload,
        label: "upgraded fixture download".to_string(),
        license_id: None,
    }];
    let upgraded_worker = fixture.write_executable("upgraded-qwen-worker");
    let upgraded_manager = RuntimeManager::new(
        upgraded_catalog,
        StorePaths::default()
            .with_store_root(&fixture.root)
            .with_runtime_override("qwen_asr_runtime", upgraded_worker),
    );
    upgraded_manager
        .reinstall_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let new_generation =
        read_current_pointer(&fixture.root.join("models/qwen3_asr_1_7b/current.json"))
            .unwrap()
            .generation;

    assert_ne!(new_generation, old_generation);
    assert!(
        fixture
            .root
            .join("models/qwen3_asr_1_7b/generations")
            .join(old_generation)
            .is_dir()
    );
    drop(resolved);
}

#[test]
fn unavailable_conversion_recipe_never_replaces_the_current_generation() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let current_path = fixture.root.join("models/qwen3_asr_1_7b/current.json");
    let before = std::fs::read(&current_path).unwrap();
    let mut conversion_catalog = fixture.catalog();
    conversion_catalog
        .models
        .get_mut("qwen3_asr_1_7b")
        .unwrap()
        .acquisition = vec![AcquisitionSpec {
        method: AcquisitionMethod::SourceConvert,
        label: "fixture conversion".to_string(),
        license_id: None,
    }];
    let conversion_manager = RuntimeManager::new(
        conversion_catalog,
        StorePaths::default().with_store_root(&fixture.root),
    );
    let error = conversion_manager
        .reinstall_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"unused"),
        )
        .unwrap_err();
    assert_eq!(error.code, "conversion_failed");
    assert_eq!(std::fs::read(current_path).unwrap(), before);
}

#[test]
fn failed_reinstall_keeps_current_generation_unchanged() {
    let fixture = Fixture::new();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap();
    let current_path = fixture.root.join("models/qwen3_asr_1_7b/current.json");
    let before = std::fs::read(&current_path).unwrap();
    let error = manager
        .reinstall_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::failure(),
        )
        .unwrap_err();
    assert_eq!(error.code, "network_failed");
    assert_eq!(std::fs::read(current_path).unwrap(), before);
}

#[cfg(unix)]
#[test]
fn publish_rejects_a_symlinked_managed_kind_directory() {
    use std::os::unix::fs::symlink;

    let fixture = Fixture::new();
    let outside = fixture.root.join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, fixture.root.join("models")).unwrap();
    let manager = fixture.managed_manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let error = manager
        .install_with_transport(
            std::slice::from_ref(&resource),
            RuntimePolicy::Benchmark,
            &confirmed(),
            &FixtureTransport::success(b"audited fixture"),
        )
        .unwrap_err();
    assert_eq!(error.code, "unmanaged_files_present");
    assert_eq!(std::fs::read_dir(outside).unwrap().count(), 0);
}

#[test]
fn remove_validates_every_resource_before_deleting_any_generation() {
    let fixture = Fixture::new();
    let manager = fixture.manager();
    let resource = ResourceRef::model("qwen3_asr_1_7b").unwrap();
    let source = fixture.write("source.gguf", b"audited fixture");
    manager
        .import_resource(&resource, &source, &confirmed())
        .unwrap();
    let unknown = ResourceRef::model("unknown_model").unwrap();
    let error = manager
        .remove(&[resource.clone(), unknown], &confirmed())
        .unwrap_err();
    assert_eq!(error.code, "unknown_resource");
    assert_eq!(
        manager
            .status(&resource, RuntimePolicy::Benchmark)
            .unwrap()
            .install_state,
        InstallState::Installed
    );
}

#[test]
fn qwen_download_url_and_hash_identity_are_catalog_pinned() {
    let catalog = ResourceCatalog::default_catalog().unwrap();
    let model = catalog.model("qwen3_asr_1_7b").unwrap();
    assert_eq!(
        managed_download_url(model).unwrap(),
        "https://huggingface.co/handy-computer/Qwen3-ASR-1.7B-gguf/resolve/92282af1610a2db19d66f2bef1e260f5deca782d/Qwen3-ASR-1.7B-Q4_K_M.gguf"
    );
    assert_eq!(
        model.source.sha256.as_deref(),
        Some("b7afe3674f653fa84f712ed2440353c6e7cf7f93697fef76b05a26538b24844e")
    );
    assert_eq!(
        model
            .source
            .algorithm
            .as_ref()
            .map(|identity| identity.repository.as_str()),
        Some("Qwen/Qwen3-ASR-1.7B")
    );
    assert_eq!(
        model.license.source_page.as_deref(),
        Some("https://huggingface.co/Qwen/Qwen3-ASR-1.7B")
    );
}

#[test]
fn plan_is_offline_and_does_not_create_store_paths() {
    let fixture = Fixture::new();
    let store = fixture.root.join("absent-store");
    let manager = RuntimeManager::new(
        ResourceCatalog::default_catalog().unwrap(),
        StorePaths::default().with_store_root(&store),
    );
    let plan = manager
        .plan(
            &[ResourceRef::model("qwen3_asr_1_7b").unwrap()],
            RuntimePolicy::Benchmark,
        )
        .unwrap();
    assert!(!plan.to_add.is_empty());
    let qwen = plan
        .to_add
        .iter()
        .find(|item| item.resource.id == "qwen3_asr_1_7b")
        .unwrap();
    assert_eq!(
        qwen.license.as_ref().map(|license| license.status.as_str()),
        Some("apache-2.0")
    );
    assert!(plan.network_required);
    assert!(!store.exists());
}

fn confirmed() -> MutationOptions {
    MutationOptions { confirmed: true }
}

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(unique_operation_id());
        std::fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    fn catalog(&self) -> ResourceCatalog {
        let mut catalog = ResourceCatalog::default_catalog().unwrap();
        let model = catalog.models.get_mut("qwen3_asr_1_7b").unwrap();
        model.source.sha256 = Some(format!("{:x}", Sha256::digest(b"audited fixture")));
        model.source.filename = Some("fixture.gguf".to_string());
        model.acquisition = vec![AcquisitionSpec {
            method: AcquisitionMethod::LocalImport,
            label: "fixture import".to_string(),
            license_id: None,
        }];
        catalog
    }

    fn managed_manager(&self) -> RuntimeManager {
        let mut catalog = self.catalog();
        catalog
            .models
            .get_mut("qwen3_asr_1_7b")
            .unwrap()
            .acquisition = vec![AcquisitionSpec {
            method: AcquisitionMethod::ManagedDownload,
            label: "fixture download".to_string(),
            license_id: None,
        }];
        let worker = self.write_executable("managed-qwen-worker");
        RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&self.root)
                .with_runtime_override("qwen_asr_runtime", worker),
        )
    }

    fn manager(&self) -> RuntimeManager {
        let worker = self.write_executable("qwen-worker");
        RuntimeManager::new(
            self.catalog(),
            StorePaths::default()
                .with_store_root(&self.root)
                .with_runtime_override("qwen_asr_runtime", worker),
        )
    }

    fn write(&self, name: &str, bytes: &[u8]) -> PathBuf {
        let path = self.root.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    fn write_executable(&self, name: &str) -> PathBuf {
        #[cfg(windows)]
        let path = self.write(&format!("{name}.exe"), b"worker");
        #[cfg(not(windows))]
        let path = self.write(name, b"worker");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        path
    }
}

struct FixtureTransport {
    bytes: Option<Vec<u8>>,
    calls: AtomicUsize,
}

impl FixtureTransport {
    fn success(bytes: &[u8]) -> Self {
        Self {
            bytes: Some(bytes.to_vec()),
            calls: AtomicUsize::new(0),
        }
    }

    fn failure() -> Self {
        Self {
            bytes: None,
            calls: AtomicUsize::new(0),
        }
    }
}

impl AcquisitionTransport for FixtureTransport {
    fn download(
        &self,
        _url: &str,
        destination: &Path,
        _maximum_bytes: Option<u64>,
    ) -> RuntimeManagerResult<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        match &self.bytes {
            Some(bytes) => std::fs::write(destination, bytes).map_err(publish_io),
            None => Err(RuntimeManagerError::new(
                "network_failed",
                "fixture network failure",
            )),
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}
