use app_core::{
    AnalysisQueue, AppConfig, LibraryMenuItems, LibrarySource, LoadSongsParams, SongsMeta,
    SongsStore,
};

#[tauri::command]
pub fn trigger_scan() {
    app_core::start_scan();
}

#[tauri::command]
pub fn set_library_source(source: LibrarySource) -> AppConfig {
    let mut config = AppConfig::load();
    config.library_source = Some(source);
    config.save();
    app_core::start_scan();
    config
}

#[tauri::command]
pub fn add_library_folder(path: std::path::PathBuf) -> AppConfig {
    let mut config = AppConfig::load();
    let mut paths = config.library_paths();
    if !paths.contains(&path) {
        paths.push(path);
    }
    config.library_source = Some(LibrarySource::Folders { paths });
    config.save();
    app_core::start_scan();
    config
}

#[tauri::command]
pub fn remove_library_folder(path: std::path::PathBuf) -> AppConfig {
    let mut config = AppConfig::load();
    let mut paths = config.library_paths();
    paths.retain(|entry| entry != &path);
    config.library_source = if paths.is_empty() {
        None
    } else {
        Some(LibrarySource::Folders { paths })
    };
    config.save();
    if config.library_source.is_some() {
        app_core::start_scan();
    } else {
        app_core::clear_library_index();
    }
    config
}

#[tauri::command]
pub fn list_library_folder(
    path: std::path::PathBuf,
) -> Result<Vec<app_core::LibraryFolderEntry>, String> {
    app_core::list_library_folder(&path).map_err(|error| error.to_string())
}

fn validate_library_entry(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let config = AppConfig::load();
    let requested = std::fs::canonicalize(path).map_err(|error| error.to_string())?;
    let allowed = config.library_paths().iter().any(|root| {
        std::fs::canonicalize(root)
            .map(|root| requested.starts_with(root))
            .unwrap_or(false)
    });
    allowed
        .then_some(requested)
        .ok_or_else(|| "Path is outside the configured library".to_string())
}

#[tauri::command]
pub fn open_library_entry(path: std::path::PathBuf) -> Result<(), String> {
    let path = validate_library_entry(&path)?;
    tauri_plugin_opener::open_path(path, None::<&str>).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn reveal_library_entry(path: std::path::PathBuf) -> Result<(), String> {
    let path = validate_library_entry(&path)?;
    tauri_plugin_opener::reveal_item_in_dir(path).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn clear_library_source() -> AppConfig {
    let mut config = AppConfig::load();
    config.library_source = None;
    config.save();
    app_core::clear_library_index();
    config
}

#[tauri::command]
pub fn load_songs(params: LoadSongsParams) -> SongsStore {
    SongsStore::load(&params)
}

#[tauri::command]
pub fn load_songs_meta() -> SongsMeta {
    SongsStore::load_meta()
}

#[tauri::command]
pub fn load_song_by_hash(file_hash: String) -> Result<Option<app_core::Song>, String> {
    app_core::load_song_by_hash(&file_hash).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn load_analysis_queue() -> AnalysisQueue {
    AnalysisQueue::load()
}

#[tauri::command]
pub fn load_analysis_tasks() -> Vec<app_core::AnalysisTask> {
    app_core::load_analysis_tasks()
}

#[tauri::command]
pub fn load_library_menu_items() -> Result<LibraryMenuItems, String> {
    app_core::load_library_menu_items().map_err(|e| e.to_string())
}
