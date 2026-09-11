use super::*;

#[test]
fn super_acceleration_is_captured_in_the_exact_request() {
    let settings = crate::analysis_experience::resolve_analysis_experience(
        &crate::analysis_experience::AnalysisExperienceSettings::default(),
        None,
        None,
    );
    let intent = AnalysisRequestIntent {
        turbo_acceleration: true,
        request_id: "super-acceleration".to_string(),
        source: ResolvedAnalysisSource {
            library_file_hash: "fixture".to_string(),
            path: std::env::temp_dir().join("uta-studio-super-acceleration.flac"),
            sha256: "a".repeat(64),
            role: AudioRoleWire::OriginalMix,
        },
        lyrics: StudioLyricsContext::default(),
        target_override: Some(AnalysisDefaultTarget::PitchEvidence),
        requested_outputs: None,
        compute_backend: Some("libtorch_xpu".to_string()),
        model_backend_overrides: BTreeMap::from([("rmvpe".to_string(), "ggml".to_string())]),
        default_device_class: Some("gpu".to_string()),
        model_device_overrides: BTreeMap::from([(
            "rmvpe".to_string(),
            "integrated_gpu".to_string(),
        )]),
    };
    let accelerated = compile_analyze_request(intent.clone(), &settings).unwrap();
    let mut ordinary_intent = intent;
    ordinary_intent.turbo_acceleration = false;
    let ordinary = compile_analyze_request(ordinary_intent, &settings).unwrap();
    assert!(accelerated.execution_policy.turbo_acceleration);
    assert!(!ordinary.execution_policy.turbo_acceleration);
    assert_eq!(
        accelerated.requested_artifacts,
        ordinary.requested_artifacts
    );
    assert_eq!(accelerated.analysis, ordinary.analysis);
    assert_eq!(accelerated.execution_policy.requested_backend, None);
    assert!(
        accelerated
            .execution_policy
            .model_backend_overrides
            .is_empty()
    );
    assert_eq!(accelerated.execution_policy.requested_device, None);
    assert!(
        accelerated
            .execution_policy
            .model_device_overrides
            .is_empty()
    );
    assert_eq!(
        ordinary.execution_policy.requested_backend,
        Some(NativeBackendWire::LibtorchXpu)
    );
    assert_eq!(
        ordinary.execution_policy.requested_device,
        Some(DeviceClassWire::Gpu)
    );
    assert_eq!(
        ordinary.execution_policy.model_backend_overrides["rmvpe"],
        NativeBackendWire::Ggml
    );
    assert_eq!(
        ordinary.execution_policy.model_device_overrides["rmvpe"],
        DeviceClassWire::IntegratedGpu
    );
    let exact = serde_json::to_value(&accelerated).unwrap();
    assert_eq!(exact["execution_policy"]["turbo_acceleration"], true);
    assert_eq!(exact["execution_policy"]["runtime_policy"], "production");
}
