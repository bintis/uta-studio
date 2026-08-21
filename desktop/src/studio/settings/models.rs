use super::*;
use crate::studio::*;

pub(crate) fn spawn_model_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    native_setup: &NativeSetup,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "LOCAL INTELLIGENCE",
        "Models & runtime",
        "Checks are read-only; downloads start only after an explicit setup confirmation.",
    );
    if native_setup.receiver.is_some() || native_setup.progress.is_some() {
        spawn_setup_progress_panel(parent, font.clone(), icons.clone(), native_setup, theme);
    }
    let status = app_core::analysis_runtime_status();
    spawn_model_runtime_status_row(parent, font.clone(), theme, session.config, &status);
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Acceleration",
        "Choose the hardware target before installing the analysis environment.",
        SettingsSelectKind::ComputeBackend,
        session,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Shared analysis runtime",
        "Setup reuses compatible host ffmpeg, uv, Python, and existing model files. Nothing downloads until you confirm.",
        Some((
            if status.managed_runtime_available {
                "Reconfigure…"
            } else {
                "Set up…"
            },
            UiAction::from(SettingsCommand::RequestSetup(None)),
        )),
    );
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "MODEL FILES BY ANALYSIS STAGE",
        "This page only manages local files. Choose which engine is active in Analysis; every download still requires confirmation.",
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "01 · STEM SEPARATION",
        "Vocal & BGM separation",
        "Installs the model files used by the independent vocal and BGM branches.",
        vocal_separation_label(session.config),
        &[app_core::ModelDownloadTarget::Separator],
    );
    spawn_audio_catalog_models(parent, font.clone(), theme, session);
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes lyrics. Compatibility and language-detection models are identified separately from the selected engine.",
        transcription_summary(session.config),
        &[
            app_core::ModelDownloadTarget::OpenVinoWhisper,
            app_core::ModelDownloadTarget::Parakeet,
            app_core::ModelDownloadTarget::WhisperLanguageDetection,
            app_core::ModelDownloadTarget::Whisper,
        ],
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "03 · WORD TIMING",
        "Word timing & alignment",
        "Refines recognized or supplied lyrics into editable word timings.",
        align_backend_label(session.config.align_backend()),
        &[
            app_core::ModelDownloadTarget::Alignment,
            app_core::ModelDownloadTarget::MmsKaraokeAlignment,
        ],
    );
    spawn_model_stage(
        parent,
        font,
        theme,
        session,
        &status.models,
        "04 · MELODY",
        "Melody & pitch",
        "Detects the sung fundamental frequency and creates note pitches.",
        pitch_model_label(session.config.pitch_model()),
        &[app_core::ModelDownloadTarget::Pitch],
    );
}

pub(crate) fn spawn_audio_catalog_models(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    _session: &StudioSessionView<'_>,
) {
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "AUDIO PROCESSING CATALOG",
        "Install only after confirming the model name, source, size, and license. Analysis stays offline.",
    );
    let catalog = app_core::list_audio_models().unwrap_or(app_core::AudioModelCatalogSummary {
        schema_version: 1,
        catalog_version: "2026.08.1".to_string(),
        models: Vec::new(),
    });
    spawn_wrapped_text(
        parent,
        font.clone(),
        format!("Catalog {}", catalog.catalog_version),
        9.0,
        theme.muted_foreground,
    );
    for model in catalog.models {
        let state = match model.state.as_str() {
            "installed" => "Installed",
            "integrity_failed" => "Checksum failed",
            _ => "Not installed",
        };
        let backends = model.supported_backends.join(" / ");
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            model.display_name.clone(),
            format!(
                "{} · {} · {} · {}",
                model.purpose, model.architecture, backends, model.license.source_attribution
            ),
            Some((
                if model.state == "installed" {
                    "Remove"
                } else {
                    "Install…"
                }
                .to_string(),
                if model.state == "installed" {
                    UiAction::from(SettingsCommand::RemoveAudioModel(model.model_id.clone()))
                } else {
                    UiAction::from(SettingsCommand::InstallAudioModel(model.model_id.clone()))
                },
            )),
        );
        spawn_wrapped_text(
            parent,
            font.clone(),
            format!(
                "{state}. Optional catalog weight — analysis does not download it automatically."
            ),
            8.0,
            theme.muted_foreground,
        );
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_model_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    models: &[app_core::ModelInstallStatus],
    eyebrow: &'static str,
    title: &'static str,
    description: &'static str,
    current: impl Into<String>,
    targets: &[app_core::ModelDownloadTarget],
) {
    if !models.iter().any(|model| targets.contains(&model.target)) {
        return;
    }
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        eyebrow,
        title,
        description,
        current,
        None,
        Some((
            "Configure in Analysis…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Analysis)),
        )),
    );
    for model in models
        .iter()
        .filter(|model| targets.contains(&model.target))
    {
        spawn_model_install_row(parent, font.clone(), theme, session, model, title);
    }
}

