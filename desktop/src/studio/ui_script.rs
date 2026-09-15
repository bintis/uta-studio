//! Scripted execution of the registered UI interaction commands.
//!
//! `UTA_STUDIO_DEBUG_UI_SCRIPT` names an NDJSON file whose lines are
//! `{"command": "ui.settings.settings_tab", "arguments": {"tab": "models"}}`.
//! Each command id is the stable id `ui_api` registers for the corresponding
//! button, and every step is dispatched through the same handler a pointer
//! press uses, so a script exercises exactly the click path. Steps run one at
//! a time after the startup banner, spaced by a few frames so the rebuilt UI
//! settles between them. `UTA_STUDIO_DEBUG_UI_REPORT` (default: the script
//! path plus `.report.ndjson`) receives one record per step with the
//! dispatch outcome and the shell state after it; when the script ends the
//! debug screenshot hook captures the final frame if it is configured,
//! otherwise the application exits.
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::studio::ui_api::ui_interaction_capabilities;
use crate::studio::*;

const DEFAULT_PACE_FRAMES: u16 = 8;

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct UiScriptStep {
    pub(crate) command: String,
    #[serde(default)]
    pub(crate) arguments: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct UiScriptRecord<'a> {
    step: usize,
    command: &'a str,
    arguments: &'a serde_json::Value,
    dispatched: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    route: String,
    settings_tab: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    notice: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_settings_select: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_model_runtime_select: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    compute_backend: Option<String>,
    model_backend_overrides: std::collections::BTreeMap<String, String>,
    model_device_overrides: std::collections::BTreeMap<String, String>,
    model_settings: app_core::model_settings::ModelSettings,
    model_tuning: String,
}

#[derive(Resource)]
pub(crate) struct UiScriptState {
    steps: Vec<UiScriptStep>,
    report: Option<PathBuf>,
    cursor: usize,
    frames_since_step: u16,
    pace: u16,
    finished: bool,
    load_error: Option<String>,
}

impl Default for UiScriptState {
    fn default() -> Self {
        let Some(path) = std::env::var_os("UTA_STUDIO_DEBUG_UI_SCRIPT").map(PathBuf::from) else {
            return Self::inactive();
        };
        let report = std::env::var_os("UTA_STUDIO_DEBUG_UI_REPORT")
            .map(PathBuf::from)
            .or_else(|| {
                let mut report = path.clone().into_os_string();
                report.push(".report.ndjson");
                Some(PathBuf::from(report))
            });
        let pace = std::env::var("UTA_STUDIO_DEBUG_UI_SCRIPT_PACE")
            .ok()
            .and_then(|value| value.parse::<u16>().ok())
            .filter(|frames| *frames > 0)
            .unwrap_or(DEFAULT_PACE_FRAMES);
        match load_steps(&path) {
            Ok(steps) => Self {
                steps,
                report,
                cursor: 0,
                frames_since_step: 0,
                pace,
                finished: false,
                load_error: None,
            },
            Err(error) => Self {
                steps: Vec::new(),
                report,
                cursor: 0,
                frames_since_step: 0,
                pace,
                finished: false,
                load_error: Some(error),
            },
        }
    }
}

impl UiScriptState {
    fn inactive() -> Self {
        Self {
            steps: Vec::new(),
            report: None,
            cursor: 0,
            frames_since_step: 0,
            pace: DEFAULT_PACE_FRAMES,
            finished: true,
            load_error: None,
        }
    }

    /// True while a script is configured and has not finished.
    pub(crate) fn is_running(&self) -> bool {
        !self.finished
    }

    /// The next step to dispatch this frame, if one is due.
    fn due_step(&mut self, banner_done: bool) -> Option<(usize, UiScriptStep)> {
        if self.finished || !banner_done {
            return None;
        }
        self.frames_since_step = self.frames_since_step.saturating_add(1);
        if self.frames_since_step < self.pace {
            return None;
        }
        self.frames_since_step = 0;
        let step = self.steps.get(self.cursor)?.clone();
        Some((self.cursor, step))
    }

    fn advance(&mut self) {
        self.cursor += 1;
    }

    fn append(&self, value: &impl Serialize) {
        let Some(report) = &self.report else {
            return;
        };
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(report)
            .and_then(|mut file| {
                serde_json::to_writer(&mut file, value)?;
                file.write_all(b"\n")
            });
        if let Err(error) = result {
            bevy::log::warn!(
                target: "uta_studio::ui_script",
                error = %error,
                "could not append the UI script report"
            );
        }
    }
}

fn load_steps(path: &std::path::Path) -> Result<Vec<UiScriptStep>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read UI script {}: {error}", path.display()))?;
    text.lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| {
            serde_json::from_str::<UiScriptStep>(line)
                .map_err(|error| format!("UI script line {} is invalid: {error}", index + 1))
        })
        .collect()
}

impl UiScriptState {
    /// The next step to dispatch this frame, parsed, if one is due. A script
    /// that failed to load reports once and finishes without dispatching.
    pub(crate) fn take_due_step(
        &mut self,
        banner_done: bool,
    ) -> Option<(usize, UiScriptStep, Result<UiCommand, String>)> {
        if let Some(error) = self.load_error.take() {
            self.append(&serde_json::json!({"event": "script_error", "error": error}));
            self.finished = true;
            return None;
        }
        let (index, step) = self.due_step(banner_done)?;
        let parsed = parse_ui_command(&step.command, &step.arguments);
        if parsed.is_ok() {
            bevy::log::info!(
                target: "uta_studio::ui_script",
                step = index,
                command = %step.command,
                "dispatching scripted UI command"
            );
        }
        self.advance();
        Some((index, step, parsed))
    }

    /// Records the outcome of one step and the shell state right after it.
    pub(crate) fn record_step(
        &self,
        index: usize,
        step: &UiScriptStep,
        dispatched: bool,
        error: Option<String>,
        shell: &ShellState,
        dialogs: &DialogState,
    ) {
        let config = &shell.config;
        self.append(&UiScriptRecord {
            step: index,
            command: &step.command,
            arguments: &step.arguments,
            dispatched,
            error,
            route: format!("{:?}", shell.route),
            settings_tab: settings_tab_name(shell.settings_tab),
            notice: shell.notice.clone(),
            open_settings_select: dialogs.open_settings_select.map(settings_select_name),
            open_model_runtime_select: dialogs.open_model_runtime_select.clone(),
            compute_backend: config.compute_backend.clone(),
            model_backend_overrides: config.model_backend_overrides.clone(),
            model_device_overrides: config.model_device_overrides.clone(),
            model_settings: config.model_settings.clone(),
            model_tuning: shell.model_tuning.clone(),
        });
    }

