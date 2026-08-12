use std::{
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use app_core::{AnalysisQueue, AppConfig, CacheStats, SongsStore};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiCapability {
    pub area: &'static str,
    pub command: &'static str,
    pub access: &'static str,
    pub automated_check: bool,
    pub description: &'static str,
}

macro_rules! capability {
    ($area:literal, $command:literal, $access:literal, $checked:literal, $description:literal) => {
        ApiCapability {
            area: $area,
            command: $command,
            access: $access,
            automated_check: $checked,
            description: $description,
        }
    };
}

/// Discoverable catalogue for every app-owned command. Destructive endpoints
/// are intentionally visible here but are never invoked by diagnostics.
#[tauri::command]
pub fn api_capabilities() -> Vec<ApiCapability> {
    vec![
        capability!(
            "app",
            "frontend_ready",
            "mutation",
            false,
            "Show the hydrated app window"
        ),
        capability!(
            "app",
            "get_log_path",
            "read",
            true,
            "Resolve the local application log"
        ),
        capability!(
            "app",
            "get_recent_logs",
            "read",
            true,
            "Read the in-memory diagnostic log"
        ),
        capability!(
            "window",
            "window_immersive",
            "read",
            false,
            "Read fullscreen state"
        ),
        capability!(
            "window",
            "minimize_window",
            "mutation",
            false,
            "Minimize the app window"
        ),
        capability!(
            "config",
            "load_config",
            "read",
            true,
            "Load persisted settings"
        ),
        capability!(
            "config",
            "save_config",
            "mutation",
            false,
            "Persist settings"
        ),
        capability!(
            "storage",
            "calculate_cache_stats",
            "read",
            true,
            "Measure cache usage"
        ),
        capability!(
            "storage",
            "clear_models_command",
            "destructive",
            false,
            "Delete downloaded models"
        ),
        capability!(
            "storage",
            "clear_all",
            "destructive",
            false,
            "Delete generated cache and models"
        ),
        capability!(
            "library",
            "trigger_scan",
            "mutation",
            false,
            "Start a library scan"
        ),
        capability!(
            "library",
            "set_library_source",
            "mutation",
            false,
            "Replace watched folders"
        ),
        capability!(
            "library",
            "add_library_folder",
            "mutation",
            false,
            "Add a watched folder"
        ),
        capability!(
            "library",
            "remove_library_folder",
            "mutation",
            false,
            "Remove a watched folder"
        ),
        capability!(
            "library",
            "list_library_folder",
            "read",
            true,
            "Browse an authorized folder"
        ),
        capability!(
            "library",
            "open_library_entry",
            "external",
            false,
            "Open media with the OS"
        ),
        capability!(
            "library",
            "reveal_library_entry",
            "external",
            false,
            "Reveal media in the OS file manager"
        ),
        capability!(
            "library",
            "clear_library_source",
            "destructive",
            false,
            "Disconnect folders and clear the index"
        ),
        capability!("library", "load_songs", "read", true, "Query songs"),
        capability!(
            "library",
            "load_song_by_hash",
            "read",
            true,
            "Load one song"
        ),
        capability!(
            "library",
            "load_songs_meta",
            "read",
            true,
            "Load library counters"
        ),
        capability!(
            "library",
            "load_analysis_queue",
            "read",
            true,
            "Load analysis queue state"
        ),
        capability!(
            "library",
            "load_analysis_tasks",
            "read",
            true,
            "Load named analysis tasks"
        ),
        capability!(
            "library",
            "load_library_menu_items",
            "read",
            true,
            "Load sidebar facets"
        ),
        capability!(
            "analysis",
            "enqueue_one",
            "mutation",
            false,
            "Queue one song for analysis"
        ),
        capability!(
            "analysis",
            "enqueue_all",
            "mutation",
            false,
            "Queue a filtered set for analysis"
        ),
        capability!(
            "analysis",
            "delete_song_cache",
            "destructive",
            false,
            "Delete generated data for one song"
        ),
        capability!(
            "analysis",
            "reanalyze_transcript",
            "mutation",
            false,
            "Regenerate the transcript"
        ),
        capability!(
            "analysis",
            "reanalyze_full",
            "mutation",
            false,
            "Regenerate all analysis assets"
        ),
        capability!(
            "analysis",
            "reanalyze_pitch",
            "mutation",
            false,
            "Regenerate pitch assets"
        ),
        capability!(
            "analysis",
            "realign",
            "mutation",
            false,
            "Realign timed lyrics"
        ),
        capability!(
            "analysis",
            "reanalyze_force_transcribe",
            "mutation",
            false,
            "Force transcription"
        ),
        capability!(
            "authoring",
            "shift_key",
            "mutation",
            false,
            "Render a key variant"
        ),
        capability!(
            "authoring",
            "shift_tempo",
            "mutation",
            false,
            "Render a tempo variant"
        ),
        capability!("lyrics", "load_lyrics", "read", true, "Load local lyrics"),
        capability!(
            "lyrics",
            "search_lrclib_lyrics",
            "external",
            false,
            "Search LRCLIB"
        ),
        capability!(
            "lyrics",
            "save_lyrics",
            "mutation",
            false,
            "Save edited lyrics"
        ),
        capability!(
            "lyrics",
            "provide_lrc",
            "mutation",
            false,
            "Import an LRC file"
        ),
        capability!(
            "lyrics",
            "apply_timed_lyrics",
            "mutation",
            false,
            "Apply timed lyrics"
        ),
        capability!(
            "export",
            "export_utz",
            "mutation",
            true,
            "Export a validated UTZ package"
        ),
        capability!(
            "export",
            "export_ultrastar",
            "mutation",
            true,
            "Export an UltraStar text bundle"
        ),
        capability!(
            "editor",
            "chart_readiness",
            "read",
            true,
            "Check editor prerequisites"
        ),
        capability!(
            "editor",
            "load_chart",
            "read",
            true,
            "Load editable chart data"
        ),
        capability!(
            "editor",
            "load_chart_audio",
            "read",
            true,
            "Stream local chart audio bytes"
        ),
        capability!(
            "editor audio",
            "editor_audio_load",
            "mutation",
            true,
            "Load chart audio into the native player"
        ),
        capability!(
            "editor audio",
            "editor_audio_play",
            "mutation",
            false,
            "Start native chart audition playback"
        ),
        capability!(
            "editor audio",
            "editor_audio_pause",
            "mutation",
            false,
            "Pause native chart audition playback"
        ),
        capability!(
            "editor audio",
            "editor_audio_seek",
            "mutation",
            false,
            "Seek native chart audition playback"
        ),
        capability!(
            "editor audio",
            "editor_audio_status",
            "read",
            true,
            "Read native chart audition position and state"
        ),
        capability!(
            "editor audio",
            "editor_audio_stop",
            "mutation",
            false,
            "Release the native chart audition pipeline"
        ),
        capability!(
            "editor",
            "save_chart",
            "mutation",
            false,
            "Persist edited words and notes"
        ),
        capability!(
            "authoring",
            "load_transcript",
            "read",
            true,
            "Load the active transcript"
        ),
        capability!(
            "models",
            "analysis_runtime_status",
            "read",
            true,
            "Inspect tools, models, and backend"
        ),
        capability!(
            "models",
            "trigger_setup",
            "external",
            false,
            "Install the shared runtime or one explicitly selected model family"
        ),
        capability!(
            "diagnostics",
            "api_capabilities",
            "read",
            true,
            "List feature API contracts"
        ),
        capability!(
            "diagnostics",
            "run_feature_diagnostics",
            "temporary",
            true,
            "Run non-destructive feature checks"
        ),
    ]
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticRequest {
    pub file_hash: Option<String>,
    #[serde(default)]
    pub include_export_smoke: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticCheck {
    pub id: &'static str,
    pub status: &'static str,
    pub detail: String,
    pub elapsed_ms: u128,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticReport {
    pub ok: bool,
    pub generated_at_ms: u128,
    pub capabilities: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub checks: Vec<DiagnosticCheck>,
}

fn timed_check(
    id: &'static str,
    check: impl FnOnce() -> Result<String, String>,
) -> DiagnosticCheck {
    let started = std::time::Instant::now();
    match check() {
        Ok(detail) => DiagnosticCheck {
            id,
            status: "passed",
            detail,
            elapsed_ms: started.elapsed().as_millis(),
        },
        Err(detail) => DiagnosticCheck {
            id,
            status: "failed",
            detail,
            elapsed_ms: started.elapsed().as_millis(),
        },
    }
}

fn skipped(id: &'static str, detail: impl Into<String>) -> DiagnosticCheck {
    DiagnosticCheck {
        id,
        status: "skipped",
        detail: detail.into(),
        elapsed_ms: 0,
    }
}

fn decode_audio(path: &Path) -> Result<String, String> {
    if !path.is_file() {
        return Err(format!("Audio file is missing: {}", path.display()));
    }
    let runtime = app_core::analysis_runtime_status();
    let ffmpeg = runtime
        .ffmpeg_path
        .map(PathBuf::from)
        .ok_or_else(|| "System ffmpeg is unavailable".to_string())?;
    let status = Command::new(ffmpeg)
        .args(["-v", "error", "-t", "1", "-i"])
        .arg(path)
        .args(["-f", "null", "-"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .status()
        .map_err(|error| error.to_string())?;
    if status.success() {
        let native = crate::editor_audio::probe_audio(path)?;
        Ok(format!("Decoded one second with ffmpeg; {native}"))
    } else {
        Err(format!(
            "ffmpeg could not decode {} ({status})",
            path.display()
        ))
    }
}

fn smoke_exports(file_hash: &str) -> Result<String, String> {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "uta-studio-diagnostics-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir(&root).map_err(|error| error.to_string())?;
    let result = (|| {
        let utz = root.join("diagnostic.utz");
        let ultrastar = root.join("diagnostic.txt");
        app_core::export_utz(file_hash, &utz).map_err(|error| error.to_string())?;
        app_core::export_ultrastar(file_hash, &ultrastar).map_err(|error| error.to_string())?;
        let utz_bytes = File::open(&utz)
            .and_then(|file| file.metadata())
            .map_err(|error| error.to_string())?
            .len();
        let ultrastar_bytes = File::open(&ultrastar)
            .and_then(|file| file.metadata())
            .map_err(|error| error.to_string())?
            .len();
        if utz_bytes == 0 || ultrastar_bytes == 0 {
            return Err("An export was empty".to_string());
        }
        Ok(format!(
            "UTZ {utz_bytes} bytes; UltraStar chart {ultrastar_bytes} bytes"
        ))
    })();
    let cleanup = std::fs::remove_dir_all(&root).map_err(|error| error.to_string());
    match (result, cleanup) {
        (Ok(detail), Ok(())) => Ok(detail),
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(format!("Exports passed, but temp cleanup failed: {error}")),
    }
}

fn run_feature_diagnostics_blocking(request: DiagnosticRequest) -> DiagnosticReport {
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut checks = Vec::new();

    let config = AppConfig::load();
    checks.push(timed_check("config.load", || {
        serde_json::to_string(&config)
            .map(|json| format!("Settings serialized ({} bytes)", json.len()))
            .map_err(|error| error.to_string())
    }));
    checks.push(timed_check("storage.cache", || {
        let stats = CacheStats::calculate();
        Ok(format!(
            "songs={} models={} other={} bytes",
            stats.songs_bytes, stats.models_bytes, stats.other_bytes
        ))
    }));
    checks.push(timed_check("library.database", || {
        let meta = SongsStore::load_meta();
        Ok(format!(
            "{} indexed; {} analyzed",
            meta.songs_count, meta.analyzed_count
        ))
    }));
    checks.push(timed_check("library.facets", || {
        app_core::load_library_menu_items()
            .map(|_| "Sidebar facets loaded".to_string())
            .map_err(|error| error.to_string())
    }));
    checks.push(timed_check("analysis.queue", || {
        let queue = AnalysisQueue::load();
        Ok(format!("{} queued or active items", queue.entries.len()))
    }));
    checks.push(timed_check("models.runtime", || {
        let status = app_core::analysis_runtime_status();
        serde_json::to_string(&status)
            .map(|_| {
                if status.ready {
                    "Runtime and selected models are ready".to_string()
                } else {
                    format!("Runtime inspected; missing: {}", status.missing.join(", "))
                }
            })
            .map_err(|error| error.to_string())
    }));

    let paths = config.library_paths();
    if paths.is_empty() {
        checks.push(skipped("library.folders", "No watched folders configured"));
    } else {
        checks.push(timed_check("library.folders", || {
            let mut entries = 0usize;
            for path in &paths {
                entries += app_core::list_library_folder(path)
                    .map_err(|error| format!("{}: {error}", path.display()))?
                    .len();
            }
            Ok(format!(
                "Browsed {} folders and {entries} entries",
                paths.len()
            ))
        }));
    }

    let selected_hash = request.file_hash.or_else(|| {
        app_core::list_exportable_songs()
            .ok()?
            .into_iter()
            .find(|song| song.ready)
            .map(|song| song.file_hash)
    });
    if let Some(file_hash) = selected_hash {
        checks.push(timed_check("song.load", || {
            app_core::load_song_by_hash(&file_hash)
                .map_err(|error| error.to_string())?
                .map(|song| format!("{} — {}", song.artist, song.title))
                .ok_or_else(|| format!("Song not found: {file_hash}"))
        }));
        checks.push(timed_check("editor.readiness", || {
            let readiness =
                app_core::chart_readiness(&file_hash).map_err(|error| error.to_string())?;
            if readiness.ready {
                Ok("Chart is editor-ready".to_string())
            } else {
                Err(readiness
                    .blocked_reason
                    .unwrap_or_else(|| format!("Missing: {}", readiness.missing.join(", "))))
            }
        }));
        let chart = app_core::load_chart(&file_hash);
        checks.push(match &chart {
            Ok(chart) => DiagnosticCheck {
                id: "editor.chart",
                status: "passed",
                detail: format!(
                    "Chart loaded; {} compatibility repairs",
                    chart.repaired_issues.len()
                ),
                elapsed_ms: 0,
            },
            Err(error) => DiagnosticCheck {
                id: "editor.chart",
                status: "failed",
                detail: error.to_string(),
                elapsed_ms: 0,
            },
        });
        if let Ok(chart) = chart {
            checks.push(timed_check("editor.audio", || {
                decode_audio(Path::new(&chart.audio.instrumental))
            }));
        } else {
            checks.push(skipped("editor.audio", "Chart could not be loaded"));
        }
        if request.include_export_smoke {
            checks.push(timed_check("export.formats", || smoke_exports(&file_hash)));
        } else {
            checks.push(skipped(
                "export.formats",
                "Set includeExportSmoke=true to write and remove real temporary exports",
            ));
        }
    } else {
        for (id, detail) in [
            ("song.load", "No analyzed song is available"),
            ("editor.readiness", "No analyzed song is available"),
            ("editor.chart", "No analyzed song is available"),
            ("editor.audio", "No analyzed song is available"),
            ("export.formats", "No exportable song is available"),
        ] {
            checks.push(skipped(id, detail));
        }
    }

    let passed = checks
        .iter()
        .filter(|check| check.status == "passed")
        .count();
    let failed = checks
        .iter()
        .filter(|check| check.status == "failed")
        .count();
    let skipped = checks
        .iter()
        .filter(|check| check.status == "skipped")
        .count();
    DiagnosticReport {
        ok: failed == 0,
        generated_at_ms,
        capabilities: api_capabilities().len(),
        passed,
        failed,
        skipped,
        checks,
    }
}

#[tauri::command]
pub async fn run_feature_diagnostics(
    request: Option<DiagnosticRequest>,
) -> Result<DiagnosticReport, String> {
    tauri::async_runtime::spawn_blocking(move || {
        run_feature_diagnostics_blocking(request.unwrap_or_default())
    })
    .await
    .map_err(|error| format!("Diagnostic task failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn catalogue_has_unique_commands_and_every_access_class_is_known() {
        let capabilities = api_capabilities();
        let mut commands = capabilities
            .iter()
            .map(|capability| capability.command)
            .collect::<Vec<_>>();
        let original_len = commands.len();
        commands.sort_unstable();
        commands.dedup();
        assert_eq!(commands.len(), original_len);
        assert!(capabilities.iter().all(|capability| matches!(
            capability.access,
            "read" | "mutation" | "destructive" | "external" | "temporary"
        )));
    }

    #[test]
    fn catalogue_exactly_matches_registered_tauri_commands() {
        let source = include_str!("lib.rs");
        let handler_body = source
            .split_once("tauri::generate_handler![")
            .expect("generate_handler registry must exist")
            .1
            .split_once("])")
            .expect("generate_handler registry must close")
            .0;
        let registered = handler_body
            .lines()
            .map(|line| line.split("//").next().unwrap_or("").trim())
            .map(|line| line.trim_end_matches(',').trim())
            .filter(|line| !line.is_empty())
            .collect::<BTreeSet<_>>();
        let catalogued = api_capabilities()
            .into_iter()
            .map(|capability| capability.command)
            .collect::<BTreeSet<_>>();
        assert_eq!(catalogued, registered);
    }
}