pub(crate) fn model_install_role(
    config: &AppConfig,
    target: app_core::ModelDownloadTarget,
) -> &'static str {
    use app_core::ModelDownloadTarget;
    match target {
        ModelDownloadTarget::Whisper
            if config.asr_engine() == "parakeet"
                || config.compute_backend.as_deref() == Some("intel") =>
        {
            "Fallback"
        }
        ModelDownloadTarget::WhisperLanguageDetection => "Support",
        ModelDownloadTarget::MmsKaraokeAlignment if config.align_backend() != "mms_karaoke" => {
            "Optional"
        }
        _ => "Selected",
    }
}

pub(crate) fn spawn_model_install_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    model: &app_core::ModelInstallStatus,
    stage: &'static str,
) {
    let role = model_install_role(session.config, model.target);
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(86),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(15)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn(Node {
                    align_items: AlignItems::Center,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(5),
                    column_gap: px(7),
                    ..default()
                })
                .with_children(|title| {
                    spawn_text(
                        title,
                        font.clone(),
                        model.label.clone(),
                        12.0,
                        theme.foreground,
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        role,
                        if role == "Optional" {
                            theme.muted_foreground
                        } else {
                            theme.primary
                        },
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        if model.available {
                            "Installed"
                        } else {
                            "Missing"
                        },
                        if model.available {
                            theme.primary
                        } else {
                            theme.destructive
                        },
                    );
                });
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!("{} Used by Analysis > {stage}.", model.description),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|actions| {
                spawn_compact_action_button(
                    actions,
                    font,
                    theme,
                    if model.available {
                        "Reinstall…"
                    } else {
                        "Download…"
                    },
                    UiAction::from(SettingsCommand::RequestSetup(Some(model.target))),
                );
            });
        });
}

