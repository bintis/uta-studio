use crate::studio::*;

pub(crate) struct SettingsActionContext<'a> {
    pub(crate) window: &'a mut Window,
    pub(crate) state: StudioStateMut<'a>,
    pub(crate) setup: &'a mut NativeSetup,
    pub(crate) diagnostics: &'a mut NativeDiagnostics,
    pub(crate) theme: &'a mut StudioTheme,
    pub(crate) clear_color: &'a mut ClearColor,
    pub(crate) invalidated: &'a mut UiInvalidated,
}

pub(crate) fn apply_settings_action(action: &UiAction, context: SettingsActionContext) -> bool {
    let SettingsActionContext {
        window,
        state: studio,
        setup,
        diagnostics,
        theme,
        clear_color,
        invalidated,
    } = context;
    match &action.0 {
        UiCommand::App(AppCommand::Settings) => {
            studio.shell.route = StudioRoute::Settings;
            studio.shell.notice = None;
            studio.dialogs.open_settings_select = None;
            if studio.shell.settings_tab == SettingsTab::Storage {
                studio.jobs.request_cache_stats_refresh = true;
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::SettingsTab(tab)) => {
            studio.shell.route = StudioRoute::Settings;
            studio.shell.settings_tab = *tab;
            studio.shell.notice = None;
            studio.dialogs.open_settings_select = None;
            studio.jobs.request_cache_stats_refresh =
                matches!(studio.shell.settings_tab, SettingsTab::Storage);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::App(AppCommand::ToggleFullscreen) => {
            if let Some(error) = toggle_fullscreen(window, &mut studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::App(AppCommand::OpenLog) => {
            let path = app_core::default_uta_studio_dir().join("uta-studio.log");
            studio.shell.notice = Some(if path.is_file() {
                match open::that_detached(&path) {
                    Ok(()) => localized_message(
                        &studio.shell.config,
                        UiMessage::PathOpened,
                        &[("{path}", &path.display().to_string())],
                    ),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                }
            } else {
                localized_message(
                    &studio.shell.config,
                    UiMessage::LogMissing,
                    &[("{path}", &path.display().to_string())],
                )
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::App(AppCommand::RunDiagnostics) => {
            if diagnostics.receiver.is_some() {
                studio.shell.notice = Some("Feature diagnostics are already running.".to_string());
            } else {
                let (sender, receiver) = mpsc::channel();
                std::thread::spawn(move || {
                    let report = uta_studio_diagnostics::run_feature_diagnostics(
                        uta_studio_diagnostics::DiagnosticRequest {
                            file_hash: None,
                            include_export_smoke: true,
                        },
                    );
                    let _ = sender.send(report);
                });
                diagnostics.receiver = Some(Mutex::new(receiver));
                studio.dialogs.diagnostic_report = None;
                studio.shell.notice =
                    Some("Running safe diagnostics and temporary export checks…".to_string());
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RefreshRuntimeStatus) => {
            studio.shell.notice = Some("Runtime status refreshed from local files.".to_string());
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::OpenSettingsSelect(kind)) => {
            studio.dialogs.open_settings_select =
                if studio.dialogs.open_settings_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::SelectSettingsValue(kind, value)) => {
            match kind {
                SettingsSelectKind::UiLanguage => {
                    studio.shell.config.ui_language = (value != "system").then(|| value.clone());
                }
                SettingsSelectKind::ComputeBackend => {
                    studio.shell.config.compute_backend = Some(value.clone());
                }
                SettingsSelectKind::Separator => {
                    studio.shell.config.separator = Some(value.clone());
                    studio.shell.config.audio_processing = Some(
                        app_core::AudioProcessingSettings::from_legacy_separator(value),
                    );
                }
                SettingsSelectKind::SeparatorPreset => {
                    apply_separator_preset(&mut studio.shell.config, value);
                }
                SettingsSelectKind::AsrEngine => {
                    studio.shell.config.asr_engine = Some(value.clone());
                }
                SettingsSelectKind::WhisperModel => {
                    studio.shell.config.whisper_model = Some(value.clone());
                }
                SettingsSelectKind::AlignBackend => {
                    studio.shell.config.align_backend = Some(value.clone());
                }
                SettingsSelectKind::PitchModel => {
                    studio.shell.config.pitch_model = Some(value.clone());
                }
                SettingsSelectKind::AudioVocalModel => {
                    audio_settings_mut(&mut studio.shell.config).vocal_model_id =
                        Some(value.clone());
                }
                SettingsSelectKind::AudioAccompanimentModel => {
                    audio_settings_mut(&mut studio.shell.config).accompaniment_model_id =
                        (value != "none").then(|| value.clone());
                }
                SettingsSelectKind::AudioKaraokeModel => {
                    audio_settings_mut(&mut studio.shell.config).karaoke_model_id =
                        (value != "none").then(|| value.clone());
                }
                SettingsSelectKind::AudioDenoise
                | SettingsSelectKind::AudioDereverb
                | SettingsSelectKind::AudioCleanupOrder => {
                    let current = audio_settings(&studio.shell.config).vocal_cleanup_chain;
                    let denoise = match kind {
                        SettingsSelectKind::AudioDenoise => value != "none",
                        _ => current.iter().any(|id| id.contains("denoise")),
                    };
                    let dereverb = match kind {
                        SettingsSelectKind::AudioDereverb => value != "none",
                        _ => current.iter().any(|id| id.contains("dereverb")),
                    };
                    let dereverb_first = match kind {
                        SettingsSelectKind::AudioCleanupOrder => value == "dereverb_denoise",
                        _ => {
                            let d = current.iter().position(|id| id.contains("denoise"));
                            let r = current.iter().position(|id| id.contains("dereverb"));
                            matches!((d, r), (Some(di), Some(ri)) if ri < di)
                        }
                    };
                    audio_settings_mut(&mut studio.shell.config).vocal_cleanup_chain =
                        rewrite_cleanup_chain(&current, denoise, dereverb, dereverb_first);
                }
                SettingsSelectKind::AudioTorchBackend => {
                    audio_settings_mut(&mut studio.shell.config).torch_backend = value.clone();
                }
                SettingsSelectKind::AudioOnnxBackend => {
                    audio_settings_mut(&mut studio.shell.config).onnx_backend = value.clone();
                }
                SettingsSelectKind::AudioPrecisionPolicy => {
                    audio_settings_mut(&mut studio.shell.config).precision_policy = value.clone();
                }
            }
            if matches!(
                kind,
                SettingsSelectKind::AudioVocalModel
                    | SettingsSelectKind::AudioAccompanimentModel
                    | SettingsSelectKind::AudioKaraokeModel
                    | SettingsSelectKind::AudioDenoise
                    | SettingsSelectKind::AudioDereverb
                    | SettingsSelectKind::AudioCleanupOrder
            ) {
                let derived = audio_settings(&studio.shell.config).derived_legacy_separator();
                studio.shell.config.separator = Some(derived.to_string());
            }
            if studio.shell.config.compute_backend.as_deref() != Some("intel")
                && studio.shell.config.separator() == "openvino_demucs"
            {
                studio.shell.config.separator = Some("karaoke".to_string());
            }
            studio.dialogs.open_settings_select = None;
            studio.shell.notice = save_config_error(&studio.shell.config).or_else(|| {
                Some(match kind {
                    SettingsSelectKind::UiLanguage => "Interface language updated.".to_string(),
                    SettingsSelectKind::ComputeBackend => localized_message(
                        &studio.shell.config,
                        UiMessage::AccelerationSet,
                        &[("{backend}", settings_select_label(*kind, value))],
                    ),
                    SettingsSelectKind::SeparatorPreset => localized_message(
                        &studio.shell.config,
                        UiMessage::SeparationProfileApplied,
                        &[("{profile}", settings_select_label(*kind, value))],
                    ),
                    _ => localized_message(
                        &studio.shell.config,
                        UiMessage::AnalysisEngineSelected,
                        &[("{engine}", settings_select_label(*kind, value))],
                    ),
                })
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ToggleAnalysisAdvanced(section)) => {
            studio.dialogs.open_analysis_advanced =
                if studio.dialogs.open_analysis_advanced == Some(*section) {
                    None
                } else {
                    Some(*section)
                };
            studio.dialogs.open_settings_select = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::InstallAudioModel(model_id)) => {
            studio.shell.notice = Some(match app_core::install_audio_model(model_id) {
                Ok(status) => format!(
                    "{} is {}. Analysis uses it only after the next run.",
                    status.display_name, status.state
                ),
                Err(error) => error,
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RemoveAudioModel(model_id)) => {
            studio.shell.notice = Some(match app_core::remove_audio_model(model_id) {
                Ok(()) => "Audio model removed. Existing song cache and charts were not deleted."
                    .to_string(),
                Err(error) => error,
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RequestSetup(target)) => {
            if setup.receiver.is_some() {
                studio.shell.notice = Some("A runtime setup job is already running.".to_string());
            } else {
                studio.dialogs.pending_setup = Some(SetupRequest { target: *target });
                studio.shell.notice = None;
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::CancelSetup) => {
            studio.dialogs.pending_setup = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ConfirmSetup) => {
            if let Some(request) = studio.dialogs.pending_setup.take() {
                start_native_setup(&studio.shell.config, request, setup);
                studio.shell.notice = Some("Preparing analysis runtime…".to_string());
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }
        UiCommand::Library(LibraryCommand::RescanLibrary) => {
            if studio.shell.config.library_paths().is_empty() {
                studio.shell.notice = Some("Add a watched folder before scanning.".to_string());
            } else if studio.library.scanning {
                studio.shell.notice = Some("A library scan is already running.".to_string());
            } else {
                studio.library.scanning = true;
                studio.shell.notice = Some("Library scan started.".to_string());
                app_core::start_scan();
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ToggleTheme) => {
            studio.shell.config.dark_mode = Some(!theme.dark);
            studio.shell.notice = save_config_error(&studio.shell.config);
            *theme = StudioTheme::new(!theme.dark);
            clear_color.0 = theme.background;
            window.window_theme = Some(if theme.dark {
                WindowTheme::Dark
            } else {
                WindowTheme::Light
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::ChooseFolder) => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let mut paths = studio.shell.config.library_paths();
                if !paths.contains(&path) {
                    paths.push(path.clone());
                    studio.shell.config.library_source = Some(LibrarySource::Folders { paths });
                    if let Some(error) = save_config_error(&studio.shell.config) {
                        studio.shell.notice = Some(error);
                    } else {
                        studio.library.scanning = true;
                        studio.shell.notice =
                            Some("Folder added; library scan started.".to_string());
                        app_core::start_scan();
                        studio.library.refresh();
                        if studio.shell.route == StudioRoute::Folders {
                            studio.library.folder_browser.select_root(path);
                        }
                    }
                } else {
                    studio.shell.notice = Some("That folder is already watched.".to_string());
                }
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }
        UiCommand::Library(LibraryCommand::ChooseExportFolder) => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                studio.shell.config.export_path = Some(path);
                studio.shell.notice = save_config_error(&studio.shell.config)
                    .or_else(|| Some("Default export folder updated.".to_string()));
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }
        UiCommand::Library(LibraryCommand::ClearExportFolder) => {
            studio.shell.config.export_path = None;
            studio.shell.notice = save_config_error(&studio.shell.config)
                .or_else(|| Some("Export dialogs will use the system default.".to_string()));
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::SelectFolderRoot(path)) => {
            studio.library.folder_browser.select_root(path.clone());
            studio.shell.notice = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::FolderUp) => {
            if let Some(parent) = studio.library.folder_browser.parent() {
                studio.library.folder_browser.current = Some(parent);
                studio.library.folder_browser.context_menu = None;
                studio.library.folder_browser.refresh();
                studio.shell.notice = None;
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }
        UiCommand::Library(LibraryCommand::OpenFolderEntry(path)) => {
            studio.library.folder_browser.context_menu = None;
            if path.is_dir() {
                studio.library.folder_browser.current = Some(path.clone());
                studio.library.folder_browser.refresh();
                studio.shell.notice = None;
            } else {
                studio.shell.notice = Some(open_library_entry(path, &studio.shell.config));
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::RevealFolderEntry(path)) => {
            studio.library.folder_browser.context_menu = None;
            studio.shell.notice = Some(reveal_library_entry(path, &studio.shell.config));
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::DismissFolderContext) => {
            studio.library.folder_browser.context_menu = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::RequestRemoveFolder(path)) => {
            studio.library.folder_browser.context_menu = None;
            studio.library.folder_browser.pending_remove = Some(path.clone());
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::CancelRemoveFolder) => {
            studio.library.folder_browser.pending_remove = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Library(LibraryCommand::ConfirmRemoveFolder) => {
            if let Some(path) = studio.library.folder_browser.pending_remove.take() {
                let mut paths = studio.shell.config.library_paths();
                paths.retain(|entry| entry != &path);
                studio.shell.config.library_source = if paths.is_empty() {
                    None
                } else {
                    Some(LibrarySource::Folders { paths })
                };
                if let Some(error) = save_config_error(&studio.shell.config) {
                    studio.shell.notice = Some(error);
                } else {
                    if studio.shell.config.library_source.is_some() {
                        studio.library.scanning = true;
                        app_core::start_scan();
                    } else {
                        app_core::clear_library_index();
                        studio.library.scanning = false;
                    }
                    studio.shell.notice = Some(localized_message(
                        &studio.shell.config,
                        UiMessage::FolderStoppedWatching,
                        &[("{path}", &path.display().to_string())],
                    ));
                }
                studio.library.folder_browser = FolderBrowser::new(&studio.shell.config);
                studio.library.refresh();
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }
        UiCommand::Settings(SettingsCommand::AdjustBeamSize(delta)) => {
            studio.shell.config.beam_size = Some(
                (i64::from(studio.shell.config.beam_size()) + i64::from(*delta)).clamp(1, 16)
                    as u32,
            );
            if let Some(error) = save_config_error(&studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustBatchSize(delta)) => {
            studio.shell.config.batch_size = Some(
                (i64::from(studio.shell.config.batch_size()) + i64::from(*delta)).clamp(1, 16)
                    as u32,
            );
            if let Some(error) = save_config_error(&studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustSeparatorSegmentSize(delta)) => {
            studio.shell.config.separator_segment_size = Some(
                (i64::from(studio.shell.config.separator_segment_size()) + i64::from(*delta))
                    .clamp(64, 1024) as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustSeparatorOverlap(delta)) => {
            studio.shell.config.separator_overlap = Some(
                (i64::from(studio.shell.config.separator_overlap()) + i64::from(*delta))
                    .clamp(2, 32) as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustSeparatorBatchSize(delta)) => {
            studio.shell.config.separator_batch_size = Some(
                (i64::from(studio.shell.config.separator_batch_size()) + i64::from(*delta))
                    .clamp(1, 8) as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustSeparatorNormalization(delta)) => {
            studio.shell.config.separator_normalization_pct = Some(
                (i64::from(studio.shell.config.separator_normalization_pct()) + i64::from(*delta))
                    .clamp(1, 100) as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustDemucsShifts(delta)) => {
            studio.shell.config.demucs_shifts = Some(
                (i64::from(studio.shell.config.demucs_shifts()) + i64::from(*delta)).clamp(1, 8)
                    as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustDemucsOverlap(delta)) => {
            studio.shell.config.demucs_overlap_pct = Some(
                (i64::from(studio.shell.config.demucs_overlap_pct()) + i64::from(*delta))
                    .clamp(1, 95) as u32,
            );
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustUiFontScale(delta)) => {
            let current = ui_font_size_percent_to_points(studio.shell.config.font_scale_percent());
            let next = (i64::from(current) + i64::from(*delta) * i64::from(UI_FONT_SIZE_STEP_PX))
                .clamp(
                    i64::from(UI_FONT_SIZE_MIN_PX),
                    i64::from(UI_FONT_SIZE_MAX_PX),
                );
            let next_percent = ui_font_points_to_scale_percent(next as u32);
            studio.shell.config.font_scale_percent = Some(next_percent);
            set_ui_font_scale(next_percent as f32 / 100.0);
            studio.shell.notice = save_config_error(&studio.shell.config).or_else(|| {
                Some(localized_message(
                    &studio.shell.config,
                    UiMessage::FontSize,
                    &[("{size}", &format!("{next}px"))],
                ))
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ToggleAutoAnalyze) => {
            studio.shell.config.auto_analyze = Some(!studio.shell.config.auto_analyze());
            if let Some(error) = save_config_error(&studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::AdjustVocalThreshold(delta)) => {
            let current = (studio.shell.config.vocal_detection_threshold_pct() * 100.0).round();
            let value = (current + f64::from(*delta)).clamp(0.0, 60.0) / 100.0;
            studio.shell.config.vocal_detection_threshold_pct = Some(value);
            if let Some(error) = save_config_error(&studio.shell.config) {
                studio.shell.notice = Some(error);
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RestoreAnalysisDefaults) => {
            studio.shell.config.separator = Some("karaoke".to_string());
            studio.shell.config.separator_segment_size = None;
            studio.shell.config.separator_overlap = None;
            studio.shell.config.separator_batch_size = None;
            studio.shell.config.separator_normalization_pct = None;
            studio.shell.config.demucs_shifts = None;
            studio.shell.config.demucs_overlap_pct = None;
            studio.shell.config.asr_engine = Some("whisper".to_string());
            studio.shell.config.align_backend = Some("whisperx".to_string());
            studio.shell.config.pitch_model = Some("rmvpe".to_string());
            studio.shell.config.vocal_detection_threshold_pct = Some(0.15);
            studio.shell.config.whisper_model = Some("large-v3".to_string());
            studio.shell.config.beam_size = Some(8);
            studio.shell.config.batch_size = Some(8);
            studio.shell.config.compute_backend = Some("cpu".to_string());
            studio.shell.config.auto_analyze = Some(false);
            studio.shell.notice = save_config_error(&studio.shell.config)
                .or_else(|| Some("Analysis defaults restored.".to_string()));
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RequestClearCache(scope)) => {
            studio.dialogs.pending_cache_clear = Some(*scope);
            studio.shell.notice = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::CancelClearCache) => {
            studio.dialogs.pending_cache_clear = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ConfirmClearCache) => {
            if let Some(scope) = studio.dialogs.pending_cache_clear.take() {
                match scope {
                    CacheClearScope::Generated => {
                        app_core::CacheDir::new().clear_all();
                        studio.library.refresh();
                        studio.jobs.request_cache_stats_refresh = true;
                        studio.shell.notice = Some(
                            "Generated cache cleared. Source media was not changed.".to_string(),
                        );
                    }
                    CacheClearScope::Models => {
                        app_core::clear_models();
                        studio.jobs.request_cache_stats_refresh = true;
                        studio.shell.notice = Some(
                                "Downloaded models cleared. Runtime setup now reports the missing artifacts."
                                    .to_string(),
                            );
                    }
                }
                invalidated.invalidate(UiDirtyRegion::Settings);
            }
        }

        _ => return false,
    }
    true
}
