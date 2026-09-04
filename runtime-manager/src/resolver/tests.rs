    use std::fs;

    use sha2::{Digest, Sha256};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn game_openvino_route_is_experimental_usable_after_repaired_full_song_rerun() {
        // GAME's OpenVINO GPU route caused this host's documented crash and
        // is no longer a default-selectable backend at any non-experimental
        // policy tier; it stays reachable only under an explicit request at
        // `RuntimePolicy::Experimental`.
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("game");
        fixture.write_model_current_with_catalog("game", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let status = manager
            .status_with_backend(
                &ResourceRef::model("game").unwrap(),
                RuntimePolicy::Experimental,
                Some(NativeBackend::OpenVino),
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::OpenVino));
    }

    #[test]
    fn game_openvino_route_is_unusable_under_production_and_benchmark_policy() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("game");
        fixture.write_model_current_with_catalog("game", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        for policy in [RuntimePolicy::Production, RuntimePolicy::Benchmark] {
            let status = manager
                .status_with_backend(
                    &ResourceRef::model("game").unwrap(),
                    policy,
                    Some(NativeBackend::OpenVino),
                )
                .unwrap();
            assert!(!status.usable, "{policy:?} {status:?}");
        }
    }

    #[test]
    fn game_is_production_usable_with_game_native_runtime() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("game");
        fixture.write_model_current_with_catalog("game", &catalog);
        let worker = fixture.write_executable("uta-game-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("game_native_v1", worker),
        );
        let status = manager
            .status_with_backend(
                &ResourceRef::model("game").unwrap(),
                RuntimePolicy::Production,
                Some(NativeBackend::Vulkan),
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::Vulkan));
    }

    #[test]
    fn jbm555_is_production_usable_with_jbm555_native_runtime() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("jbm555_cectc_80");
        fixture.write_model_current_with_catalog("jbm555_cectc_80", &catalog);
        let worker = fixture.write_executable("uta-jbm-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("jbm555_native_v1", worker),
        );
        let status = manager
            .status_with_backend(
                &ResourceRef::model("jbm555_cectc_80").unwrap(),
                RuntimePolicy::Production,
                Some(NativeBackend::NativeDsp),
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::NativeDsp));
    }

    #[test]
    fn rmvpe_is_production_usable_when_installed_and_runtime_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::Vulkan));
        // The GGML/Vulkan route carries its own native evidence, so the
        // production analysis path resolves it without a policy downgrade.
        let production = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert!(production.usable, "{production:?}");
        assert_eq!(production.selected_backend, Some(NativeBackend::Vulkan));
        assert!(production.reasons.is_empty(), "{production:?}");
    }

    #[test]
    fn cpu_reference_route_requires_explicit_experimental_selection() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("fcpe");
        fixture.write_model_current_with_catalog("fcpe", &catalog);
        let worker = fixture.write_executable("openvino-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        );
        let resource = ResourceRef::model("fcpe").unwrap();
        let status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Experimental,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::CpuReference));
        let resolved = manager
            .resolve_model_with_backend(
                "fcpe",
                RuntimePolicy::Experimental,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::CpuReference);
        let production = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Production,
                Some(NativeBackend::CpuReference),
            )
            .unwrap();
        assert!(!production.usable);
        assert_eq!(production.selected_backend, None);
    }

    #[test]
    fn pinned_ggml_route_selects_its_worker_without_hash_rejection() {
        let fixture = Fixture::new();
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let ggml_root = fixture.root.join("ggml-models");
        let model_dir = ggml_root.join("melband_roformer_inst_v2");
        fs::create_dir_all(&model_dir).unwrap();
        let model = fs::File::create(model_dir.join("model-fp16.gguf")).unwrap();
        model.set_len(787_918_656).unwrap();
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_ggml_models_root(ggml_root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let resource = ResourceRef::model("melband_roformer_inst_v2").unwrap();
        let status = manager.status(&resource, RuntimePolicy::Benchmark).unwrap();
        assert!(status.usable, "{status:?}");
        assert_eq!(status.selected_backend, Some(NativeBackend::Vulkan));
        assert_eq!(
            status.runtime_resource,
            Some(ResourceRef::runtime("ggml_vulkan_v1").unwrap())
        );
        let resolved = manager
            .resolve_model("melband_roformer_inst_v2", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::Vulkan);
    }

    #[test]
    fn polarformer_pinned_ggml_route_resolves_from_the_flat_ggml_models_directory() {
        let fixture = Fixture::new();
        let catalog = ResourceCatalog::default_catalog().unwrap();
        let ggml_root = fixture.root.join("ggml-models");
        let model_dir = ggml_root.join("bs_polarformer_public_instrumental");
        fs::create_dir_all(&model_dir).unwrap();
        let model = fs::File::create(model_dir.join("model-fp16.gguf")).unwrap();
        model.set_len(204_237_408).unwrap();
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_ggml_models_root(ggml_root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let resolved = manager
            .resolve_model(
                "bs_polarformer_public_instrumental",
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::Vulkan);
    }

    #[test]
    fn managed_qwen_vulkan_resolution_selects_the_converted_gguf_file() {
        let fixture = Fixture::new();
        let mut catalog = ResourceCatalog::default_catalog().unwrap();
        let payload_digest = format!("{:x}", Sha256::digest(b"managed fixture"));
        let model = catalog.models.get_mut("qwen3_forced_aligner_0_6b").unwrap();
        model.source.filename = Some("model.bin".to_string());
        model.source.sha256 = Some(payload_digest.clone());
        model.source.artifacts.clear();
        let converted = model.source.converted_artifact.as_mut().unwrap();
        converted.manifest_filename = "model.bin".to_string();
        converted.manifest_sha256 = payload_digest;
        converted.conversion_recipe_sha256.clear();
        fixture.write_model_current_with_catalog("qwen3_forced_aligner_0_6b", &catalog);
        let worker = fixture.write_executable("qwen-align-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("qwen_align_runtime", worker),
        );
        let resolved = manager
            .resolve_model("qwen3_forced_aligner_0_6b", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::Vulkan);
        assert_eq!(resolved.model_path.file_name().unwrap(), "model.bin");
        assert!(resolved.model_path.is_file());
    }

    #[test]
    fn managed_pinned_source_cannot_claim_identity_for_different_payload() {
        let fixture = Fixture::new();
        fixture.write_model_current("qwen3_asr_1_7b", "fake-qwen");
        let worker = fixture.write_executable("qwen-worker");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("qwen_asr_runtime", worker),
        )
        .unwrap();
        let status = manager
            .status(
                &ResourceRef::model("qwen3_asr_1_7b").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert!(!status.usable);
        assert_eq!(status.install_state, InstallState::Corrupt);
        assert_eq!(status.origin, ResourceOrigin::Managed);
        assert!(status.reasons.contains(&ReadinessReason::Corrupt));
    }

    #[test]
    fn fcpe_native_route_is_production_usable_when_installed_and_runtime_ready() {
        // fcpe's OpenVINO GPU route used to be the model's default via the
        // now-removed blanket promotion pass; it now has its own real
        // native CPU DSP route (`uta-fcpe-worker`) that is the default
        // instead, with no promotion mechanism involved.
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("fcpe");
        fixture.write_model_current_with_catalog("fcpe", &catalog);
        let worker = fixture.write_executable("uta-fcpe-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("fcpe_native_v1", worker),
        );
        let status = manager
            .status(
                &ResourceRef::model("fcpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert!(status.integrity_verified);
        assert!(status.runnable);
        assert!(status.usable);
        let resolved = manager
            .resolve_model("fcpe", RuntimePolicy::Production)
            .unwrap();
        assert_eq!(resolved.backend, NativeBackend::NativeDsp);
    }

    #[test]
    fn benchmark_can_resolve_fcpe_native_route_when_all_local_pieces_are_ready() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("fcpe");
        fixture.write_model_current_with_catalog("fcpe", &catalog);
        let worker = fixture.write_executable("uta-fcpe-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("fcpe_native_v1", worker.clone()),
        );
        let resolved = manager
            .resolve_model("fcpe", RuntimePolicy::Benchmark)
            .unwrap();
        assert_eq!(resolved.model_id, "fcpe");
        assert_eq!(resolved.backend, NativeBackend::NativeDsp);
        assert_eq!(resolved.runtime_content_digest, "environment");
        assert_eq!(resolved.runtime_executable, worker);
    }

    #[test]
    fn installed_model_is_not_usable_when_runtime_is_missing() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default().with_store_root(&fixture.root),
        );
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Installed);
        assert!(!status.usable);
        assert!(status.reasons.contains(&ReadinessReason::RuntimeMissing));
    }

    #[test]
    fn show_combines_catalog_metadata_with_local_status() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let details = manager
            .show(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Benchmark,
            )
            .unwrap();
        assert_eq!(details.metadata.display_name, "RMVPE");
        assert!(details.status.usable);
        assert_eq!(details.metadata.capabilities, vec!["pitch.track"]);
    }

    #[test]
    fn metadata_and_verified_status_use_structural_checks_for_same_size_payloads() {
        let fixture = Fixture::new();
        let catalog = Fixture::catalog_with_fixture_model("rmvpe");
        fixture.write_model_current_with_catalog("rmvpe", &catalog);
        let worker = fixture.write_executable("ggml-worker");
        let manager = RuntimeManager::new(
            catalog,
            StorePaths::default()
                .with_store_root(&fixture.root)
                .with_runtime_override("ggml_vulkan_v1", worker),
        );
        let resource = ResourceRef::model("rmvpe").unwrap();

        let payload = fixture.current_model_generation("rmvpe").join("model.bin");
        fs::remove_file(&payload).unwrap();
        assert_eq!(
            manager
                .status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Incomplete
        );

        fs::write(&payload, b"managed fixturE").unwrap();
        assert_eq!(
            manager
                .status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Installed
        );
        assert_eq!(
            manager
                .verified_status(&resource, RuntimePolicy::Production)
                .unwrap()
                .install_state,
            InstallState::Installed
        );
    }

    #[test]
    fn managed_generation_rejects_undeclared_files() {
        let fixture = Fixture::new();
        fixture.write_model_current("rmvpe", "rmvpe-gen");
        fs::write(
            fixture
                .current_model_generation("rmvpe")
                .join("unknown.bin"),
            b"unmanaged",
        )
        .unwrap();
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let status = manager
            .status(
                &ResourceRef::model("rmvpe").unwrap(),
                RuntimePolicy::Production,
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Corrupt);
    }

    #[test]
    fn legacy_openvino_roformer_manifest_cannot_restore_forbidden_route() {
        let fixture = Fixture::new();
        let model_dir = fixture
            .root
            .join("audio-processing/melband_roformer_inst_v2");
        fs::create_dir_all(&model_dir).unwrap();
        fs::write(model_dir.join("install-manifest.json"), b"{}").unwrap();
        let worker = fixture.write_executable("roformer-worker");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default()
                .with_legacy_models_root(&fixture.root)
                .with_runtime_override("openvino_2026_3", worker),
        )
        .unwrap();
        let resource = ResourceRef::model("melband_roformer_inst_v2").unwrap();
        let status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Benchmark,
                Some(NativeBackend::OpenVino),
            )
            .unwrap();
        assert_eq!(status.install_state, InstallState::Legacy);
        assert!(!status.usable);
        assert!(status.reasons.contains(&ReadinessReason::Legacy));

        let testing_status = manager
            .status_with_backend(
                &resource,
                RuntimePolicy::Experimental,
                Some(NativeBackend::OpenVino),
            )
            .unwrap();
        assert!(!testing_status.runnable, "{testing_status:#?}");
        assert!(!testing_status.usable);
        assert!(
            testing_status
                .reasons
                .contains(&ReadinessReason::BackendUnvalidated)
        );
        assert!(testing_status.reasons.contains(&ReadinessReason::Legacy));
    }

    #[test]
    fn read_operations_do_not_create_a_configured_store() {
        let fixture = Fixture::new();
        let absent_store = fixture.root.join("not-created");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&absent_store),
        )
        .unwrap();
        let rmvpe = ResourceRef::model("rmvpe").unwrap();
        let _ = manager.list(RuntimePolicy::Production).unwrap();
        let _ = manager.show(&rmvpe, RuntimePolicy::Production).unwrap();
        let _ = manager.status(&rmvpe, RuntimePolicy::Production).unwrap();
        let _ = manager.paths_summary();
        let _ = manager.verify(&[rmvpe], RuntimePolicy::Production).unwrap();
        let _ = manager.doctor();
        assert!(!absent_store.exists());
    }

    #[test]
    fn verify_reports_corrupt_current_pointer_without_mutating_it() {
        let fixture = Fixture::new();
        fixture.write_raw_model_current("rmvpe", b"not json");
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let before = fixture.read_model_current("rmvpe");
        let report = manager
            .verify(
                &[ResourceRef::model("rmvpe").unwrap()],
                RuntimePolicy::Production,
            )
            .unwrap();
        let after = fixture.read_model_current("rmvpe");
        assert_eq!(before, after);
        assert_eq!(report.corrupt, vec![ResourceRef::model("rmvpe").unwrap()]);
    }

    #[test]
    fn doctor_is_read_only_and_reports_runtime_lock() {
        let fixture = Fixture::new();
        let manager = RuntimeManager::with_default_catalog(
            StorePaths::default().with_store_root(&fixture.root),
        )
        .unwrap();
        let before = fs::read_dir(&fixture.root).unwrap().count();
        let report = manager.doctor();
        let after = fs::read_dir(&fixture.root).unwrap().count();
        assert_eq!(before, after);
        assert!(report.checks.iter().any(|check| check.id == "runtime_lock"));
    }

    struct Fixture {
        root: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let stamp = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let root = std::env::temp_dir().join(format!(
                "uta-runtime-manager-test-{}-{stamp}",
                std::process::id()
            ));
            fs::create_dir_all(&root).unwrap();
            Self { root }
        }

        fn catalog_with_fixture_model(model_id: &str) -> ResourceCatalog {
            let mut catalog = ResourceCatalog::default_catalog().unwrap();
            let model = catalog.models.get_mut(model_id).unwrap();
            model.source.filename = Some("model.bin".to_string());
            model.source.sha256 = Some(format!("{:x}", Sha256::digest(b"managed fixture")));
            model.source.converted_artifact = None;
            catalog
        }

        fn write_model_current(&self, model_id: &str, _generation_label: &str) {
            let catalog = ResourceCatalog::default_catalog().unwrap();
            self.write_model_current_with_catalog(model_id, &catalog);
        }

        fn write_model_current_with_catalog(&self, model_id: &str, catalog: &ResourceCatalog) {
            let resource = ResourceRef::model(model_id).unwrap();
            let generation_root = self.root.join("models").join(model_id).join("generations");
            let payload = b"managed fixture";
            let payload_digest = format!("{:x}", Sha256::digest(payload));
            let model = catalog.model(model_id).unwrap();
            let manifest = crate::manifest::InstallManifest {
                schema: crate::manifest::INSTALL_MANIFEST_SCHEMA.to_string(),
                schema_version: crate::manifest::INSTALL_MANIFEST_SCHEMA_VERSION,
                resource,
                catalog_version: crate::catalog::RUNTIME_CATALOG_VERSION.to_string(),
                source: Some(model.source.clone()),
                source_sha256: model.source.sha256.clone(),
                model_recipe_digest: Some(model.recipe_digest.clone()),
                conversion_recipe_digest: None,
                runtime_recipe_digest: model.runtime_recipe_digest.clone(),
                files: vec![crate::manifest::InstalledFile {
                    path: PathBuf::from("model.bin"),
                    sha256: payload_digest,
                    size: payload.len() as u64,
                }],
                created_timestamp: "fixture".to_string(),
            };
            let manifest_bytes = serde_json::to_vec(&manifest).unwrap();
            let generation = crate::manifest::generation_id(&manifest_bytes);
            let generation_dir = generation_root.join(&generation);
            fs::create_dir_all(&generation_dir).unwrap();
            fs::write(generation_dir.join("model.bin"), payload).unwrap();
            fs::write(generation_dir.join("install-manifest.json"), manifest_bytes).unwrap();
            self.write_raw_model_current(
                model_id,
                format!(r#"{{"generation":"{generation}"}}"#).as_bytes(),
            );
        }

        fn write_raw_model_current(&self, model_id: &str, bytes: &[u8]) {
            let dir = self.root.join("models").join(model_id);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("current.json"), bytes).unwrap();
        }

        fn read_model_current(&self, model_id: &str) -> Vec<u8> {
            fs::read(self.root.join("models").join(model_id).join("current.json")).unwrap()
        }

        fn current_model_generation(&self, model_id: &str) -> PathBuf {
            let pointer: CurrentPointer =
                serde_json::from_slice(&self.read_model_current(model_id)).unwrap();
            self.root
                .join("models")
                .join(model_id)
                .join("generations")
                .join(pointer.generation)
        }

        fn write_executable(&self, name: &str) -> PathBuf {
            #[cfg(windows)]
            let path = self.root.join(format!("{name}.exe"));
            #[cfg(not(windows))]
            let path = self.root.join(name);
            fs::write(&path, b"fixture").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
            }
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