pub(crate) fn spawn_model_runtime_status_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    config: &AppConfig,
    status: &app_core::AnalysisRuntimeStatus,
) {
    let (headline, badge, status_color, status_hint) = if status.ready
        && status.runtime_contract_current
    {
        (
            "Ready to analyze",
            "OK",
            theme.primary,
            "The selected runtime and every required model are available locally.",
        )
    } else if status.ready {
        (
            "Ready to analyze",
            "REBUILD OPTIONAL",
            theme.editor_warning,
            "This installation still works. A newer runtime contract is available; use Reconfigure only if you want it. Analysis is not blocked.",
        )
    } else {
        (
            "Setup required",
            "MISSING",
            theme.destructive,
            "Some required components are missing. Open setup to install or repair.",
        )
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(168),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                flex_direction: FlexDirection::Column,
                row_gap: px(10),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    ..default()
                })
                .with_children(|status_row| {
                    status_row
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|status_copy| {
                            status_copy
                                .spawn(Node {
                                    align_items: AlignItems::Center,
                                    column_gap: px(8),
                                    ..default()
                                })
                                .with_children(|headline_row| {
                                    spawn_text(
                                        headline_row,
                                        font.clone(),
                                        "Runtime status",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                    spawn_text(
                                        headline_row,
                                        font.clone(),
                                        headline,
                                        12.0,
                                        theme.foreground,
                                    );
                                    headline_row.spawn((
                                        Node {
                                            padding: UiRect::axes(px(8), px(3)),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(status_color.with_alpha(0.16)),
                                        BorderColor::all(status_color.with_alpha(0.45)),
                                        children![(
                                            Text::new(badge),
                                            ui_text_font(font.clone(), 8.0),
                                            TextColor(status_color),
                                        )],
                                    ));
                                });
                            spawn_wrapped_text(
                                status_copy,
                                font.clone(),
                                status_hint.to_string(),
                                9.0,
                                theme.muted_foreground,
                            );
                            if !status.ready && !status.missing.is_empty() {
                                spawn_wrapped_text(
                                    status_copy,
                                    font.clone(),
                                    localized_message(
                                        config,
                                        UiMessage::RuntimeMissingComponents,
                                        &[("{components}", &status.missing.join(" · "))],
                                    ),
                                    8.5,
                                    theme.destructive,
                                );
                            }
                        });
                    spawn_setting_actions(
                        status_row,
                        font.clone(),
                        theme,
                        vec![(
                            "Check again".to_string(),
                            UiAction::from(SettingsCommand::RefreshRuntimeStatus),
                        )],
                    );
                });
            panel
                .spawn(Node {
                    width: percent(100),
                    max_width: px(760),
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|stack| {
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "ffmpeg",
                        status.ffmpeg_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "uv",
                        status.uv_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Python",
                        status.system_python_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Analyzer",
                        status.analyzer_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Pitch model",
                        status.pitch_model_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Selected models",
                        status.selected_models_available,
                    );
                    spawn_runtime_contract_row(
                        stack,
                        font.clone(),
                        theme,
                        status.runtime_contract_current,
                        status.ready,
                    );
                });
        });
}

pub(crate) fn spawn_runtime_contract_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    current: bool,
    usable: bool,
) {
    let (badge, color) = if current {
        ("available", theme.primary)
    } else if usable {
        ("outdated", theme.editor_warning)
    } else {
        ("missing", theme.destructive)
    };
    parent
        .spawn((
            Node {
                min_width: px(180),
                min_height: px(32),
                flex_basis: px(220),
                flex_grow: 1.0,
                max_width: px(250),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(px(9), px(5)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), "Runtime contract", 9.0, theme.foreground);
            row.spawn((
                Node {
                    padding: UiRect::axes(px(8), px(3)),
                    border_radius: BorderRadius::all(px(999.0)),
                    ..default()
                },
                BackgroundColor(color.with_alpha(0.16)),
                BorderColor::all(color.with_alpha(0.45)),
                children![(
                    Text::new(badge),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(color),
                )],
            ));
        });
}

pub(crate) fn spawn_runtime_component_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    available: bool,
) {
    let color = if available {
        theme.primary
    } else {
        theme.destructive
    };
    let badge_label = if available {
        availability(true)
    } else {
        availability(false)
    };
    let badge_background = if available {
        theme.primary.with_alpha(0.16)
    } else {
        theme.destructive.with_alpha(0.16)
    };
    let badge_border = if available {
        theme.primary.with_alpha(0.45)
    } else {
        theme.destructive.with_alpha(0.45)
    };

    parent
        .spawn((
            Node {
                min_width: px(180),
                min_height: px(32),
                flex_basis: px(220),
                flex_grow: 1.0,
                max_width: px(250),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                padding: UiRect::axes(px(9), px(5)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.5)),
        ))
        .with_children(|row| {
            spawn_text(row, font.clone(), label, 9.0, theme.foreground);
            row.spawn((
                Node {
                    padding: UiRect::axes(px(8), px(3)),
                    border_radius: BorderRadius::all(px(999.0)),
                    ..default()
                },
                BackgroundColor(badge_background),
                BorderColor::all(badge_border),
                children![(
                    Text::new(badge_label),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(color),
                )],
            ));
        });
}

