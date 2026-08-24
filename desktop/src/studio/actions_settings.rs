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
            let route_changed = studio.shell.route != StudioRoute::Settings;
            studio.shell.route = StudioRoute::Settings;
            studio.shell.notice = None;
            studio.dialogs.open_settings_select = None;
            studio.dialogs.plan_preview_draft = None;
            if studio.shell.settings_tab == SettingsTab::Storage {
                studio.jobs.request_cache_stats_refresh = true;
            }
            if matches!(
                studio.shell.settings_tab,
                SettingsTab::Analysis | SettingsTab::Models
            ) && studio.jobs.model_settings_job.current.is_none()
            {
                studio.jobs.request_model_settings_refresh = true;
            }
            invalidated.invalidate(if route_changed {
                UiDirtyRegion::Chrome
            } else {
                UiDirtyRegion::Settings
            });
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Settings(SettingsCommand::SettingsTab(tab)) => {
            let route_changed = studio.shell.route != StudioRoute::Settings;
            studio.shell.route = StudioRoute::Settings;
            studio.shell.settings_tab = *tab;
            studio.shell.notice = None;
            studio.dialogs.open_settings_select = None;
            studio.dialogs.plan_preview_draft = None;
            studio.jobs.request_cache_stats_refresh =
                matches!(studio.shell.settings_tab, SettingsTab::Storage);
            studio.jobs.request_model_settings_refresh =
                matches!(
                    studio.shell.settings_tab,
                    SettingsTab::Analysis | SettingsTab::Models
                ) && studio.jobs.model_settings_job.current.is_none();
            invalidated.invalidate(if route_changed {
                UiDirtyRegion::Chrome
            } else {
                UiDirtyRegion::Settings
            });
            invalidated.invalidate(UiDirtyRegion::Dialog);
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
            app_core::invalidate_analysis_runtime_status_cache();
            studio.jobs.request_model_settings_refresh = true;
            studio.shell.notice = Some("Refreshing local model and runtime status…".to_string());
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
                SettingsSelectKind::Separator => {
                    let mut settings = audio_settings(&studio.shell.config);
                    settings.vocal_model_id = Some(value.clone());
                    settings.multistem_model_id = None;
                    settings.migrated_profile = None;
                    settings.runtime_policy = "validated_auto".to_string();
                    studio.shell.config.separator = Some("native_workflow".to_string());
                    studio.shell.config.audio_processing = Some(settings);
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
                SettingsSelectKind::AnalysisTarget => {
                    studio.shell.config.analysis_experience.default_target = match value.as_str() {
                        "transcript" => app_core::AnalysisDefaultTarget::Transcript,
                        "alignment" => app_core::AnalysisDefaultTarget::Alignment,
                        "pitch_evidence" => app_core::AnalysisDefaultTarget::PitchEvidence,
                        "instrumental" => app_core::AnalysisDefaultTarget::Instrumental,
                        _ => app_core::AnalysisDefaultTarget::FullCandidate,
                    };
                }
                SettingsSelectKind::AudioVocalModel => {
                    let settings = audio_settings_mut(&mut studio.shell.config);
                    settings.migrated_profile = None;
                    settings.multistem_model_id = None;
                    settings.vocal_model_id = Some(value.clone());
                }
                SettingsSelectKind::AudioAccompanimentModel => {
                    let settings = audio_settings_mut(&mut studio.shell.config);
                    settings.migrated_profile = None;
                    settings.multistem_model_id = None;
                    settings
                        .vocal_model_id
                        .get_or_insert_with(|| app_core::DEFAULT_VOCAL_MODEL_ID.to_string());
                    settings.accompaniment_model_id = Some(value.clone());
                }
                SettingsSelectKind::AudioKaraokeModel => {
                    audio_settings_mut(&mut studio.shell.config).karaoke_model_id =
                        (value != "none").then(|| value.clone());
                }
                SettingsSelectKind::AudioVocalPostprocess1
                | SettingsSelectKind::AudioVocalPostprocess2 => {
                    let current = audio_settings(&studio.shell.config).vocal_cleanup_chain;
                    let slot =
                        usize::from(matches!(kind, SettingsSelectKind::AudioVocalPostprocess2));
                    let settings = audio_settings_mut(&mut studio.shell.config);
                    settings.migrated_profile = None;
                    settings.multistem_model_id = None;
                    settings
                        .vocal_model_id
                        .get_or_insert_with(|| app_core::DEFAULT_VOCAL_MODEL_ID.to_string());
                    settings.vocal_cleanup_chain = rewrite_cleanup_slot(&current, slot, value);
                }
                SettingsSelectKind::AudioBgmPostprocess1
                | SettingsSelectKind::AudioBgmPostprocess2 => {
                    let current = audio_settings(&studio.shell.config).accompaniment_cleanup_chain;
                    let slot =
                        usize::from(matches!(kind, SettingsSelectKind::AudioBgmPostprocess2));
                    let settings = audio_settings_mut(&mut studio.shell.config);
                    settings.migrated_profile = None;
                    settings.multistem_model_id = None;
                    settings
                        .vocal_model_id
                        .get_or_insert_with(|| app_core::DEFAULT_VOCAL_MODEL_ID.to_string());
                    settings.accompaniment_cleanup_chain =
                        rewrite_cleanup_slot(&current, slot, value);
                }
            }
            if matches!(
                kind,
                SettingsSelectKind::AudioVocalModel
                    | SettingsSelectKind::AudioAccompanimentModel
                    | SettingsSelectKind::AudioKaraokeModel
                    | SettingsSelectKind::AudioVocalPostprocess1
                    | SettingsSelectKind::AudioVocalPostprocess2
                    | SettingsSelectKind::AudioBgmPostprocess1
                    | SettingsSelectKind::AudioBgmPostprocess2
            ) {
                let derived = audio_settings(&studio.shell.config).derived_legacy_separator();
                studio.shell.config.separator = Some(derived.to_string());
            }
            studio.dialogs.open_settings_select = None;
            studio.shell.notice = save_config_error(&studio.shell.config).or_else(|| {
                Some(match kind {
                    SettingsSelectKind::UiLanguage => "Interface language updated.".to_string(),
                    _ => localized_message(
                        &studio.shell.config,
                        UiMessage::AnalysisEngineSelected,
                        &[("{engine}", settings_select_label(*kind, value))],
                    ),
                })
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::SetModelBackend(model_id, backend)) => {
            let valid = backend.as_deref().is_none_or(|backend| {
                matches!(
                    backend,
                    "openvino" | "vulkan" | "native_dsp" | "diagnostic_cpu"
                )
            });
            if !valid {
                studio.shell.notice = Some("Unsupported model backend selection.".to_string());
            } else {
                match backend {
                    Some(backend) => {
                        studio
                            .shell
                            .config
                            .model_backend_overrides
                            .insert(model_id.clone(), backend.clone());
                    }
                    None => {
                        studio.shell.config.model_backend_overrides.remove(model_id);
                    }
                }
                studio.shell.notice = save_config_error(&studio.shell.config).or_else(|| {
                    Some(format!(
                        "Backend preference updated for {model_id}. Existing artifacts are unchanged; the next Plan Preview validates this route."
                    ))
                });
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::SetAnalysisQuality(quality)) => {
            studio.shell.config.analysis_experience.quality_profile = *quality;
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::TogglePreserveContinuousPitch) => {
            studio
                .shell
                .config
                .analysis_experience
                .preserve_continuous_pitch = !studio.shell.config.preserve_continuous_pitch();
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ToggleAnalysisQuantization) => {
            studio.shell.config.analysis_experience.enable_quantization =
                !studio.shell.config.enable_quantization();
            studio.shell.notice = save_config_error(&studio.shell.config);
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::InstallAudioModel(model_id)) => {
            if setup.receiver.is_some() {
                studio.shell.notice =
                    Some("Another model or runtime installation is already running.".to_string());
            } else {
                let model_id = model_id.clone();
                let (sender, receiver) = mpsc::channel();
                setup.receiver = Some(Mutex::new(receiver));
                setup.progress = None;
                setup.logs.clear();
                setup
                    .logs
                    .push(format!("Installing audio model {model_id}…"));
                setup.last_ui_refresh = None;
                studio.shell.notice = Some("Audio model installation started.".to_string());
                if let Err(error) = std::thread::Builder::new()
                    .name("uta-studio-audio-model-install".to_string())
                    .spawn(move || {
                        let result = app_core::install_audio_model(&model_id).map(|status| {
                            let _ = sender.send(SetupEvent::Log(format!(
                                "Installed {}. Analysis uses it only after the next run.",
                                status.display_name
                            )));
                        });
                        let _ = sender.send(SetupEvent::Complete(result));
                    })
                {
                    setup.receiver = None;
                    setup.logs.clear();
                    studio.shell.notice =
                        Some(format!("Could not start audio model installation: {error}"));
                }
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::RemoveAudioModel(model_id)) => {
            app_core::invalidate_analysis_runtime_status_cache();
            studio.shell.notice = Some(match app_core::remove_audio_model(model_id) {
                Ok(()) => "Audio model removed. Existing song cache and charts were not deleted."
                    .to_string(),
                Err(error) => error,
            });
            studio.jobs.request_model_settings_refresh = true;
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
        UiCommand::Settings(SettingsCommand::RestoreAnalysisDefaults) => {
            studio.shell.config.analysis_experience =
                app_core::AnalysisExperienceSettings::default();
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