    /// Marks the script finished once every step has been dispatched.
    /// Returns true on the frame the script completes.
    pub(crate) fn finish_if_done(&mut self, banner_done: bool) -> bool {
        if self.finished || !banner_done || self.cursor < self.steps.len() {
            return false;
        }
        self.append(&serde_json::json!({"event": "completed", "steps": self.steps.len()}));
        self.finished = true;
        true
    }
}

fn settings_tab_name(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::General => "general",
        SettingsTab::Storage => "storage",
        SettingsTab::Models => "models",
        SettingsTab::Analysis => "analysis",
    }
}

fn settings_select_name(kind: SettingsSelectKind) -> &'static str {
    match kind {
        SettingsSelectKind::UiLanguage => "ui_language",
        SettingsSelectKind::AnalysisTarget => "analysis_target",
        SettingsSelectKind::ComputeBackend => "compute_backend",
    }
}

fn argument<'a>(
    arguments: &'a serde_json::Value,
    name: &str,
) -> Result<&'a serde_json::Value, String> {
    arguments
        .get(name)
        .ok_or_else(|| format!("missing argument `{name}`"))
}

fn text(arguments: &serde_json::Value, name: &str) -> Result<String, String> {
    argument(arguments, name)?
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("argument `{name}` must be a string"))
}

fn optional_text(arguments: &serde_json::Value, name: &str) -> Result<Option<String>, String> {
    match arguments.get(name) {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err(format!("argument `{name}` must be a string or null")),
    }
}

fn integer<T: TryFrom<i64>>(arguments: &serde_json::Value, name: &str) -> Result<T, String> {
    argument(arguments, name)?
        .as_i64()
        .and_then(|value| T::try_from(value).ok())
        .ok_or_else(|| format!("argument `{name}` must be an integer in range"))
}

fn boolean(arguments: &serde_json::Value, name: &str) -> Result<bool, String> {
    argument(arguments, name)?
        .as_bool()
        .ok_or_else(|| format!("argument `{name}` must be a boolean"))
}

fn path(arguments: &serde_json::Value, name: &str) -> Result<PathBuf, String> {
    text(arguments, name).map(PathBuf::from)
}

fn typed<T: serde::de::DeserializeOwned>(
    arguments: &serde_json::Value,
    name: &str,
) -> Result<T, String> {
    serde_json::from_value(argument(arguments, name)?.clone())
        .map_err(|error| format!("argument `{name}` is invalid: {error}"))
}

fn settings_tab(arguments: &serde_json::Value) -> Result<SettingsTab, String> {
    Ok(match text(arguments, "tab")?.as_str() {
        "general" => SettingsTab::General,
        "storage" => SettingsTab::Storage,
        "models" => SettingsTab::Models,
        "analysis" => SettingsTab::Analysis,
        other => return Err(format!("unknown settings tab `{other}`")),
    })
}

fn settings_select_kind(arguments: &serde_json::Value) -> Result<SettingsSelectKind, String> {
    Ok(match text(arguments, "kind")?.as_str() {
        "ui_language" => SettingsSelectKind::UiLanguage,
        "analysis_target" => SettingsSelectKind::AnalysisTarget,
        "compute_backend" => SettingsSelectKind::ComputeBackend,
        other => return Err(format!("unknown settings select kind `{other}`")),
    })
}

fn quality_profile(
    arguments: &serde_json::Value,
) -> Result<app_core::AnalysisQualityProfile, String> {
    Ok(match text(arguments, "profile")?.as_str() {
        "fast" => app_core::AnalysisQualityProfile::Fast,
        "balanced" => app_core::AnalysisQualityProfile::Balanced,
        "maximum" => app_core::AnalysisQualityProfile::Maximum,
        other => return Err(format!("unknown analysis quality profile `{other}`")),
    })
}

fn setup_tier(arguments: &serde_json::Value) -> Result<app_core::SetupTier, String> {
    let tier = text(arguments, "tier")?;
    app_core::SetupTier::parse(&tier).ok_or_else(|| format!("unknown setup tier `{tier}`"))
}

fn download_target(
    arguments: &serde_json::Value,
) -> Result<Option<app_core::ModelDownloadTarget>, String> {
    match optional_text(arguments, "target")?.as_deref() {
        None => Ok(None),
        Some("roformer" | "ro_former") => Ok(Some(app_core::ModelDownloadTarget::RoFormer)),
        Some("pitch") => Ok(Some(app_core::ModelDownloadTarget::Pitch)),
        Some(other) => Err(format!("unknown model download target `{other}`")),
    }
}

fn library_view(arguments: &serde_json::Value) -> Result<LibraryView, String> {
    Ok(match text(arguments, "view")?.as_str() {
        "all" => LibraryView::All,
        "queue" => LibraryView::Queue,
        "completed" => LibraryView::Completed,
        "videos" => LibraryView::Videos,
        "artists" => LibraryView::Artists,
        "albums" => LibraryView::Albums,
        other => return Err(format!("unknown library view `{other}`")),
    })
}

fn library_facet(arguments: &serde_json::Value) -> Result<LibraryFacet, String> {
    let value = text(arguments, "value")?;
    let label = optional_text(arguments, "label")?.unwrap_or_else(|| value.clone());
    Ok(match text(arguments, "facet")?.as_str() {
        "artist" => LibraryFacet::Artist { value, label },
        "album" => LibraryFacet::Album { value, label },
        "playlist" => LibraryFacet::Playlist { value, label },
        other => return Err(format!("unknown library facet `{other}`")),
    })
}

fn library_sort(arguments: &serde_json::Value) -> Result<&'static str, String> {
    Ok(match text(arguments, "column")?.as_str() {
        "artist" => "artist",
        "album" => "album",
        "time" => "time",
        "status" => "status",
        other => return Err(format!("unknown library sort column `{other}`")),
    })
}

