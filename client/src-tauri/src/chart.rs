use std::path::Path;

use tauri::Manager;

#[tauri::command]
pub async fn chart_readiness(file_hash: String) -> Result<app_core::ChartReadiness, String> {
    tauri::async_runtime::spawn_blocking(move || app_core::chart_readiness(&file_hash))
        .await
        .map_err(|error| format!("Chart readiness task failed: {error}"))?
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn load_transcript(file_hash: String) -> Result<serde_json::Value, String> {
    app_core::load_transcript(&file_hash).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn load_chart(
    app: tauri::AppHandle,
    file_hash: String,
) -> Result<app_core::ChartDocument, String> {
    let chart = tauri::async_runtime::spawn_blocking(move || app_core::load_chart(&file_hash))
        .await
        .map_err(|error| format!("Chart load task failed: {error}"))?
        .map_err(|error| error.to_string())?;

    for path in [
        Some(chart.audio.instrumental.as_str()),
        chart.audio.vocals.as_deref(),
        Some(chart.audio.original.as_str()),
    ]
    .into_iter()
    .flatten()
    {
        if let Some(parent) = Path::new(path).parent() {
            app.asset_protocol_scope()
                .allow_directory(parent, false)
                .map_err(|error| format!("Could not authorize chart audio: {error}"))?;
        }
    }

    Ok(chart)
}

#[tauri::command]
pub async fn load_chart_audio(
    file_hash: String,
    source: String,
) -> Result<tauri::ipc::Response, String> {
    let bytes = tauri::async_runtime::spawn_blocking(move || {
        let chart = app_core::load_chart(&file_hash).map_err(|error| error.to_string())?;
        let path = match source.as_str() {
            "vocals" => chart
                .audio
                .vocals
                .as_deref()
                .unwrap_or(&chart.audio.instrumental),
            "instrumental" => &chart.audio.instrumental,
            "original" => &chart.audio.original,
            _ => return Err(format!("Unknown chart audio source: {source}")),
        };
        std::fs::read(path).map_err(|error| format!("Could not read chart audio: {error}"))
    })
    .await
    .map_err(|error| format!("Chart audio task failed: {error}"))??;

    Ok(tauri::ipc::Response::new(bytes))
}

#[tauri::command]
pub async fn save_chart(
    file_hash: String,
    transcript: serde_json::Value,
    pitch_notes: serde_json::Value,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        app_core::save_chart(&file_hash, transcript, pitch_notes)
    })
    .await
    .map_err(|error| format!("Chart save task failed: {error}"))?
    .map_err(|error| error.to_string())
}
