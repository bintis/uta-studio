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
            studio.dialogs.open_model_runtime_select = None;
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
            studio.dialogs.open_model_runtime_select = None;
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
        UiCommand::Settings(SettingsCommand::OpenModelDownloads) => {
            studio.dialogs.model_downloads_open = true;
            studio.dialogs.open_settings_select = None;
            studio.dialogs.open_model_runtime_select = None;
            if studio.jobs.model_settings_job.current.is_none() {
                studio.jobs.request_model_settings_refresh = true;
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Settings(SettingsCommand::CloseModelDownloads) => {
            studio.dialogs.model_downloads_open = false;
            studio.dialogs.pending_setup = None;
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        UiCommand::Settings(SettingsCommand::OpenSettingsSelect(kind)) => {
            studio.dialogs.open_model_runtime_select = None;
            studio.dialogs.open_settings_select =
                if studio.dialogs.open_settings_select == Some(*kind) {
                    None
                } else {
                    Some(*kind)
                };
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::ToggleModelRuntimeSelect(model_id)) => {
            studio.dialogs.open_settings_select = None;
            studio.dialogs.open_model_runtime_select =
                if studio.dialogs.open_model_runtime_select.as_deref() == Some(model_id) {
                    None
                } else {
                    Some(model_id.clone())
                };
            invalidated.invalidate(UiDirtyRegion::Settings);
        }
        UiCommand::Settings(SettingsCommand::SelectSettingsValue(kind, value)) => {
            match kind {
                SettingsSelectKind::UiLanguage => {
                    studio.shell.config.ui_language = (value != "system").then(|| value.clone());
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
        UiCommand::Settings(SettingsCommand::SelectFusionProvider(provider)) => {
            studio.shell.notice = Some(match app_core::configure_fusion_provider(provider) {
                Ok(report) => {
                    studio.jobs.request_model_settings_refresh = true;
                    let name = report
                        .providers
                        .iter()
                        .find(|status| status.provider == *provider)
                        .map(|status| status.display_name.as_str())
                        .unwrap_or(provider);
                    format!(
                        "{name} selected for AI judgment. Provider authentication remains owned by its CLI."
                    )
                }
                Err(error) => format!(
                    "Could not select Fusion provider: {error}. Install both the provider CLI and its packaged Uta adapter, then scan again."
                ),
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Settings(SettingsCommand::ClearFusionProvider) => {
            studio.shell.notice = Some(match app_core::clear_fusion_provider() {
                Ok(_) => {
                    studio.jobs.request_model_settings_refresh = true;
                    "Fusion provider selection cleared. Runtime Manager may use a configured custom adapter instead."
                        .to_string()
                }
                Err(error) => format!("Could not clear Fusion provider: {error}"),
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Settings(SettingsCommand::ChooseFusionAgentAdapter) => {
            if let Some(path) = rfd::FileDialog::new().pick_file() {
                studio.shell.notice = Some(match app_core::configure_fusion_agent_adapter(&path) {
                    Ok(status) if status.usable => {
                        studio.jobs.request_model_settings_refresh = true;
                        "Fusion Agent Adapter configured and verified. AI judgment remains an explicit per-workflow choice.".to_string()
                    }
                    Ok(status) => {
                        let reasons = status
                            .reasons
                            .iter()
                            .map(readiness_reason_label)
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!(
                            "Fusion Agent Adapter is configured but unusable: {}",
                            if reasons.is_empty() {
                                "No readiness reason reported"
                            } else {
                                &reasons
                            }
                        )
                    }
                    Err(error) => format!(
                        "Could not configure Fusion Agent Adapter: {error}. Choose an executable with a valid Uta adapter manifest; plain coding-agent CLIs are not compatible."
                    ),
                });
            }
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Settings(SettingsCommand::ClearFusionAgentAdapter) => {
            studio.shell.notice = Some(match app_core::clear_fusion_agent_adapter() {
                Ok(_) => {
                    studio.jobs.request_model_settings_refresh = true;
                    "Fusion Agent Adapter configuration cleared. AI workflows now fail closed; Algorithm workflows are unaffected.".to_string()
                }
                Err(error) => format!("Could not clear Fusion Agent Adapter: {error}"),
            });
            invalidated.invalidate(UiDirtyRegion::Settings);
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        UiCommand::Settings(SettingsCommand::SetModelBackend(model_id, backend)) => {
            studio.dialogs.open_model_runtime_select = None;
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
        UiCommand::Settings(SettingsCommand::SetModelDevice(model_id, device)) => {
            let valid = device
                .as_deref()
                .is_none_or(|device| matches!(device, "cpu" | "gpu" | "integrated_gpu"));
            if !valid {
                studio.shell.notice = Some("Unsupported model device selection.".to_string());
            } else {
                match device {
                    Some(device) => {
                        studio
                            .shell
                            .config
                            .model_device_overrides
                            .insert(model_id.clone(), device.clone());
                    }
                    None => {
                        studio.shell.config.model_device_overrides.remove(model_id);
                    }
                }
                studio.shell.notice = save_config_error(&studio.shell.config).or_else(|| {
                    Some(format!(
                        "Device preference recorded for {model_id}. This is captured for upcoming multi-device routing; it does not yet change which physical device Runtime Manager selects."
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
            let dark = !theme.dark;
            let transparent = studio.shell.config.window_transparency.unwrap_or(false);
            studio.shell.config.dark_mode = Some(dark);
            studio.shell.notice = save_config_error(&studio.shell.config);
            *theme = StudioTheme::new_with_transparency(dark, transparent);
            clear_color.0 = window_clear_color(theme, transparent);
            window.window_theme = Some(if theme.dark {
                WindowTheme::Dark
            } else {
                WindowTheme::Light
            });
            invalidated.invalidate(UiDirtyRegion::Chrome);
        }
        UiCommand::Settings(SettingsCommand::ToggleWindowTransparency) => {
            let transparent = !studio.shell.config.window_transparency.unwrap_or(false);
            studio.shell.config.window_transparency = Some(transparent);
            studio.shell.notice = save_config_error(&studio.shell.config);
            *theme = StudioTheme::new_with_transparency(theme.dark, transparent);
            clear_color.0 = window_clear_color(theme, transparent);
            invalidated.invalidate(UiDirtyRegion::Chrome);
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
                            "Generated cache cleared. Source media and installed models were not changed."
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
