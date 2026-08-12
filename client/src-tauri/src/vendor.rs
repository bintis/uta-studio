use app_core::{
    analysis_runtime_status as core_analysis_runtime_status, run_vendor_setup,
    AnalysisRuntimeStatus, CachePaths, ComputeBackend, ModelDownloadTarget, SetupFolders,
};
use tauri::{AppHandle, Emitter, Manager};

static SETUP_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

struct SetupJobGuard;

impl Drop for SetupJobGuard {
    fn drop(&mut self) {
        SETUP_RUNNING.store(false, std::sync::atomic::Ordering::Release);
    }
}

#[tauri::command]
pub fn trigger_setup(
    app: AppHandle,
    data_path: Option<String>,
    cache_paths: Option<CachePaths>,
    compute_backend: Option<ComputeBackend>,
    model_target: Option<ModelDownloadTarget>,
) -> Result<(), String> {
    SETUP_RUNNING
        .compare_exchange(
            false,
            true,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Acquire,
        )
        .map_err(|_| "Another analysis setup or model download is already running".to_string())?;
    let job_label = model_target
        .map(|target| format!("model {target:?}"))
        .unwrap_or_else(|| "analysis runtime".to_string());
    std::thread::spawn(move || {
        let _job_guard = SetupJobGuard;
        // The setup clears and replaces the Python runtime.  A persistent
        // analyzer may otherwise keep executing the replaced environment
        // after the UI reports that the selected backend was installed.
        app_core::shutdown_server();

        if let Some(paths) = cache_paths.as_ref() {
            for path in [&paths.songs, &paths.models, &paths.vendor]
                .into_iter()
                .flatten()
            {
                let _ = app.asset_protocol_scope().allow_directory(path, true);
            }
        }

        let app_for_progress = app.clone();
        let app_for_relocation = app.clone();
        if let Err(e) = run_vendor_setup(
            SetupFolders {
                data_path,
                cache_paths,
                compute_backend: compute_backend.unwrap_or_default(),
                model_target,
            },
            move |progress| {
                let _ = app_for_progress.emit("setup-progress", progress);
            },
            {
                let app_for_log = app.clone();
                move |line| {
                    let _ = app_for_log.emit("setup-log", line);
                }
            },
            move |new_path| {
                app_for_relocation
                    .asset_protocol_scope()
                    .allow_directory(new_path, true)
                    .map_err(|e| {
                        format!(
                            "Failed to update asset protocol scope for relocated data path {:?}: {e}",
                            new_path
                        )
                    })
            },
        ) {
            let _ = app.emit("setup-error", e);
        }
    });
    tracing::info!("Started {job_label} setup job");
    Ok(())
}

#[tauri::command]
pub fn analysis_runtime_status() -> AnalysisRuntimeStatus {
    core_analysis_runtime_status()
}
