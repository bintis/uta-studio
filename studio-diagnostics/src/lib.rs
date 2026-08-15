//! Safe diagnostics shared by every Uta Studio desktop surface.
//!
//! The runner is deliberately read-only except for verified UTZ and UltraStar
//! exports in one uniquely named temporary directory. A drop guard always
//! removes that directory, including when an export or validation fails.

use std::{
    collections::BTreeSet,
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};

use app_core::{AnalysisQueue, AppConfig, CacheStats, SongsStore};
use serde::{Deserialize, Serialize};

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
        .stderr(Stdio::null())
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(format!(
            "ffmpeg could not decode {} ({status})",
            path.display()
        ));
    }
    let native = uta_studio_audio::probe_audio(path)?;
    Ok(format!("Decoded one second with ffmpeg; {native}"))
}

struct TemporaryExportDir(PathBuf);

impl TemporaryExportDir {
    fn new() -> Result<Self, String> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "uta-studio-diagnostics-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir(&path).map_err(|error| error.to_string())?;
        Ok(Self(path))
    }
}

impl Drop for TemporaryExportDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn smoke_exports(file_hash: &str) -> Result<String, String> {
    let root = TemporaryExportDir::new()?;
    let utz = root.0.join("diagnostic.utz");
    let ultrastar = root.0.join("diagnostic.txt");
    app_core::export_utz(file_hash, &utz).map_err(|error| error.to_string())?;
    app_core::export_ultrastar(file_hash, &ultrastar).map_err(|error| error.to_string())?;
    if app_core::export_utz(file_hash, &utz).is_ok()
        || app_core::export_ultrastar(file_hash, &ultrastar).is_ok()
    {
        return Err("An exporter silently overwrote an existing target".to_string());
    }
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

    let encoded_utz = std::fs::read(&utz).map_err(|error| error.to_string())?;
    let package = utz::UtzPackage::from_bytes(&encoded_utz)
        .map_err(|error| format!("UTZ validation failed: {error}"))?;
    let manifest = package.manifest();
    let manifest = manifest.as_v0_2().ok_or_else(|| {
        format!(
            "Unexpected UTZ format version: {}",
            manifest.format_version()
        )
    })?;
    if manifest.format != utz::FORMAT_ID {
        return Err(format!("Unexpected UTZ format: {}", manifest.format));
    }
    let vocal_chart = package
        .vocal_chart()
        .map_err(|error| format!("UTZ vocal chart is unreadable: {error}"))?
        .ok_or_else(|| "UTZ package has no vocal chart".to_string())?;
    vocal_chart
        .validate()
        .map_err(|error| format!("UTZ vocal chart is invalid: {error}"))?;
    let charted_notes: usize = vocal_chart
        .tracks
        .iter()
        .flat_map(|track| track.phrases.iter())
        .map(|phrase| phrase.notes.len())
        .sum();
    // Pitch evidence is optional, but when it ships it must parse and must be
    // declared as an optional feature rather than smuggled in.
    let pitch_frames = match package.pitch_evidence() {
        Ok(Some(evidence)) => {
            if !manifest
                .optional_features
                .iter()
                .any(|feature| feature == "pitch-evidence/1")
            {
                return Err("UTZ pitch evidence is undeclared".to_string());
            }
            evidence.frequency_hz.len()
        }
        Ok(None) => 0,
        Err(error) => return Err(format!("UTZ pitch evidence is unreadable: {error}")),
    };
    let audio_assets = [
        Some(&manifest.audio.instrumental),
        manifest.audio.guide_vocals.as_ref(),
        manifest.audio.original.as_ref(),
    ];
    let mut decoded_utz_audio = 0usize;
    for (index, asset) in audio_assets.into_iter().flatten().enumerate() {
        let bytes = package
            .file(&asset.path)
            .ok_or_else(|| format!("UTZ audio asset is missing: {}", asset.path))?;
        let extension = Path::new(&asset.path)
            .extension()
            .and_then(|extension| extension.to_str())
            .unwrap_or("audio");
        let path = root
            .0
            .join(format!("validate-utz-audio-{index}.{extension}"));
        std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
        decode_audio(&path)?;
        decoded_utz_audio += 1;
    }

    app_core::validate_ultrastar_chart(&ultrastar).map_err(|error| error.to_string())?;
    let ultrastar_text = std::fs::read_to_string(&ultrastar).map_err(|error| error.to_string())?;
    let mut decoded_ultrastar_audio = 0usize;
    for key in ["#MP3:", "#VOCALS:"] {
        let Some(name) = ultrastar_text
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .map(str::trim)
        else {
            continue;
        };
        let name = Path::new(name);
        if name.components().count() != 1 {
            return Err(format!("Unsafe UltraStar audio path: {}", name.display()));
        }
        decode_audio(&root.0.join(name))?;
        decoded_ultrastar_audio += 1;
    }
    if decoded_utz_audio == 0 || decoded_ultrastar_audio == 0 {
        return Err("An exported bundle has no decodable audio asset".to_string());
    }
    Ok(format!(
        "UTZ {utz_bytes} bytes ({decoded_utz_audio} audio asset(s), hashes valid, {} vocal track(s)/{charted_notes} note(s)/{pitch_frames} pitch frame(s) validated); UltraStar chart {ultrastar_bytes} bytes ({decoded_ultrastar_audio} audio asset(s), parsed)",
        vocal_chart.tracks.len()
    ))
}

pub fn run_feature_diagnostics(request: DiagnosticRequest) -> DiagnosticReport {
    let generated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let mut checks = Vec::new();

    checks.push(timed_check("library.connection", || {
        app_core::init_library()
            .map(|()| "Local library connection is ready".to_string())
            .map_err(|error| error.to_string())
    }));
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
    checks.push(timed_check("editor.actions", || {
        // Read-only enumeration of the editor command registry: the keyboard
        // map, the toolbars, and the undo labels all resolve through it, so a
        // duplicate id or a chord bound twice is a real defect.
        let actions = app_core::editor_actions();
        let mut commands = BTreeSet::new();
        let mut chords = BTreeSet::new();
        for action in actions {
            if !commands.insert(action.command) {
                return Err(format!("Duplicate editor command `{}`", action.command));
            }
            for chord in action.shortcuts {
                if !chords.insert((chord.key, chord.ctrl, chord.shift)) {
                    return Err(format!(
                        "`{}` reuses the {} shortcut",
                        action.command,
                        chord.describe()
                    ));
                }
            }
        }
        Ok(format!(
            "{} editor commands; {} key chords",
            actions.len(),
            chords.len()
        ))
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
            checks.push(skipped("export.formats", "Export smoke was not requested"));
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
        capabilities: app_core::api_capabilities().len(),
        passed,
        failed,
        skipped,
        checks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_without_library_is_safe_and_reports_every_area() {
        let report = run_feature_diagnostics(DiagnosticRequest::default());
        assert_eq!(report.capabilities, app_core::api_capabilities().len());
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == "models.runtime")
        );
        assert!(
            report
                .checks
                .iter()
                .any(|check| check.id == "export.formats")
        );
        let actions = report
            .checks
            .iter()
            .find(|check| check.id == "editor.actions")
            .expect("editor action registry check");
        assert_eq!(actions.status, "passed");
    }
}