fn optional_workflow_card(
    arguments: &serde_json::Value,
) -> Result<app_core::OptionalWorkflowCard, String> {
    match text(arguments, "card")?.as_str() {
        "acoustic_dsp" => Ok(app_core::OptionalWorkflowCard::AcousticDsp),
        other => Err(format!("unknown optional workflow card `{other}`")),
    }
}

fn note_kind(arguments: &serde_json::Value) -> Result<app_core::NoteKind, String> {
    Ok(match text(arguments, "kind")?.as_str() {
        "normal" => app_core::NoteKind::Normal,
        "golden" => app_core::NoteKind::Golden,
        "freestyle" => app_core::NoteKind::Freestyle,
        "rap" => app_core::NoteKind::Rap,
        "golden_rap" => app_core::NoteKind::GoldenRap,
        other => return Err(format!("unknown note kind `{other}`")),
    })
}

fn boundary_target(arguments: &serde_json::Value) -> Result<TranscriptBoundaryTarget, String> {
    let segment: usize = integer(arguments, "segment")?;
    match arguments.get("word") {
        None | Some(serde_json::Value::Null) => Ok(TranscriptBoundaryTarget::Segment(segment)),
        Some(_) => Ok(TranscriptBoundaryTarget::Word {
            segment,
            word: integer(arguments, "word")?,
        }),
    }
}

fn boundary_edge(arguments: &serde_json::Value) -> Result<TranscriptBoundaryEdge, String> {
    Ok(match text(arguments, "edge")?.as_str() {
        "start" => TranscriptBoundaryEdge::Start,
        "end" => TranscriptBoundaryEdge::End,
        other => return Err(format!("unknown transcript boundary edge `{other}`")),
    })
}

fn lyrics_use_mode(arguments: &serde_json::Value) -> Result<LyricsCandidateUseMode, String> {
    Ok(match text(arguments, "mode")?.as_str() {
        "plain" => LyricsCandidateUseMode::Plain,
        "timed_lrc" => LyricsCandidateUseMode::TimedLrc,
        "translation" => LyricsCandidateUseMode::Translation,
        "romanization" => LyricsCandidateUseMode::Romanization,
        other => return Err(format!("unknown lyrics candidate use mode `{other}`")),
    })
}

fn editor_select_kind(arguments: &serde_json::Value) -> Result<EditorDockSelectKind, String> {
    Ok(match text(arguments, "kind")?.as_str() {
        "audio_source" => EditorDockSelectKind::AudioSource,
        "snap_grid" => EditorDockSelectKind::SnapGrid,
        "audition_mode" => EditorDockSelectKind::AuditionMode,
        other => return Err(format!("unknown editor select kind `{other}`")),
    })
}

fn waveform_source(arguments: &serde_json::Value) -> Result<WaveformSource, String> {
    Ok(match text(arguments, "source")?.as_str() {
        "instrumental" => WaveformSource::Instrumental,
        "vocals" => WaveformSource::Vocals,
        "original" => WaveformSource::Original,
        other => return Err(format!("unknown waveform source `{other}`")),
    })
}

fn waveform_style(arguments: &serde_json::Value) -> Result<WaveformStyle, String> {
    Ok(match text(arguments, "style")?.as_str() {
        "bars" => WaveformStyle::Bars,
        "filled" => WaveformStyle::Filled,
        "line" => WaveformStyle::Line,
        other => return Err(format!("unknown waveform style `{other}`")),
    })
}

fn audition_slot(arguments: &serde_json::Value) -> Result<ArtifactAuditionSlot, String> {
    Ok(match text(arguments, "slot")?.as_str() {
        "a" => ArtifactAuditionSlot::A,
        "b" => ArtifactAuditionSlot::B,
        other => return Err(format!("unknown audition slot `{other}`")),
    })
}

fn problems_filter(arguments: &serde_json::Value) -> Result<ProblemsFilter, String> {
    Ok(match text(arguments, "filter")?.as_str() {
        "all" => ProblemsFilter::All,
        "errors" => ProblemsFilter::Errors,
        "warnings" => ProblemsFilter::Warnings,
        other => return Err(format!("unknown problems filter `{other}`")),
    })
}

/// Parses a registered command id plus named arguments into the exact
/// `UiCommand` its button dispatches. Unknown ids and malformed arguments are
/// reported, never guessed.
pub(crate) fn parse_ui_command(
    command: &str,
    arguments: &serde_json::Value,
) -> Result<UiCommand, String> {
    let (namespace, name) = command
        .strip_prefix("ui.")
        .and_then(|rest| rest.split_once('.'))
        .ok_or_else(|| format!("`{command}` is not a ui.* command id"))?;
    let parsed = match namespace {
        "app" => parse_app(name).map(UiCommand::App),
        "library" => parse_library(name, arguments).map(UiCommand::Library),
        "settings" => parse_settings(name, arguments).map(UiCommand::Settings),
        "analysis" => parse_analysis(name, arguments).map(UiCommand::Analysis),
        "editor" => parse_editor(name, arguments).map(UiCommand::Editor),
        other => Err(format!("unknown command namespace `{other}`")),
    }?;
    let registered = parsed.api_command();
    if registered != command {
        return Err(format!(
            "`{command}` parsed to `{registered}`; the registry and the parser disagree"
        ));
    }
    Ok(parsed)
}

fn parse_app(name: &str) -> Result<AppCommand, String> {
    Ok(match name {
        "back" => AppCommand::Back,
        "home" => AppCommand::Home,
        "toggle_global_search" => AppCommand::ToggleGlobalSearch,
        "folders" => AppCommand::Folders,
        "settings" => AppCommand::Settings,
        "documentation" => AppCommand::Documentation,
        "open_documentation" => AppCommand::OpenDocumentation(None),
        "documentation_back" => AppCommand::DocumentationBack,
        "documentation_forward" => AppCommand::DocumentationForward,
        "toggle_activity" => AppCommand::ToggleActivity,
        "close_activity" => AppCommand::CloseActivity,
        "open_about" => AppCommand::OpenAbout,
        "close_about" => AppCommand::CloseAbout,
        "toggle_fullscreen" => AppCommand::ToggleFullscreen,
        "open_log" => AppCommand::OpenLog,
        "toggle_debug_logging" => AppCommand::ToggleDebugLogging,
        "run_diagnostics" => AppCommand::RunDiagnostics,
        "cancel_leave" => AppCommand::CancelLeave,
        "confirm_leave" => AppCommand::ConfirmLeave,
        other => return Err(format!("unknown app command `{other}`")),
    })
}