pub(crate) fn transcription_summary(config: &AppConfig) -> String {
    if config.asr_engine() == "parakeet" {
        "Parakeet v3".to_string()
    } else if config.compute_backend.as_deref() == Some("intel") {
        "OpenVINO Whisper large-v3-turbo".to_string()
    } else {
        format!(
            "Whisper {}",
            settings_select_label(SettingsSelectKind::WhisperModel, config.whisper_model(),)
        )
    }
}

pub(crate) fn transcription_model_target(config: &AppConfig) -> app_core::ModelDownloadTarget {
    if config.asr_engine() == "parakeet" {
        app_core::ModelDownloadTarget::Parakeet
    } else if config.compute_backend.as_deref() == Some("intel") {
        app_core::ModelDownloadTarget::OpenVinoWhisper
    } else {
        app_core::ModelDownloadTarget::Whisper
    }
}

pub(crate) fn alignment_model_target(config: &AppConfig) -> Option<app_core::ModelDownloadTarget> {
    match config.align_backend() {
        "qwen" => Some(app_core::ModelDownloadTarget::Alignment),
        "mms_karaoke" => Some(app_core::ModelDownloadTarget::MmsKaraokeAlignment),
        _ => None,
    }
}

pub(crate) fn analysis_stage_status(
    status: &app_core::AnalysisRuntimeStatus,
    target: Option<app_core::ModelDownloadTarget>,
) -> (String, bool) {
    match target.and_then(|target| model_available(status, target)) {
        Some(true) => ("Installed".to_string(), true),
        Some(false) => ("Model missing".to_string(), false),
        None if status.analyzer_available => ("Runtime managed".to_string(), true),
        None => ("Runtime missing".to_string(), false),
    }
}

pub(crate) fn spawn_analysis_pipeline(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    status: &app_core::AnalysisRuntimeStatus,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.3)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|panel| {
            spawn_text(
                panel,
                font.clone(),
                "CURRENT ANALYSIS PIPELINE",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                panel,
                font.clone(),
                "The same four stages and names are used on Models & runtime.",
                9.0,
                theme.muted_foreground,
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|pipeline| {
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "01 · Vocals",
                        vocal_separation_label(session.config),
                        analysis_stage_status(
                            status,
                            Some(app_core::ModelDownloadTarget::Separator),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "02 · Lyrics",
                        transcription_summary(session.config),
                        analysis_stage_status(
                            status,
                            Some(transcription_model_target(session.config)),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "03 · Timing",
                        align_backend_label(session.config.align_backend()),
                        analysis_stage_status(status, alignment_model_target(session.config)),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "04 · Pitch",
                        pitch_model_label(session.config.pitch_model()),
                        analysis_stage_status(status, Some(app_core::ModelDownloadTarget::Pitch)),
                    );
                });
        });
}

pub(crate) fn spawn_analysis_pipeline_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stage: &'static str,
    selected: impl Into<String>,
    status: (String, bool),
) {
    parent
        .spawn((
            Node {
                min_width: px(190),
                min_height: px(70),
                flex_basis: px(220),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(11)),
                row_gap: px(5),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.48)),
            BorderColor::all(theme.border.with_alpha(0.46)),
        ))
        .with_children(|card| {
            spawn_text(card, font.clone(), stage, 8.0, theme.muted_foreground);
            spawn_text(card, font.clone(), selected, 10.0, theme.foreground);
            spawn_settings_badge(
                card,
                font,
                status.0,
                if status.1 {
                    theme.primary
                } else {
                    theme.destructive
                },
            );
        });
}
