mod analyzer;
mod cache;
mod chart;
mod config;
mod diagnostics;
mod editor_audio;
mod logging;
mod lyrics;
mod scanner;
mod utz;
mod vendor;

use analyzer::{
    delete_song_cache, enqueue_all, enqueue_one, realign, reanalyze_force_transcribe,
    reanalyze_full, reanalyze_pitch, reanalyze_transcript, shift_key, shift_tempo,
};
use app_core::{AppConfig, SongsStore};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use cache::{calculate_cache_stats, clear_all, clear_models_command};
use chart::{chart_readiness, load_chart, load_chart_audio, load_transcript, save_chart};
use config::{load_config, save_config};
use diagnostics::{api_capabilities, run_feature_diagnostics};
use editor_audio::{
    editor_audio_load, editor_audio_pause, editor_audio_play, editor_audio_seek,
    editor_audio_status, editor_audio_stop, EditorAudioPlayer,
};
use lyrics::{apply_timed_lyrics, load_lyrics, provide_lrc, save_lyrics, search_lrclib_lyrics};
use scanner::{
    add_library_folder, clear_library_source, list_library_folder, load_analysis_queue,
    load_analysis_tasks, load_library_menu_items, load_song_by_hash, load_songs, load_songs_meta,
    open_library_entry, remove_library_folder, reveal_library_entry, set_library_source,
    trigger_scan,
};
use tauri::{Manager, RunEvent, WebviewWindowBuilder};
use tracing::{info, warn};
use utz::{export_ultrastar, export_utz};
use vendor::{analysis_runtime_status, trigger_setup};

#[tauri::command]
fn frontend_ready(window: tauri::Window) {
    info!("[window] Frontend ready; showing main window");
    window.show().unwrap();
}

#[tauri::command]
fn get_log_path() -> String {
    app_core::default_uta_studio_dir()
        .join("uta-studio.log")
        .to_string_lossy()
        .into_owned()
}

#[tauri::command]
fn get_recent_logs() -> Vec<String> {
    logging::recent_logs()
}

