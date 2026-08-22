#[test]
fn runtime_status_is_read_only_and_reports_the_runtime_lock() {
    let status = analysis_runtime_status();
    assert_eq!(
        status.runtime_lock_sha256,
        crate::native_runtime::RUNTIME_LOCK_SHA256
    );
}

#[test]
fn production_model_statuses_name_the_native_families() {
    let statuses = model_install_statuses();
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