fn parse_library(name: &str, arguments: &serde_json::Value) -> Result<LibraryCommand, String> {
    Ok(match name {
        "set_library_view" => LibraryCommand::SetLibraryView(library_view(arguments)?),
        "set_library_facet" => LibraryCommand::SetLibraryFacet(library_facet(arguments)?),
        "set_library_sort" => LibraryCommand::SetLibrarySort(library_sort(arguments)?),
        "load_more_songs" => LibraryCommand::LoadMoreSongs,
        "apply_library_search" => LibraryCommand::ApplyLibrarySearch,
        "clear_library_search" => LibraryCommand::ClearLibrarySearch,
        "toggle_library_layout" => LibraryCommand::ToggleLibraryLayout,
        "toggle_export_all_menu" => LibraryCommand::ToggleExportAllMenu,
        "export_all_utz" => LibraryCommand::ExportAllUtz,
        "export_all_ultra_star" => LibraryCommand::ExportAllUltraStar,
        "analyze_all" => LibraryCommand::AnalyzeAll,
        "rescan_library" => LibraryCommand::RescanLibrary,
        "choose_folder" => LibraryCommand::ChooseFolder,
        "choose_export_folder" => LibraryCommand::ChooseExportFolder,
        "clear_export_folder" => LibraryCommand::ClearExportFolder,
        "select_folder_root" => LibraryCommand::SelectFolderRoot(path(arguments, "path")?),
        "folder_up" => LibraryCommand::FolderUp,
        "open_folder_entry" => LibraryCommand::OpenFolderEntry(path(arguments, "path")?),
        "reveal_folder_entry" => LibraryCommand::RevealFolderEntry(path(arguments, "path")?),
        "dismiss_folder_context" => LibraryCommand::DismissFolderContext,
        "request_remove_folder" => LibraryCommand::RequestRemoveFolder(path(arguments, "path")?),
        "cancel_remove_folder" => LibraryCommand::CancelRemoveFolder,
        "confirm_remove_folder" => LibraryCommand::ConfirmRemoveFolder,
        "open_song" => LibraryCommand::OpenSong(text(arguments, "file_hash")?),
        "analyze_song" => LibraryCommand::AnalyzeSong(text(arguments, "file_hash")?),
        "choose_editor_file" => LibraryCommand::ChooseEditorFile,
        "open_editor" => LibraryCommand::OpenEditor(text(arguments, "file_hash")?),
        "export_utz" => LibraryCommand::ExportUtz(text(arguments, "file_hash")?),
        "export_ultra_star" => LibraryCommand::ExportUltraStar(text(arguments, "file_hash")?),
        "export_to_osu" => LibraryCommand::ExportToOsu(text(arguments, "file_hash")?),
        "open_source" => LibraryCommand::OpenSource(path(arguments, "path")?),
        "reveal_source" => LibraryCommand::RevealSource(path(arguments, "path")?),
        "dismiss_song_context" => LibraryCommand::DismissSongContext,
        "play_library_song" => LibraryCommand::PlayLibrarySong(text(arguments, "file_hash")?),
        "toggle_library_playback" => LibraryCommand::ToggleLibraryPlayback,
        "seek_library_relative" => {
            LibraryCommand::SeekLibraryRelative(integer(arguments, "delta")?)
        }
        "previous_library_song" => LibraryCommand::PreviousLibrarySong,
        "next_library_song" => LibraryCommand::NextLibrarySong,
        "toggle_library_shuffle" => LibraryCommand::ToggleLibraryShuffle,
        "cycle_library_repeat" => LibraryCommand::CycleLibraryRepeat,
        "adjust_library_volume" => {
            LibraryCommand::AdjustLibraryVolume(integer(arguments, "delta")?)
        }
        "toggle_library_mute" => LibraryCommand::ToggleLibraryMute,
        "toggle_library_audio_source_menu" => LibraryCommand::ToggleLibraryAudioSourceMenu,
        "select_library_audio_source" => {
            LibraryCommand::SelectLibraryAudioSource(text(arguments, "source")?)
        }
        "toggle_library_queue" => LibraryCommand::ToggleLibraryQueue,
        other => return Err(format!("unknown library command `{other}`")),
    })
}

