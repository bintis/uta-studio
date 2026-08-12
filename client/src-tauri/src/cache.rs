use app_core::{clear_models, CacheDir, CacheStats};

#[tauri::command]
pub fn calculate_cache_stats() -> CacheStats {
    CacheStats::calculate()
}

#[tauri::command]
pub fn clear_models_command() {
    clear_models();
}

#[tauri::command]
pub fn clear_all() {
    CacheDir::new().clear_all();
    clear_models();
}
