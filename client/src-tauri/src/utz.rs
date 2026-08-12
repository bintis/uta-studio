use tauri::ipc::Channel;

#[tauri::command]
pub async fn export_utz(
    file_hash: String,
    output: String,
    on_event: Channel<app_core::ExportProgress>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app_core::export_utz_with_progress(&file_hash, &output, |progress| {
            let _ = on_event.send(progress);
        })
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("UTZ export task failed: {error}"))?
}

#[tauri::command]
pub async fn export_ultrastar(file_hash: String, output: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        app_core::export_ultrastar(&file_hash, &output)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| format!("UltraStar export task failed: {error}"))?
}
