mod analyzer;
mod audio_format;
mod authoring;
mod cache;
mod chart;
mod config;
mod error;
mod library_db;
mod library_menu;
mod library_model;
mod lrc;
mod lyrics;
mod scanner;
mod song;
mod source;
mod ultrastar_export;
mod usdx;
mod utz_export;
mod vendor;
mod vendor_scripts;

pub use analyzer::{
    AnalysisQueue, AnalysisTask, delete_cache, enqueue_all, enqueue_one, load_analysis_tasks,
    realign, reanalyze_force_transcribe, reanalyze_full, reanalyze_pitch, reanalyze_transcript,
    shutdown_server,
};
pub use authoring::{
    AudioPaths, ShiftDone, ShiftResult, get_audio_paths, load_pitch_guide, load_transcript,
    shift_key, shift_key_done_payload, shift_tempo, shift_tempo_done_payload,
};
pub use cache::{
    CacheDir, CachePaths, CacheStats, cache_roots, clear_models, default_uta_studio_dir,
    normalized_target_path, same_path, uta_studio_dir,
};
pub use chart::{
    ChartAudio, ChartDocument, ChartReadiness, chart_readiness, load_chart, save_chart,
};
pub use config::{AppConfig, LibrarySource};
pub use library_db::{init_library, library_db_path, load_song_by_hash};
pub use library_menu::{LibraryMenuItem, LibraryMenuItems, load_library_menu_items};
pub use library_model::{LibraryMenuFilters, LoadSongsParams, SongsMeta, SongsStore};
pub use lyrics::{
    LrclibCandidate, LyricsFile, apply_timed_lyrics, load_lyrics_file, provide_lrc,
    save_lyrics_and_realign, search_lrclib_for_hash,
};
pub use scanner::{clear_library_index, start_scan};
pub use song::{Song, SongOrigin};
pub use source::{
    FolderSource, LibraryFolderEntry, MediaSource, active_source, list_library_folder,
};
pub use ultrastar_export::export_ultrastar;
pub use utz_export::{
    ExportProgress, ExportableSong, export_utz, export_utz_with_progress, list_exportable_songs,
};
pub use vendor::{
    AnalysisRuntimeStatus, ComputeBackend, ModelDownloadTarget, ModelInstallStatus, SetupFolders,
    SetupProgress, SetupStep, SetupTask, SetupTaskState, analysis_runtime_status, is_ready,
    mark_ready, model_install_statuses, refresh_analyzer_scripts_if_ready, resolve_data_path_input,
    run_vendor_setup, step_create_venv, step_download_ffmpeg, step_download_model,
    step_download_pitch_model, step_download_selected_models, step_download_uv,
    step_extract_scripts, step_install_packages, step_install_python,
};

pub fn startup() -> Result<(), String> {
    init_library().map_err(|e| e.to_string())?;

    AnalysisQueue::clear();

    if let Err(e) = refresh_analyzer_scripts_if_ready() {
        tracing::warn!("Failed to refresh analyzer scripts: {e}");
    }

    if AppConfig::load().auto_analyze() && is_ready() {
        analyzer::enqueue_all(&LibraryMenuFilters::default());
    }

    Ok(())
}
