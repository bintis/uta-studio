use crate::studio::*;

#[allow(clippy::too_many_arguments)]
pub(crate) fn apply_settings_action(
    action: &UiAction,
    mut window: &mut Window,
    session: &mut StudioSession,
    mut setup: &mut NativeSetup,
    diagnostics: &mut NativeDiagnostics,
    theme: &mut StudioTheme,
    clear_color: &mut ClearColor,
    invalidated: &mut UiInvalidated,
) -> bool {
    match action {
        UiAction::Settings => {
            session.route = StudioRoute::Settings;
            session.notice = None;
            session.open_settings_select = None;
            if session.settings_tab == SettingsTab::Storage {
                session.request_cache_stats_refresh = true;
            }
            invalidated.0 = true;
        }
        UiAction::SettingsTab(tab) => {
            session.route = StudioRoute::Settings;
            session.settings_tab = *tab;
            session.notice = None;
            session.open_settings_select = None;
            session.request_cache_stats_refresh =
                matches!(session.settings_tab, SettingsTab::Storage);
            invalidated.0 = true;
        }
        UiAction::ToggleFullscreen => {
            if let Some(error) = toggle_fullscreen(&mut window, &mut session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::OpenLog => {
            let path = app_core::default_uta_studio_dir().join("uta-studio.log");
            session.notice = Some(if path.is_file() {
                match open::that_detached(&path) {
                    Ok(()) => format!("Opened {}", path.display()),
                    Err(error) => format!("Could not open {}: {error}", path.display()),
                }
            } else {
                format!("No application log exists yet at {}", path.display())
            });
            invalidated.0 = true;
        }
        UiAction::RunDiagnostics => {
            if diagnostics.receiver.is_some() {
                session.notice = Some("Feature diagnostics are already running.".to_string());
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
                session.diagnostic_report = None;
                session.notice =
                    Some("Running safe diagnostics and temporary export checks…".to_string());
            }
            invalidated.0 = true;
        }
        UiAction::RefreshRuntimeStatus => {
            session.notice = Some("Runtime status refreshed from local files.".to_string());
            invalidated.0 = true;
        }
        UiAction::OpenSettingsSelect(kind) => {
            session.open_settings_select = if session.open_settings_select == Some(*kind) {
                None
            } else {
                Some(*kind)
            };
            invalidated.0 = true;
        }
        UiAction::SelectSettingsValue(kind, value) => {
            match kind {
                SettingsSelectKind::UiLanguage => {
                    session.config.ui_language = (value != "system").then(|| value.clone());
                }
                SettingsSelectKind::ComputeBackend => {
                    session.config.compute_backend = Some(value.clone());
                }
                SettingsSelectKind::Separator => {
                    session.config.separator = Some(value.clone());
                    session.config.audio_processing = Some(
                        app_core::AudioProcessingSettings::from_legacy_separator(value),
                    );
                }
                SettingsSelectKind::SeparatorPreset => {
                    apply_separator_preset(&mut session.config, value);
                }
                SettingsSelectKind::AsrEngine => {
                    session.config.asr_engine = Some(value.clone());
                }
                SettingsSelectKind::WhisperModel => {
                    session.config.whisper_model = Some(value.clone());
                }
                SettingsSelectKind::AlignBackend => {
                    session.config.align_backend = Some(value.clone());
                }
                SettingsSelectKind::PitchModel => {
                    session.config.pitch_model = Some(value.clone());
                }
                SettingsSelectKind::AudioVocalModel => {
                    audio_settings_mut(&mut session.config).vocal_model_id = Some(value.clone());
                }
                SettingsSelectKind::AudioAccompanimentModel => {
                    audio_settings_mut(&mut session.config).accompaniment_model_id =
                        (value != "none").then(|| value.clone());
                }
                SettingsSelectKind::AudioKaraokeModel => {
                    audio_settings_mut(&mut session.config).karaoke_model_id =
                        (value != "none").then(|| value.clone());
                }
                SettingsSelectKind::AudioDenoise
                | SettingsSelectKind::AudioDereverb
                | SettingsSelectKind::AudioCleanupOrder => {
                    let current = audio_settings(&session.config).vocal_cleanup_chain;
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
                    audio_settings_mut(&mut session.config).vocal_cleanup_chain =
                        rewrite_cleanup_chain(&current, denoise, dereverb, dereverb_first);
                }
                SettingsSelectKind::AudioTorchBackend => {
                    audio_settings_mut(&mut session.config).torch_backend = value.clone();
                }
                SettingsSelectKind::AudioOnnxBackend => {
                    audio_settings_mut(&mut session.config).onnx_backend = value.clone();
                }
                SettingsSelectKind::AudioPrecisionPolicy => {
                    audio_settings_mut(&mut session.config).precision_policy = value.clone();
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
                let derived = audio_settings(&session.config).derived_legacy_separator();
                session.config.separator = Some(derived.to_string());
            }
            if session.config.compute_backend.as_deref() != Some("intel")
                && session.config.separator() == "openvino_demucs"
            {
                session.config.separator = Some("karaoke".to_string());
            }
            session.open_settings_select = None;
            session.notice = save_config_error(&session.config).or_else(|| {
                    Some(match kind {
                        SettingsSelectKind::UiLanguage => {
                            "Interface language updated.".to_string()
                        }
                        SettingsSelectKind::ComputeBackend => format!(
                            "Acceleration set to {}. Reconfigure the runtime to apply it.",
                            settings_select_label(*kind, value)
                        ),
                        SettingsSelectKind::SeparatorPreset => format!(
                            "{} separation profile applied. Existing stems change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                        _ => format!(
                            "{} selected. Existing charts change only after re-analysis.",
                            settings_select_label(*kind, value)
                        ),
                    })
                });
            invalidated.0 = true;
        }
        UiAction::ToggleAnalysisAdvanced(section) => {
            session.open_analysis_advanced = if session.open_analysis_advanced == Some(*section) {
                None
            } else {
                Some(*section)
            };
            session.open_settings_select = None;
            invalidated.0 = true;
        }
        UiAction::InstallAudioModel(model_id) => {
            session.notice = Some(match app_core::install_audio_model(model_id) {
                Ok(status) => format!(
                    "{} is {}. Analysis uses it only after the next run.",
                    status.display_name, status.state
                ),
                Err(error) => error,
            });
            invalidated.0 = true;
        }
        UiAction::RemoveAudioModel(model_id) => {
            session.notice = Some(match app_core::remove_audio_model(model_id) {
                Ok(()) => "Audio model removed. Existing song cache and charts were not deleted."
                    .to_string(),
                Err(error) => error,
            });
            invalidated.0 = true;
        }
        UiAction::RequestSetup(target) => {
            if setup.receiver.is_some() {
                session.notice = Some("A runtime setup job is already running.".to_string());
            } else {
                session.pending_setup = Some(SetupRequest { target: *target });
                session.notice = None;
            }
            invalidated.0 = true;
        }
        UiAction::CancelSetup => {
            session.pending_setup = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmSetup => {
            if let Some(request) = session.pending_setup.take() {
                start_native_setup(&session.config, request, &mut setup);
                session.notice = Some("Preparing analysis runtime…".to_string());
                invalidated.0 = true;
            }
        }
        UiAction::RescanLibrary => {
            if session.config.library_paths().is_empty() {
                session.notice = Some("Add a watched folder before scanning.".to_string());
            } else if session.scanning {
                session.notice = Some("A library scan is already running.".to_string());
            } else {
                session.scanning = true;
                session.notice = Some("Library scan started.".to_string());
                app_core::start_scan();
            }
            invalidated.0 = true;
        }
        UiAction::ToggleTheme => {
            session.config.dark_mode = Some(!theme.dark);
            session.notice = save_config_error(&session.config);
            *theme = StudioTheme::new(!theme.dark);
            clear_color.0 = theme.background;
            window.window_theme = Some(if theme.dark {
                WindowTheme::Dark
            } else {
                WindowTheme::Light
            });
            invalidated.0 = true;
        }
        UiAction::ChooseFolder => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                let mut paths = session.config.library_paths();
                if !paths.contains(&path) {
                    paths.push(path.clone());
                    session.config.library_source = Some(LibrarySource::Folders { paths });
                    if let Some(error) = save_config_error(&session.config) {
                        session.notice = Some(error);
                    } else {
                        session.scanning = true;
                        session.notice = Some("Folder added; library scan started.".to_string());
                        app_core::start_scan();
                        session.refresh_library();
                        if session.route == StudioRoute::Folders {
                            session.folder_browser.select_root(path);
                        }
                    }
                } else {
                    session.notice = Some("That folder is already watched.".to_string());
                }
                invalidated.0 = true;
            }
        }
        UiAction::ChooseExportFolder => {
            if let Some(path) = rfd::FileDialog::new().pick_folder() {
                session.config.export_path = Some(path);
                session.notice = save_config_error(&session.config)
                    .or_else(|| Some("Default export folder updated.".to_string()));
                invalidated.0 = true;
            }
        }
        UiAction::ClearExportFolder => {
            session.config.export_path = None;
            session.notice = save_config_error(&session.config)
                .or_else(|| Some("Export dialogs will use the system default.".to_string()));
            invalidated.0 = true;
        }
        UiAction::SelectFolderRoot(path) => {
            session.folder_browser.select_root(path.clone());
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::FolderUp => {
            if let Some(parent) = session.folder_browser.parent() {
                session.folder_browser.current = Some(parent);
                session.folder_browser.context_menu = None;
                session.folder_browser.refresh();
                session.notice = None;
                invalidated.0 = true;
            }
        }
        UiAction::OpenFolderEntry(path) => {
            session.folder_browser.context_menu = None;
            if path.is_dir() {
                session.folder_browser.current = Some(path.clone());
                session.folder_browser.refresh();
                session.notice = None;
            } else {
                session.notice = Some(open_library_entry(path, &session.config));
            }
            invalidated.0 = true;
        }
        UiAction::RevealFolderEntry(path) => {
            session.folder_browser.context_menu = None;
            session.notice = Some(reveal_library_entry(path, &session.config));
            invalidated.0 = true;
        }
        UiAction::DismissFolderContext => {
            session.folder_browser.context_menu = None;
            invalidated.0 = true;
        }
        UiAction::RequestRemoveFolder(path) => {
            session.folder_browser.context_menu = None;
            session.folder_browser.pending_remove = Some(path.clone());
            invalidated.0 = true;
        }
        UiAction::CancelRemoveFolder => {
            session.folder_browser.pending_remove = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmRemoveFolder => {
            if let Some(path) = session.folder_browser.pending_remove.take() {
                let mut paths = session.config.library_paths();
                paths.retain(|entry| entry != &path);
                session.config.library_source = if paths.is_empty() {
                    None
                } else {
                    Some(LibrarySource::Folders { paths })
                };
                if let Some(error) = save_config_error(&session.config) {
                    session.notice = Some(error);
                } else {
                    if session.config.library_source.is_some() {
                        session.scanning = true;
                        app_core::start_scan();
                    } else {
                        app_core::clear_library_index();
                        session.scanning = false;
                    }
                    session.notice = Some(format!(
                        "Stopped watching {}. No source media was moved or deleted.",
                        path.display()
                    ));
                }
                session.folder_browser = FolderBrowser::new(&session.config);
                session.refresh_library();
                invalidated.0 = true;
            }
        }
        UiAction::AdjustBeamSize(delta) => {
            session.config.beam_size = Some(
                (i64::from(session.config.beam_size()) + i64::from(*delta)).clamp(1, 16) as u32,
            );
            if let Some(error) = save_config_error(&session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::AdjustBatchSize(delta) => {
            session.config.batch_size = Some(
                (i64::from(session.config.batch_size()) + i64::from(*delta)).clamp(1, 16) as u32,
            );
            if let Some(error) = save_config_error(&session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::AdjustSeparatorSegmentSize(delta) => {
            session.config.separator_segment_size = Some(
                (i64::from(session.config.separator_segment_size()) + i64::from(*delta))
                    .clamp(64, 1024) as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustSeparatorOverlap(delta) => {
            session.config.separator_overlap = Some(
                (i64::from(session.config.separator_overlap()) + i64::from(*delta)).clamp(2, 32)
                    as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustSeparatorBatchSize(delta) => {
            session.config.separator_batch_size = Some(
                (i64::from(session.config.separator_batch_size()) + i64::from(*delta)).clamp(1, 8)
                    as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustSeparatorNormalization(delta) => {
            session.config.separator_normalization_pct = Some(
                (i64::from(session.config.separator_normalization_pct()) + i64::from(*delta))
                    .clamp(1, 100) as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustDemucsShifts(delta) => {
            session.config.demucs_shifts = Some(
                (i64::from(session.config.demucs_shifts()) + i64::from(*delta)).clamp(1, 8) as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustDemucsOverlap(delta) => {
            session.config.demucs_overlap_pct = Some(
                (i64::from(session.config.demucs_overlap_pct()) + i64::from(*delta)).clamp(1, 95)
                    as u32,
            );
            session.notice = save_config_error(&session.config);
            invalidated.0 = true;
        }
        UiAction::AdjustUiFontScale(delta) => {
            let current = ui_font_size_percent_to_points(session.config.font_scale_percent());
            let next = (i64::from(current) + i64::from(*delta) * i64::from(UI_FONT_SIZE_STEP_PX))
                .clamp(
                    i64::from(UI_FONT_SIZE_MIN_PX),
                    i64::from(UI_FONT_SIZE_MAX_PX),
                );
            let next_percent = ui_font_points_to_scale_percent(next as u32);
            session.config.font_scale_percent = Some(next_percent);
            set_ui_font_scale(next_percent as f32 / 100.0);
            session.notice = save_config_error(&session.config)
                .or_else(|| Some(format!("Font size: {}px", next)));
            invalidated.0 = true;
        }
        UiAction::ToggleAutoAnalyze => {
            session.config.auto_analyze = Some(!session.config.auto_analyze());
            if let Some(error) = save_config_error(&session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::AdjustVocalThreshold(delta) => {
            let current = (session.config.vocal_detection_threshold_pct() * 100.0).round();
            let value = (current + f64::from(*delta)).clamp(0.0, 60.0) / 100.0;
            session.config.vocal_detection_threshold_pct = Some(value);
            if let Some(error) = save_config_error(&session.config) {
                session.notice = Some(error);
            }
            invalidated.0 = true;
        }
        UiAction::RestoreAnalysisDefaults => {
            session.config.separator = Some("karaoke".to_string());
            session.config.separator_segment_size = None;
            session.config.separator_overlap = None;
            session.config.separator_batch_size = None;
            session.config.separator_normalization_pct = None;
            session.config.demucs_shifts = None;
            session.config.demucs_overlap_pct = None;
            session.config.asr_engine = Some("whisper".to_string());
            session.config.align_backend = Some("whisperx".to_string());
            session.config.pitch_model = Some("rmvpe".to_string());
            session.config.vocal_detection_threshold_pct = Some(0.15);
            session.config.whisper_model = Some("large-v3".to_string());
            session.config.beam_size = Some(8);
            session.config.batch_size = Some(8);
            session.config.compute_backend = Some("cpu".to_string());
            session.config.auto_analyze = Some(false);
            session.notice = save_config_error(&session.config)
                .or_else(|| Some("Analysis defaults restored.".to_string()));
            invalidated.0 = true;
        }
        UiAction::RequestClearCache(scope) => {
            session.pending_cache_clear = Some(*scope);
            session.notice = None;
            invalidated.0 = true;
        }
        UiAction::CancelClearCache => {
            session.pending_cache_clear = None;
            invalidated.0 = true;
        }
        UiAction::ConfirmClearCache => {
            if let Some(scope) = session.pending_cache_clear.take() {
                match scope {
                    CacheClearScope::Generated => {
                        app_core::CacheDir::new().clear_all();
                        session.refresh_library();
                        session.request_cache_stats_refresh = true;
                        session.notice = Some(
                            "Generated cache cleared. Source media was not changed.".to_string(),
                        );
                    }
                    CacheClearScope::Models => {
                        app_core::clear_models();
                        session.request_cache_stats_refresh = true;
                        session.notice = Some(
                                "Downloaded models cleared. Runtime setup now reports the missing artifacts."
                                    .to_string(),
                            );
                    }
                }
                invalidated.0 = true;
            }
        }

        _ => return false,
    }
    true
}