/// True for native fullscreen or macOS "simple" fullscreen (`set_simple_fullscreen`), where
/// `isFullscreen()` stays false but the window fills the screen.
#[tauri::command]
fn window_immersive(window: tauri::WebviewWindow) -> Result<bool, String> {
    if window.is_fullscreen().map_err(|e| e.to_string())? {
        return Ok(true);
    }
    #[cfg(target_os = "macos")]
    {
        let inner = window.inner_size().map_err(|e| e.to_string())?;
        if let Some(monitor) = window.current_monitor().map_err(|e| e.to_string())? {
            let ms = monitor.size();
            let dw = (inner.width as i32 - ms.width as i32).abs();
            let dh = (inner.height as i32 - ms.height as i32).abs();
            if dw <= 2 && dh <= 2 {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// macOS simple fullscreen clears `Miniaturizable`; exit that mode before minimizing.
#[tauri::command]
fn minimize_window(window: tauri::WebviewWindow) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_simple_fullscreen(false);
    }
    window.minimize().map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();

    tauri::Builder::default()
        .manage(EditorAudioPlayer::new())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_process::init())
        .invoke_handler(tauri::generate_handler![
            // Init
            frontend_ready,
            get_log_path,
            get_recent_logs,
            window_immersive,
            minimize_window,
            // Config
            load_config,
            save_config,
            // Discoverable feature API and non-destructive verification
            api_capabilities,
            run_feature_diagnostics,
            // Cache
            calculate_cache_stats,
            clear_models_command,
            clear_all,
            // Scanner
            trigger_scan,
            set_library_source,
            add_library_folder,
            remove_library_folder,
            list_library_folder,
            open_library_entry,
            reveal_library_entry,
            clear_library_source,
            load_songs,
            load_song_by_hash,
            load_songs_meta,
            load_analysis_queue,
            load_analysis_tasks,
            load_library_menu_items,
            // Analyzer
            enqueue_one,
            enqueue_all,
            delete_song_cache,
            reanalyze_transcript,
            reanalyze_full,
            reanalyze_pitch,
            realign,
            reanalyze_force_transcribe,
            shift_key,
            shift_tempo,
            // Lyrics
            load_lyrics,
            search_lrclib_lyrics,
            save_lyrics,
            provide_lrc,
            apply_timed_lyrics,
            // Package export
            export_utz,
            export_ultrastar,
            // Chart editor
            chart_readiness,
            load_chart,
            load_chart_audio,
            save_chart,
            editor_audio_load,
            editor_audio_play,
            editor_audio_pause,
            editor_audio_seek,
            editor_audio_status,
            editor_audio_stop,
            // Authoring data
            load_transcript,
            // Vendor
            analysis_runtime_status,
            trigger_setup
        ])
        .setup(|app| {
            let _ = dotenvy::dotenv();
            app_core::startup()?;

            let config = AppConfig::load();
            app.handle()
                .asset_protocol_scope()
                .allow_directory(config.effective_data_path(), true)
                .map_err(|e| format!("failed to allow asset protocol for data path: {e}"))?;
            app.handle()
                .asset_protocol_scope()
                .allow_directory(app_core::default_uta_studio_dir(), true)
                .map_err(|e| format!("failed to allow asset protocol for default path: {e}"))?;
            for root in app_core::cache_roots() {
                app.handle()
                    .asset_protocol_scope()
                    .allow_directory(root, true)
                    .map_err(|e| format!("failed to allow asset protocol for cache path: {e}"))?;
            }
            let json = serde_json::to_string(&config).map_err(|e| e.to_string())?;
            let b64 = B64.encode(json.as_bytes());

            let songs_meta = SongsStore::load_meta();
            let meta_json = serde_json::to_string(&songs_meta).map_err(|e| e.to_string())?;
            let meta_b64 = B64.encode(meta_json.as_bytes());

            let init_script = format!(
                "window.__UTA_STUDIO_APP_CONFIG__ = JSON.parse(atob('{b64}')); \
                 window.__UTA_STUDIO_SONGS_META__ = JSON.parse(atob('{meta_b64}'));",
            );

            let window_config = app
                .config()
                .app
                .windows
                .first()
                .ok_or_else(|| "tauri.conf.json must define at least one window".to_string())?;

            let window = WebviewWindowBuilder::from_config(app.handle(), window_config)
                .map_err(|e| e.to_string())?
                .initialization_script(init_script)
                .build()
                .map_err(|e| e.to_string())?;

            // The chart editor uses local media for precise auditioning.
            #[cfg(target_os = "linux")]
            window
                .with_webview(|platform_webview| {
                    use webkit2gtk::{SettingsExt, WebViewExt};

                    if let Some(settings) = platform_webview.inner().settings() {
                        settings.set_enable_media(true);
                        settings.set_enable_webaudio(true);
                        settings.set_media_playback_allows_inline(true);
                        settings.set_media_playback_requires_user_gesture(true);
                        settings.set_enable_write_console_messages_to_stdout(true);
                    }
                })
                .map_err(|e| format!("failed to configure WebKit media audition: {e}"))?;

            if config.fullscreen == Some(true) {
                let _ = window.set_simple_fullscreen(true);
            }

            // The main window starts hidden to avoid a white flash while the
            // frontend hydrates. Never leave the process invisible forever if
            // WebKit fails before it can send `frontend_ready` (which would
            // otherwise make `nix run .` look like it has hung).
            let fallback_window = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_secs(5));
                if fallback_window.is_visible().unwrap_or(true) {
                    return;
                }
                warn!("[window] Frontend-ready timeout; showing main window as fallback");
                let _ = fallback_window.show();
            });

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|_app, event| {
            if let RunEvent::Exit = event {
                app_core::shutdown_server();
            }
        });
}
