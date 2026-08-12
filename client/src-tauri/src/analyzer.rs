use app_core::{
    delete_cache as core_delete_cache, enqueue_all as core_enqueue_all,
    enqueue_one as core_enqueue_one, realign as core_realign,
    reanalyze_force_transcribe as core_reanalyze_force_transcribe,
    reanalyze_full as core_reanalyze_full, reanalyze_pitch as core_reanalyze_pitch,
    reanalyze_transcript as core_reanalyze_transcript, shift_key_done_payload,
    shift_tempo_done_payload, LibraryMenuFilters,
};
use tauri::{AppHandle, Emitter};

fn analysis_runtime_requirement(ready: bool) -> Result<(), String> {
    if ready {
        Ok(())
    } else {
        Err("Analysis runtime or models are not ready. Open Settings > Analysis to install or repair them.".to_string())
    }
}

fn require_analysis_runtime() -> Result<(), String> {
    analysis_runtime_requirement(app_core::is_ready())
}

#[cfg(test)]
mod tests {
    use super::analysis_runtime_requirement;

    #[test]
    fn missing_runtime_blocks_analysis_with_settings_guidance() {
        let message = analysis_runtime_requirement(false).unwrap_err();
        assert!(message.contains("Settings > Analysis"));
    }
}

#[tauri::command]
pub fn enqueue_one(file_hash: String) -> Result<(), String> {
    require_analysis_runtime()?;
    core_enqueue_one(&file_hash);
    Ok(())
}

#[tauri::command]
pub fn enqueue_all(filters: LibraryMenuFilters) -> Result<(), String> {
    require_analysis_runtime()?;
    core_enqueue_all(&filters);
    Ok(())
}

#[tauri::command]
pub fn delete_song_cache(file_hash: String) {
    core_delete_cache(&file_hash);
}

#[tauri::command]
pub fn reanalyze_transcript(file_hash: String, language: Option<String>) -> Result<(), String> {
    require_analysis_runtime()?;
    core_reanalyze_transcript(&file_hash, language);
    Ok(())
}

#[tauri::command]
pub fn reanalyze_full(file_hash: String) -> Result<(), String> {
    require_analysis_runtime()?;
    core_reanalyze_full(&file_hash);
    Ok(())
}

#[tauri::command]
pub fn reanalyze_pitch(file_hash: String) -> Result<(), String> {
    require_analysis_runtime()?;
    core_reanalyze_pitch(&file_hash);
    Ok(())
}

#[tauri::command]
pub fn realign(file_hash: String, language: Option<String>) -> Result<(), String> {
    require_analysis_runtime()?;
    core_realign(&file_hash, language);
    Ok(())
}

#[tauri::command]
pub fn reanalyze_force_transcribe(file_hash: String) -> Result<(), String> {
    require_analysis_runtime()?;
    core_reanalyze_force_transcribe(&file_hash);
    Ok(())
}

#[tauri::command]
pub fn shift_key(
    app: AppHandle,
    file_hash: String,
    key: String,
    pitch_ratio: f64,
    key_offset: i32,
) {
    std::thread::spawn(move || {
        let payload = shift_key_done_payload(file_hash, key, pitch_ratio, key_offset);
        let _ = app.emit("shift-key-done", payload);
    });
}

#[tauri::command]
pub fn shift_tempo(app: AppHandle, file_hash: String, tempo: f64) {
    std::thread::spawn(move || {
        let payload = shift_tempo_done_payload(file_hash, tempo);
        let _ = app.emit("shift-tempo-done", payload);
    });
}
