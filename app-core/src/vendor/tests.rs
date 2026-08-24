fn isolated_runtime_client(label: &str) -> crate::backend_cli::RuntimeCliClient {
    crate::backend_cli::RuntimeCliClient::discover()
        .expect("uta-runtime debug CLI is required for process-contract tests")
        .with_store(std::env::temp_dir().join(format!("uta-studio-runtime-{label}-{}", std::process::id())))
}

#[test]
fn runtime_status_is_read_only_and_reports_the_runtime_lock() {
    let client = isolated_runtime_client("status");
    let status = analysis_runtime_status_with_clients(
        true,
        Some(&client),
        std::path::PathBuf::from("missing-ffmpeg"),
    );
    assert_eq!(status.runtime_lock_sha256, crate::native_runtime::RUNTIME_LOCK_SHA256);
    let serialized = serde_json::to_value(&status).expect("runtime status serializes");
    assert!(serialized.get("openvinoRuntimeAvailable").is_some());
}

#[test]
fn production_model_statuses_name_the_native_families() {
    let statuses = model_install_statuses_with_client(&isolated_runtime_client("models"));
    for target in [
        ModelDownloadTarget::RoFormer,
        ModelDownloadTarget::FireRed,
        ModelDownloadTarget::QwenAsr,
        ModelDownloadTarget::QwenAlign,
        ModelDownloadTarget::Pitch,
    ] {
        assert!(statuses.iter().any(|status| status.target == target));
    }
}