fn parse_settings(name: &str, arguments: &serde_json::Value) -> Result<SettingsCommand, String> {
    Ok(match name {
        "settings_tab" => SettingsCommand::SettingsTab(settings_tab(arguments)?),
        "refresh_runtime_status" => SettingsCommand::RefreshRuntimeStatus,
        "open_model_downloads" => SettingsCommand::OpenModelDownloads,
        "close_model_downloads" => SettingsCommand::CloseModelDownloads,
        "open_settings_select" => {
            SettingsCommand::OpenSettingsSelect(settings_select_kind(arguments)?)
        }
        "select_settings_value" => SettingsCommand::SelectSettingsValue(
            settings_select_kind(arguments)?,
            text(arguments, "value")?,
        ),
        "toggle_model_runtime_select" => {
            SettingsCommand::ToggleModelRuntimeSelect(text(arguments, "model_id")?)
        }
        "set_model_backend" => SettingsCommand::SetModelBackend(
            text(arguments, "model_id")?,
            optional_text(arguments, "backend")?,
        ),
        "set_model_device" => SettingsCommand::SetModelDevice(
            text(arguments, "model_id")?,
            optional_text(arguments, "device")?,
        ),
        "select_model_tuning" => SettingsCommand::SelectModelTuning(text(arguments, "model_id")?),
        "set_model_parameter" => SettingsCommand::SetModelParameter(
            text(arguments, "model_id")?,
            text(arguments, "parameter")?,
            arguments
                .get("value")
                .map(|value| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .ok_or("missing value")?,
        ),
        "adjust_model_parameter" => SettingsCommand::AdjustModelParameter(
            text(arguments, "model_id")?,
            text(arguments, "parameter")?,
            integer(arguments, "delta")?,
        ),
        "apply_model_parameter" => SettingsCommand::ApplyModelParameter(
            text(arguments, "model_id")?,
            text(arguments, "parameter")?,
        ),
        "reset_model_parameters" => {
            SettingsCommand::ResetModelParameters(text(arguments, "model_id")?)
        }
        "toggle_turbo_acceleration" => SettingsCommand::ToggleTurboAcceleration,
        "set_analysis_quality" => SettingsCommand::SetAnalysisQuality(quality_profile(arguments)?),
        "toggle_preserve_continuous_pitch" => SettingsCommand::TogglePreserveContinuousPitch,
        "toggle_analysis_quantization" => SettingsCommand::ToggleAnalysisQuantization,
        "request_setup" => SettingsCommand::RequestSetup(download_target(arguments)?),
        "install_audio_model" => SettingsCommand::InstallAudioModel(text(arguments, "model_id")?),
        "remove_audio_model" => SettingsCommand::RemoveAudioModel(text(arguments, "model_id")?),
        "cancel_setup" => SettingsCommand::CancelSetup,
        "confirm_setup" => SettingsCommand::ConfirmSetup,
        "open_setup_guide" => SettingsCommand::OpenSetupGuide,
        "close_setup_guide" => SettingsCommand::CloseSetupGuide,
        "skip_setup_guide" => SettingsCommand::SkipSetupGuide,
        "open_setup_tiers" => SettingsCommand::OpenSetupTiers,
        "select_setup_tier" => SettingsCommand::SelectSetupTier(setup_tier(arguments)?),
        "close_setup_tiers" => SettingsCommand::CloseSetupTiers,
        "confirm_setup_tier" => SettingsCommand::ConfirmSetupTier,
        "toggle_theme" => SettingsCommand::ToggleTheme,
        "toggle_window_transparency" => SettingsCommand::ToggleWindowTransparency,
        "adjust_window_opacity" => {
            SettingsCommand::AdjustWindowOpacity(integer(arguments, "delta")?)
        }
        "adjust_ui_font_scale" => SettingsCommand::AdjustUiFontScale(integer(arguments, "delta")?),
        "toggle_auto_analyze" => SettingsCommand::ToggleAutoAnalyze,
        "restore_analysis_defaults" => SettingsCommand::RestoreAnalysisDefaults,
        "request_clear_cache" => SettingsCommand::RequestClearCache(
            match optional_text(arguments, "scope")?.as_deref() {
                Some("logs") => CacheClearScope::Logs,
                Some("generated") | None => CacheClearScope::Generated,
                Some(scope) => return Err(format!("Unknown cache cleanup scope: {scope}")),
            },
        ),
        "cancel_clear_cache" => SettingsCommand::CancelClearCache,
        "confirm_clear_cache" => SettingsCommand::ConfirmClearCache,
        "select_fusion_provider" => {
            SettingsCommand::SelectFusionProvider(text(arguments, "provider")?)
        }
        "clear_fusion_provider" => SettingsCommand::ClearFusionProvider,
        "choose_fusion_agent_adapter" => SettingsCommand::ChooseFusionAgentAdapter,
        "clear_fusion_agent_adapter" => SettingsCommand::ClearFusionAgentAdapter,
        other => return Err(format!("unknown settings command `{other}`")),
    })
}

fn parse_analysis(name: &str, arguments: &serde_json::Value) -> Result<AnalysisCommand, String> {
    Ok(match name {
        "open_analysis_queue" => AnalysisCommand::OpenAnalysisQueue,
        "move_analysis_queue_item" => AnalysisCommand::MoveAnalysisQueueItem(
            text(arguments, "task_id")?,
            boolean(arguments, "up")?,
        ),
        "delete_analysis_queue_item" => {
            AnalysisCommand::DeleteAnalysisQueueItem(text(arguments, "task_id")?)
        }
        "start_analysis" => AnalysisCommand::StartAnalysis(text(arguments, "file_hash")?),
        "start_queued_analysis" => {
            AnalysisCommand::StartQueuedAnalysis(text(arguments, "file_hash")?)
        }
        "merge_selected_candidate_phrase" => AnalysisCommand::MergeSelectedCandidatePhrase(
            typed(arguments, "from")?,
            typed(arguments, "to")?,
        ),
        "merge_selected_candidate_range" => AnalysisCommand::MergeSelectedCandidateRange(
            typed(arguments, "from")?,
            typed(arguments, "to")?,
        ),
        "keep_authored_chart" => AnalysisCommand::KeepAuthoredChart,
        "select_analysis_history" => {
            AnalysisCommand::SelectAnalysisHistory(match arguments.get("run_id") {
                None | Some(serde_json::Value::Null) => None,
                Some(_) => Some(integer(arguments, "run_id")?),
            })
        }
        "open_song_analysis" => AnalysisCommand::OpenSongAnalysis(text(arguments, "file_hash")?),
        "open_song_model_selection" => {
            AnalysisCommand::OpenSongModelSelection(text(arguments, "file_hash")?)
        }
        "open_processing_studio" => {
            AnalysisCommand::OpenProcessingStudio(text(arguments, "file_hash")?)
        }
        "analyze_now" => AnalysisCommand::AnalyzeNow(text(arguments, "file_hash")?),
        "open_empty_processing_studio" => AnalysisCommand::OpenEmptyProcessingStudio,
        "select_workflow_node" => AnalysisCommand::SelectWorkflowNode(text(arguments, "node_id")?),
        "move_workflow_node" => AnalysisCommand::MoveWorkflowNode(
            text(arguments, "node_id")?,
            boolean(arguments, "up")?,
        ),
        "remove_workflow_node" => AnalysisCommand::RemoveWorkflowNode(text(arguments, "node_id")?),
        "set_workflow_node_model" => AnalysisCommand::SetWorkflowNodeModel(
            text(arguments, "node_id")?,
            text(arguments, "model_id")?,
        ),
        "set_workflow_separation_strategy" => AnalysisCommand::SetWorkflowSeparationStrategy(
            text(arguments, "node_id")?,
            typed(arguments, "strategy")?,
        ),
        "add_workflow_processor" => AnalysisCommand::AddWorkflowProcessor(
            text(arguments, "node_id")?,
            text(arguments, "processor")?,
            text(arguments, "model_id")?,
            optional_text(arguments, "after")?,
        ),
        "add_optional_workflow_card" => AnalysisCommand::AddOptionalWorkflowCard(
            text(arguments, "node_id")?,
            text(arguments, "model_id")?,
            optional_workflow_card(arguments)?,
        ),
        "set_workflow_parameter" => AnalysisCommand::SetWorkflowParameter(
            text(arguments, "node_id")?,
            text(arguments, "parameter")?,
            argument(arguments, "value")?.clone(),
        ),
        "set_workflow_policy" => AnalysisCommand::SetWorkflowPolicy(
            text(arguments, "node_id")?,
            typed(arguments, "policy")?,
        ),
        "set_workflow_preprocessing_enabled" => {
            AnalysisCommand::SetWorkflowPreprocessingEnabled(boolean(arguments, "enabled")?)
        }
        "set_workflow_skip_if_unchanged" => AnalysisCommand::SetWorkflowSkipIfUnchanged(
            text(arguments, "node_id")?,
            boolean(arguments, "enabled")?,
        ),
        "adjust_workflow_priority" => AnalysisCommand::AdjustWorkflowPriority(
            text(arguments, "node_id")?,
            integer(arguments, "delta")?,
        ),
        "rebind_workflow_analyzer" => AnalysisCommand::RebindWorkflowAnalyzer(
            text(arguments, "node_id")?,
            text(arguments, "analyzer")?,
            text(arguments, "model_id")?,
        ),
        "save_workflow" => AnalysisCommand::SaveWorkflow,
        "run_workflow" => AnalysisCommand::RunWorkflow,
        "open_analysis_inspect" => AnalysisCommand::OpenAnalysisInspect(
            text(arguments, "file_hash")?,
            text(arguments, "node_id")?,
        ),
        "adjust_analysis_graph_zoom" => {
            AnalysisCommand::AdjustAnalysisGraphZoom(integer(arguments, "delta")?)
        }
        "fit_analysis_graph" => AnalysisCommand::FitAnalysisGraph,
        "toggle_analysis_graph_follow" => AnalysisCommand::ToggleAnalysisGraphFollow,
        "close_analysis_model_panel" => AnalysisCommand::CloseAnalysisModelPanel,
        "dismiss_analysis_node_context" => AnalysisCommand::DismissAnalysisNodeContext,
        "request_clear_analysis_history" => AnalysisCommand::RequestClearAnalysisHistory,
        "cancel_clear_analysis_history" => AnalysisCommand::CancelClearAnalysisHistory,
        "confirm_clear_analysis_history" => AnalysisCommand::ConfirmClearAnalysisHistory,
        "compare_node_attempt_with_previous" => AnalysisCommand::CompareNodeAttemptWithPrevious(
            text(arguments, "file_hash")?,
            text(arguments, "node_id")?,
            integer(arguments, "attempt")?,
        ),
        "close_plan_preview" => AnalysisCommand::ClosePlanPreview,
        "queue_exact_preview" => AnalysisCommand::QueueExactPreview,
        "toggle_plan_preview_output" => {
            AnalysisCommand::TogglePlanPreviewOutput(typed(arguments, "output")?)
        }
        "reset_plan_preview_outputs" => AnalysisCommand::ResetPlanPreviewOutputs,
        "set_plan_preview_quality" => {
            AnalysisCommand::SetPlanPreviewQuality(quality_profile(arguments)?)
        }
        "reset_plan_preview_quality" => AnalysisCommand::ResetPlanPreviewQuality,
        "open_analysis_log_viewer" => AnalysisCommand::OpenAnalysisLogViewer(
            text(arguments, "file_hash")?,
            text(arguments, "node_id")?,
        ),
        "close_analysis_log_viewer" => AnalysisCommand::CloseAnalysisLogViewer,
        "request_delete_song_cache" => {
            AnalysisCommand::RequestDeleteSongCache(text(arguments, "file_hash")?)
        }
        "cancel_analysis_run" => AnalysisCommand::CancelAnalysisRun(text(arguments, "file_hash")?),
        "force_stop_all_analysis" => AnalysisCommand::ForceStopAllAnalysis,
        "cancel_delete_song_cache" => AnalysisCommand::CancelDeleteSongCache,
        "confirm_delete_song_cache" => AnalysisCommand::ConfirmDeleteSongCache,
        "request_delete_authored_chart" => {
            AnalysisCommand::RequestDeleteAuthoredChart(text(arguments, "file_hash")?)
        }
        "cancel_delete_authored_chart" => AnalysisCommand::CancelDeleteAuthoredChart,
        "confirm_delete_authored_chart" => AnalysisCommand::ConfirmDeleteAuthoredChart,
        "request_replace_authored_chart" => {
            AnalysisCommand::RequestReplaceAuthoredChart(text(arguments, "file_hash")?)
        }
        "cancel_replace_authored_chart" => AnalysisCommand::CancelReplaceAuthoredChart,
        "confirm_replace_authored_chart" => AnalysisCommand::ConfirmReplaceAuthoredChart,
        "request_remove_song" => AnalysisCommand::RequestRemoveSong(text(arguments, "file_hash")?),
        "cancel_remove_song" => AnalysisCommand::CancelRemoveSong,
        "confirm_remove_song" => AnalysisCommand::ConfirmRemoveSong,
        other => return Err(format!("unknown analysis command `{other}`")),
    })
}

fn parse_editor(name: &str, arguments: &serde_json::Value) -> Result<EditorCommand, String> {
    if let Some(action) = name.strip_prefix("action.") {
        return EditorAction::from_command(action)
            .map(EditorCommand::Editor)
            .ok_or_else(|| format!("unknown editor action `{action}`"));
    }
    Ok(match name {
        "open_lyrics_editor" => EditorCommand::OpenLyricsEditor(text(arguments, "file_hash")?),
        "close_lyrics_editor" => EditorCommand::CloseLyricsEditor,
        "toggle_lyrics_input_mode" => EditorCommand::ToggleLyricsInputMode,
        "search_all_lyrics_sources" => EditorCommand::SearchAllLyricsSources,
        "extract_lyrics" => EditorCommand::ExtractLyrics,
        "previous_lyrics_candidate_page" => EditorCommand::PreviousLyricsCandidatePage,
        "next_lyrics_candidate_page" => EditorCommand::NextLyricsCandidatePage,
        "load_lyrics_candidate" => EditorCommand::LoadLyricsCandidate(integer(arguments, "index")?),
        "use_lyrics_candidate" => EditorCommand::UseLyricsCandidate(
            integer(arguments, "index")?,
            lyrics_use_mode(arguments)?,
        ),
        "normalize_lyrics_editor" => EditorCommand::NormalizeLyricsEditor,
        "strip_lyrics_timing" => EditorCommand::StripLyricsTiming,
        "clear_lyrics_editor" => EditorCommand::ClearLyricsEditor,
        "save_lyrics_editor" => EditorCommand::SaveLyricsEditor,
        "save_lyrics_editor_and_align" => EditorCommand::SaveLyricsEditorAndAlign,
        "adjust_transcript_boundary" => EditorCommand::AdjustTranscriptBoundary(
            boundary_target(arguments)?,
            boundary_edge(arguments)?,
            integer(arguments, "delta")?,
        ),
        "preview_transcript_at" => EditorCommand::PreviewTranscriptAt(
            text(arguments, "file_hash")?,
            integer(arguments, "at")?,
        ),
        "open_language_editor" => EditorCommand::OpenLanguageEditor(text(arguments, "file_hash")?),
        "close_language_editor" => EditorCommand::CloseLanguageEditor,
        "toggle_language_reprocess" => EditorCommand::ToggleLanguageReprocess,
        "toggle_language_picker" => EditorCommand::ToggleLanguagePicker,
        "select_analysis_language" => {
            EditorCommand::SelectAnalysisLanguage(text(arguments, "language")?)
        }
        "save_language_editor" => EditorCommand::SaveLanguageEditor,
        "open_song_settings" => EditorCommand::OpenSongSettings(text(arguments, "file_hash")?),
        "close_song_settings" => EditorCommand::CloseSongSettings,
        "choose_background_video" => EditorCommand::ChooseBackgroundVideo,
        "clear_background_video" => EditorCommand::ClearBackgroundVideo,
        "save_song_settings" => EditorCommand::SaveSongSettings,
        "shift_song_key" => {
            EditorCommand::ShiftSongKey(text(arguments, "file_hash")?, integer(arguments, "delta")?)
        }
        "shift_song_tempo" => EditorCommand::ShiftSongTempo(
            text(arguments, "file_hash")?,
            integer(arguments, "delta")?,
        ),
        "focus_chart_problem" => EditorCommand::FocusChartProblem(
            integer(arguments, "index")?,
            integer(arguments, "time")?,
        ),
        "open_editor_select" => EditorCommand::OpenEditorSelect(editor_select_kind(arguments)?),
        "select_editor_value" => EditorCommand::SelectEditorValue(
            editor_select_kind(arguments)?,
            text(arguments, "value")?,
        ),
        "select_editor_word" => EditorCommand::SelectEditorWord(
            integer(arguments, "track")?,
            integer(arguments, "word")?,
            integer(arguments, "time")?,
        ),
        "select_editor_track" => EditorCommand::SelectEditorTrack(integer(arguments, "track")?),
        "move_selection_to_track" => {
            EditorCommand::MoveSelectionToTrack(integer(arguments, "track")?)
        }
        "set_note_kind" => EditorCommand::SetNoteKind(note_kind(arguments)?),
        "toggle_editor_file_menu" => EditorCommand::ToggleEditorFileMenu,
        "dismiss_editor_file_menu" => EditorCommand::DismissEditorFileMenu,
        "save_editor_as_utz" => EditorCommand::SaveEditorAsUtz,
        "save_editor_as_ultra_star" => EditorCommand::SaveEditorAsUltraStar,
        "toggle_editor_layout_menu" => EditorCommand::ToggleEditorLayoutMenu,
        "dismiss_editor_layout_menu" => EditorCommand::DismissEditorLayoutMenu,
        "dismiss_lyric_context" => EditorCommand::DismissLyricContext,
        "dismiss_note_context" => EditorCommand::DismissNoteContext,
        "select_waveform_source" => {
            EditorCommand::SelectWaveformSource(waveform_source(arguments)?)
        }
        "select_artifact_audition" => EditorCommand::SelectArtifactAudition(
            audition_slot(arguments)?,
            typed(arguments, "artifact")?,
        ),
        "activate_artifact_audition" => {
            EditorCommand::ActivateArtifactAudition(audition_slot(arguments)?)
        }
        "select_artifact_waveform" => {
            EditorCommand::SelectArtifactWaveform(typed(arguments, "artifact")?)
        }
        "select_waveform_style" => EditorCommand::SelectWaveformStyle(waveform_style(arguments)?),
        "dismiss_waveform_context" => EditorCommand::DismissWaveformContext,
        "toggle_evidence" => EditorCommand::ToggleEvidence(typed(arguments, "evidence")?),
        "review_previous" => EditorCommand::ReviewPrevious,
        "review_next" => EditorCommand::ReviewNext,
        "mark_review_region" => EditorCommand::MarkReviewRegion,
        "accept_suggestion" => EditorCommand::AcceptSuggestion(text(arguments, "suggestion_id")?),
        "ignore_suggestion" => EditorCommand::IgnoreSuggestion(text(arguments, "suggestion_id")?),
        "set_problems_filter" => EditorCommand::SetProblemsFilter(problems_filter(arguments)?),
        "apply_all_lyrics_edit" => EditorCommand::ApplyAllLyricsEdit,
        "extend_lyric_over_note" => EditorCommand::ExtendLyricOverNote(
            typed(arguments, "selection")?,
            integer(arguments, "note")?,
        ),
        "dismiss_problems_panel" => EditorCommand::DismissProblemsPanel,
        "dismiss_shortcuts_panel" => EditorCommand::DismissShortcutsPanel,
        other => return Err(format!("unknown editor command `{other}`")),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(command: &str, arguments: serde_json::Value) -> UiCommand {
        parse_ui_command(command, &arguments).unwrap_or_else(|error| panic!("{command}: {error}"))
    }

    #[test]
    fn setup_guide_commands_parse_with_download_confirmation_classified_external() {
        assert_eq!(
            parse(
                "ui.settings.select_setup_tier",
                serde_json::json!({"tier": "maximum"})
            ),
            UiCommand::Settings(SettingsCommand::SelectSetupTier(
                app_core::SetupTier::Maximum
            ))
        );
        assert!(
            parse_ui_command(
                "ui.settings.select_setup_tier",
                &serde_json::json!({"tier": "everything"})
            )
            .is_err()
        );
        for (command, access) in [
            ("ui.settings.open_setup_guide", "read"),
            ("ui.settings.close_setup_guide", "read"),
            ("ui.settings.skip_setup_guide", "mutation"),
            ("ui.settings.open_setup_tiers", "read"),
            ("ui.settings.close_setup_tiers", "read"),
            ("ui.settings.confirm_setup_tier", "external"),
        ] {
            assert_eq!(
                parse(command, serde_json::json!({})).api_request().access,
                access
            );
        }
    }

    #[test]
    fn log_cleanup_requires_its_explicit_scope_and_confirmation_command() {
        assert_eq!(
            parse(
                "ui.settings.request_clear_cache",
                serde_json::json!({"scope": "logs"})
            ),
            UiCommand::Settings(SettingsCommand::RequestClearCache(CacheClearScope::Logs))
        );
        assert_eq!(
            parse("ui.settings.cancel_clear_cache", serde_json::json!({})),
            UiCommand::Settings(SettingsCommand::CancelClearCache)
        );
        assert!(
            parse_ui_command(
                "ui.settings.request_clear_cache",
                &serde_json::json!({"scope": "typo"})
            )
            .is_err()
        );
        assert!(
            parse_ui_command(
                "ui.settings.request_clear_cache",
                &serde_json::json!({"scope": ["logs"]})
            )
            .is_err()
        );
        let request = UiAction::from(SettingsCommand::ConfirmClearCache).api_request();
        assert_eq!(request.access, "destructive");
    }

    #[test]
    fn settings_backend_selection_parses_to_the_button_commands() {
        assert_eq!(
            parse("ui.app.settings", serde_json::json!({})),
            UiCommand::App(AppCommand::Settings)
        );
        assert_eq!(
            parse(
                "ui.settings.settings_tab",
                serde_json::json!({"tab": "models"})
            ),
            UiCommand::Settings(SettingsCommand::SettingsTab(SettingsTab::Models))
        );
        assert_eq!(
            parse(
                "ui.settings.open_settings_select",
                serde_json::json!({"kind": "compute_backend"})
            ),
            UiCommand::Settings(SettingsCommand::OpenSettingsSelect(
                SettingsSelectKind::ComputeBackend
            ))
        );
        assert_eq!(
            parse(
                "ui.settings.select_settings_value",
                serde_json::json!({"kind": "compute_backend", "value": "libtorch_xpu"})
            ),
            UiCommand::Settings(SettingsCommand::SelectSettingsValue(
                SettingsSelectKind::ComputeBackend,
                "libtorch_xpu".to_string()
            ))
        );
        assert_eq!(
            parse(
                "ui.settings.set_model_backend",
                serde_json::json!({"model_id": "rmvpe", "backend": "libtorch_xpu"})
            ),
            UiCommand::Settings(SettingsCommand::SetModelBackend(
                "rmvpe".to_string(),
                Some("libtorch_xpu".to_string())
            ))
        );
        assert_eq!(
            parse(
                "ui.settings.set_model_backend",
                serde_json::json!({"model_id": "rmvpe", "backend": null})
            ),
            UiCommand::Settings(SettingsCommand::SetModelBackend("rmvpe".to_string(), None))
        );
    }

    #[test]
    fn every_registered_command_id_has_a_parser_or_a_named_argument_error() {
        let arguments = serde_json::json!({});
        for capability in ui_interaction_capabilities() {
            let command = capability.command;
            if command.starts_with("ui.pointer.") {
                continue;
            }
            match parse_ui_command(&command, &arguments) {
                Ok(parsed) => assert_eq!(parsed.api_command(), command),
                Err(error) => assert!(
                    error.starts_with("missing argument") || error.starts_with("argument `"),
                    "{command}: {error}"
                ),
            }
        }
    }

    #[test]
    fn model_quality_commands_keep_model_ownership_and_numeric_values() {
        assert_eq!(
            parse(
                "ui.settings.set_model_parameter",
                serde_json::json!({"model_id":"bs_roformer_leap_xe90_vocals", "parameter":"overlap", "value":8})
            ),
            UiCommand::Settings(SettingsCommand::SetModelParameter(
                "bs_roformer_leap_xe90_vocals".into(),
                "overlap".into(),
                "8".into()
            ))
        );
        assert_eq!(
            parse(
                "ui.settings.adjust_model_parameter",
                serde_json::json!({"model_id":"rmvpe", "parameter":"voiced_threshold", "delta":-1})
            ),
            UiCommand::Settings(SettingsCommand::AdjustModelParameter(
                "rmvpe".into(),
                "voiced_threshold".into(),
                -1
            ))
        );
    }

    #[test]
    fn unknown_ids_and_malformed_arguments_are_reported() {
        assert!(parse_ui_command("ui.settings.nonexistent", &serde_json::json!({})).is_err());
        assert!(parse_ui_command("settings_tab", &serde_json::json!({})).is_err());
        assert!(
            parse_ui_command(
                "ui.settings.settings_tab",
                &serde_json::json!({"tab": "nowhere"})
            )
            .is_err()
        );
        assert!(
            parse_ui_command(
                "ui.library.seek_library_relative",
                &serde_json::json!({"delta": 1000})
            )
            .is_err()
        );
    }

    #[test]
    fn scripts_load_as_ndjson_and_reject_invalid_lines() {
        let directory = std::env::temp_dir().join(format!(
            "uta-ui-script-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let script = directory.join("script.ndjson");
        std::fs::write(
            &script,
            "{\"command\":\"ui.app.settings\"}\n\n{\"command\":\"ui.settings.settings_tab\",\"arguments\":{\"tab\":\"models\"}}\n",
        )
        .unwrap();
        let steps = load_steps(&script).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[1].arguments["tab"], "models");
        std::fs::write(&script, "not json\n").unwrap();
        assert!(load_steps(&script).unwrap_err().contains("line 1"));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
