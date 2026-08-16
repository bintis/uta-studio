mod analyzer;
mod api;
mod audio_format;
mod authoring;
mod cache;
mod chart;
mod config;
mod editor;
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
mod vocal_chart;

pub use analyzer::{
    AnalysisProgressSnapshot, AnalysisQueue, AnalysisRunHistory, AnalysisStageRoute, AnalysisTask,
    QueuedStatus, clear_analysis_history, delete_cache, enqueue_all, enqueue_one,
    load_analysis_history, load_analysis_tasks, realign, reanalyze_force_transcribe,
    reanalyze_full, reanalyze_pitch, reanalyze_transcript, shutdown_server,
};
pub use api::{API_CAPABILITIES, ApiCapability, api_capabilities};
pub use authoring::{
    AudioPaths, ShiftDone, ShiftResult, get_audio_paths, load_pitch_guide, load_transcript,
    shift_key, shift_key_done_payload, shift_tempo, shift_tempo_done_payload,
};
pub use cache::{
    CacheDir, CachePaths, CacheStats, cache_roots, clear_models, default_uta_studio_dir,
    normalized_target_path, same_path, uta_studio_dir,
};
pub use chart::{
    ChartAudio, ChartDocument, ChartReadiness, ChartWaveform, chart_readiness,
    decode_chart_waveform, load_chart, save_vocal_chart,
};
pub use config::{AppConfig, LibrarySource};
pub use editor::{
    ChartLyric, ChartNote, ChartProblem, ClipboardNote, EDITOR_ACTIONS, EditorActionAccess,
    EditorActionDef, EditorActionGroup, EditorDocument, KeyChord, LyricAddress, MIN_NOTE_SECONDS,
    NoteKind, ProblemKind, ProblemReport, Severity, Syllable, TrackRole, TrackSummary,
    editor_action, editor_action_for_chord, editor_actions, kana_morae, syllables,
};
pub use library_db::{init_library, library_db_path, load_song_by_hash};
pub use library_menu::{LibraryMenuItem, LibraryMenuItems, load_library_menu_items};
pub use library_model::{LibraryMenuFilters, LoadSongsParams, SongsMeta, SongsStore};
pub use lyrics::{
    LrclibCandidate, LyricsFile, apply_timed_lyrics, load_lyrics_file, provide_lrc,
    save_lyrics_and_realign, search_lrclib_for_hash,
};
pub use scanner::{clear_library_index, start_scan};
pub use song::{
    MusicAnalysis, MusicAnalysisDescriptors, MusicKeyAnalysis, MusicRhythmAnalysis, Song,
    SongOrigin, TranscriptSource, load_music_analysis, update_song_settings,
};
pub use source::{
    FolderSource, LibraryFolderEntry, MediaSource, active_source, list_library_folder,
};
pub use ultrastar_export::{export_ultrastar, validate_ultrastar_chart};
pub use utz::VocalChartV1;
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
pub use vocal_chart::migrate_analyzer_chart;

pub fn startup() -> Result<(), String> {
    // Load and repair configuration before opening SQLite: the configured
    // data root decides which library database belongs to this process.
    let config = AppConfig::load();
    init_library().map_err(|e| e.to_string())?;

    if let Err(e) = refresh_analyzer_scripts_if_ready() {
        tracing::warn!("Failed to refresh analyzer scripts: {e}");
    }

    if is_ready() {
        // The queue is persisted so a desktop restart must not erase work the
        // user explicitly requested. An interrupted `Analyzing` entry is safe
        // to resume from the beginning because generated files are committed
        // by the analyzer only after each stage succeeds.
        let resumable = AnalysisQueue::load()
            .entries
            .into_iter()
            .filter_map(|(file_hash, status)| {
                matches!(
                    status,
                    analyzer::QueuedStatus::Queued | analyzer::QueuedStatus::Analyzing(_)
                )
                .then_some(file_hash)
            })
            .collect::<Vec<_>>();
        for file_hash in resumable {
            analyzer::enqueue_one(&file_hash);
        }
        if config.auto_analyze() {
            analyzer::enqueue_all(&LibraryMenuFilters::default());
        }
    }

    Ok(())
}
