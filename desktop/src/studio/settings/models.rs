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
        "Install or verify local resources, then tune the parameters owned by each model. Provider choice exists only in the Analysis workspace model selector.",
    );
    if native_setup.receiver.is_some() || native_setup.progress.is_some() {
        spawn_setup_progress_panel(parent, font.clone(), icons.clone(), native_setup, theme);
    }

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "PACKAGED RUNTIME CONTRACTS",
        "The canonical Engine path uses versioned package parameters. Legacy segment, overlap, batch, normalization, beam, and voiced-sensitivity settings are hidden because they are not encoded into the compiled Workflow request.",
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "No editable model parameters",
        "Models & runtime is lifecycle-only: install, verify, inspect, repair, and remove. Provider and analysis intent remain in the Analysis workspace; exact request behavior remains in Plan Preview.",
        None::<(String, UiAction)>,
    );
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "LOCAL RESOURCES",
        "Installation and verification are lifecycle operations only. The exact model used by a song is chosen from Quick model selection in the Analysis workspace.",
    );

    if let Some(snapshot) = session.model_settings_job.current.as_ref() {
        let status = &snapshot.runtime_status;
        spawn_model_runtime_status_row(parent, font.clone(), theme, session.config, status);
        spawn_model_backend_settings(
            parent,
            font.clone(),
            theme,
            session.config,
            &snapshot.runtime_registry,
        );
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            "Testing runtime routing",
            "The local testing policy prefers OpenVINO, then available Vulkan/native routes, and may use CPU reference execution when no faster local route is available.",
            Some((
                if status.native_analyzer_available {
                    "Verify…"
                } else {
                    "Repair package…"
                },
                UiAction::from(SettingsCommand::RequestSetup(Some(
                    app_core::ModelDownloadTarget::SharedRuntime,
                ))),
            )),
        );
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "RESOURCES BY CAPABILITY",
            "Manage installation state here. Exact request readiness is evaluated by Plan Preview; this page never writes provider preferences.",
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
            "Local separation resources",
            &[app_core::ModelDownloadTarget::RoFormer],
        );
        spawn_audio_catalog_models(
            parent,
            font.clone(),
            theme,
            &snapshot.audio_catalog,
            snapshot.audio_catalog_error.as_deref(),
        );
        spawn_model_stage(
            parent,
            font.clone(),
            theme,
            session,
            &status.models,
            "02 · LYRICS TRANSCRIPTION",
            "Lyrics transcription",
            "Qwen is the baseline transcription resource. FireRed remains an optional challenger and never becomes mandatory from this page.",
            "Transcription resources",
            &[
                app_core::ModelDownloadTarget::FireRed,
                app_core::ModelDownloadTarget::QwenAsr,
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
            "Alignment resources",
            &[app_core::ModelDownloadTarget::QwenAlign],
        );
        spawn_model_stage(
            parent,
            font.clone(),
            theme,
            session,
            &status.models,
            "04 · MELODY",
            "Melody & pitch",
            "Keeps RMVPE continuous F0, GAME note/boundary evidence, and optional challengers distinct. Experimental routes are enabled for local testing.",
            "Pitch and note resources",
            &[
                app_core::ModelDownloadTarget::Pitch,
                app_core::ModelDownloadTarget::Fcpe,
                app_core::ModelDownloadTarget::Game,
                app_core::ModelDownloadTarget::Stars,
                app_core::ModelDownloadTarget::BasicPitch,
            ],
        );
    } else {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "LOCAL RESOURCE STATUS",
            "Model and runtime inspection runs outside the UI thread so this page stays responsive.",
        );
        let (title, description) = if session.model_settings_job.receiver.is_some() {
            (
                "Checking local models…",
                "The current page remains interactive while Runtime Manager inspects installed files and available backends.".to_string(),
            )
        } else if let Some(error) = session.model_settings_job.error.as_deref() {
            (
                "Could not read local model status",
                format!("{error} You can retry without leaving this page."),
            )
        } else {
            (
                "Local model status has not been loaded",
                "Run a local status check to populate installation and backend details."
                    .to_string(),
            )
        };
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            title,
            description,
            Some((
                if session.model_settings_job.receiver.is_some() {
                    "Checking…"
                } else {
                    "Check now"
                },
                UiAction::from(SettingsCommand::RefreshRuntimeStatus),
            )),
        );
    }
}

fn backend_value(backend: app_core::NativeBackend) -> &'static str {
    match backend {
        app_core::NativeBackend::OpenVino => "openvino",
        app_core::NativeBackend::Vulkan => "vulkan",
        app_core::NativeBackend::NativeDsp => "native_dsp",
        app_core::NativeBackend::CpuReference => "diagnostic_cpu",
    }
}

fn backend_label(backend: app_core::NativeBackend) -> &'static str {
    match backend {
        app_core::NativeBackend::OpenVino => "OpenVINO",
        app_core::NativeBackend::Vulkan => "Vulkan / GGML",
        app_core::NativeBackend::NativeDsp => "Native DSP",
        app_core::NativeBackend::CpuReference => "Diagnostic CPU",
    }
}

fn validation_label(validation: app_core::ValidationState) -> &'static str {
    match validation {
        app_core::ValidationState::ProductionPinned => "production pinned",
        app_core::ValidationState::BenchmarkCandidate => "benchmark candidate",
        app_core::ValidationState::Experimental => "experimental",
        app_core::ValidationState::Unsupported => "unsupported",
    }
}

fn model_backend_display_name(model_id: &str) -> String {
    match model_id {
        "bs_roformer_vocals_ep317" => "BS-RoFormer Vocals EP317".to_string(),
        "melband_roformer_denoise_aufr33" => "MelBand RoFormer Denoise".to_string(),
        "melband_roformer_dereverb_anvuew" => "MelBand RoFormer Dereverb".to_string(),
        "melband_roformer_inst_v2" => "MelBand RoFormer Instrumental V2".to_string(),
        "melband_roformer_harmony" => "MelBand RoFormer Lead Isolation".to_string(),
        "qwen3_asr_1_7b" => "Qwen3 ASR 1.7B".to_string(),
        "qwen3_forced_aligner_0_6b" => "Qwen3 Forced Aligner 0.6B".to_string(),
        "firered_asr2_aed" => "FireRed ASR2 AED".to_string(),
        "basic_pitch" => "Basic Pitch".to_string(),
        "rmvpe" => "RMVPE".to_string(),
        "fcpe" => "FCPE".to_string(),
        "game" => "GAME".to_string(),
        "stars" => "STARS".to_string(),
        "rosvot" => "ROSVOT".to_string(),
        other => other.replace('_', " "),
    }
}

fn spawn_model_backend_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    config: &AppConfig,
    registry: &[app_core::NativeModelRuntime],
) {
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "MODEL BACKEND ROUTING",
        "Choose a backend per model. Default keeps Runtime Manager's pinned route; unavailable or unvalidated explicit choices fail in Plan Preview and never fall back.",
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Backend routing note",
        config.model_backend_note.clone(),
        None::<(String, UiAction)>,
    );
    if registry.is_empty() {
        spawn_setting_row(
            parent,
            font,
            theme,
            "Model backend capabilities unavailable",
            "Refresh runtime status after installing the packaged Runtime Manager.",
            None::<(String, UiAction)>,
        );
        return;
    }
    for model in registry {
        let selected = config.model_backend_overrides.get(&model.model_id);
        let default = model
            .pinned_backend
            .map(backend_label)
            .unwrap_or("unresolved");
        let capabilities = model
            .backends
            .iter()
            .filter(|capability| capability.validation != app_core::ValidationState::Unsupported)
            .map(|capability| {
                format!(
                    "{} ({})",
                    backend_label(capability.backend),
                    validation_label(capability.validation)
                )
            })
            .collect::<Vec<_>>()
            .join(" · ");
        let mut actions = vec![(
            if selected.is_none() {
                format!("✓ Default · {default}")
            } else {
                format!("Default · {default}")
            },
            UiAction::from(SettingsCommand::SetModelBackend(
                model.model_id.clone(),
                None,
            )),
        )];
        actions.extend(
            model
                .backends
                .iter()
                .filter(|capability| {
                    capability.validation != app_core::ValidationState::Unsupported
                })
                .map(|capability| {
                    let value = backend_value(capability.backend);
                    (
                        if selected.is_some_and(|selected| selected == value) {
                            format!("✓ {}", backend_label(capability.backend))
                        } else {
                            backend_label(capability.backend).to_string()
                        },
                        UiAction::from(SettingsCommand::SetModelBackend(
                            model.model_id.clone(),
                            Some(value.to_string()),
                        )),
                    )
                }),
        );
        spawn_setting_row_with_actions(
            parent,
            font.clone(),
            theme,
            model_backend_display_name(&model.model_id),
            format!(
                "Model ID: {} · Available: {}. Explicit CPU remains diagnostic only; Qwen retains its pinned Vulkan runtime.",
                model.model_id, capabilities
            ),
            actions,
        );
    }
}

pub(crate) fn spawn_audio_catalog_models(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    catalog: &app_core::AudioModelCatalogSummary,
    error: Option<&str>,
) {
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "AUDIO PROCESSING CATALOG",
        "Install only after confirming the model name, source, size, and license. Analysis stays offline.",
    );
    if let Some(error) = error {
        spawn_wrapped_text(
            parent,
            font.clone(),
            format!("Audio model catalog unavailable: {error}"),
            9.0,
            theme.destructive,
        );
    }
    spawn_wrapped_text(
        parent,
        font.clone(),
        format!("Catalog {}", catalog.catalog_version),
        9.0,
        theme.muted_foreground,
    );
    for model in &catalog.models {
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
        None,
    );
    for model in models
        .iter()
        .filter(|model| targets.contains(&model.target))
    {
        spawn_model_install_row(parent, font.clone(), theme, session, model, title);
    }
}

pub(crate) fn model_install_role(
    _config: &AppConfig,
    target: app_core::ModelDownloadTarget,
) -> &'static str {
    use app_core::ModelDownloadTarget;
    match target {
        ModelDownloadTarget::FireRed
        | ModelDownloadTarget::Fcpe
        | ModelDownloadTarget::Stars
        | ModelDownloadTarget::BasicPitch => "Optional challenger",
        ModelDownloadTarget::Game => "Candidate requirement",
        _ => "Baseline resource",
    }
}

fn model_status_copy(model: &app_core::ModelInstallStatus, stage: &str) -> String {
    format!(
        "{} Capability group: {stage}. Runtime fact: {} · backend: {}.",
        model.description, model.validation, model.backend
    )
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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(15)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
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
                        if role.starts_with("Optional") {
                            theme.muted_foreground
                        } else {
                            theme.primary
                        },
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        if model.available {
                            "Ready to test"
                        } else {
                            "Needs setup"
                        },
                        if model.available {
                            theme.primary
                        } else {
                            theme.editor_warning
                        },
                    );
                });
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    model_status_copy(model, stage),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                min_width: px(180),
                max_width: px(SETTINGS_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_CONTROL_WIDTH),
                flex_grow: 1.0,
                margin: UiRect::top(px(2)),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|actions| {
                spawn_compact_action_button(
                    actions,
                    font,
                    theme,
                    "Manage…",
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
            "Baseline component set available",
            "BASELINE READY",
            theme.primary,
            "Global component health is available. Each action still derives readiness from its exact requested artifacts in Plan Preview.",
        )
    } else if status.ready {
        (
            "Baseline component set available",
            "CONTRACT REVIEW",
            theme.editor_warning,
            "The baseline set is present but its runtime contract needs review. Request-specific Plan Preview remains authoritative.",
        )
    } else {
        (
            "Baseline component set incomplete",
            "PARTIAL",
            theme.editor_warning,
            "One or more baseline resources are unavailable. This does not imply every partial analysis action is blocked; inspect the exact request plan.",
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
                        "Native analyzer",
                        status.native_analyzer_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "OpenVINO model worker",
                        status.openvino_runtime_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "GGML Vulkan model worker",
                        status.ggml_vulkan_runtime_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Qwen ASR Vulkan",
                        status.qwen_asr_runtime_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Qwen align Vulkan",
                        status.qwen_align_runtime_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Pitch model",
                        status.pitch_model_available,
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

#[cfg(test)]
mod tests {
    use super::{model_install_role, model_status_copy};

    #[test]
    fn lifecycle_roles_do_not_claim_optional_challengers_are_selected() {
        let config = app_core::AppConfig::default();
        assert_eq!(
            model_install_role(&config, app_core::ModelDownloadTarget::FireRed),
            "Optional challenger"
        );
        assert_eq!(
            model_install_role(&config, app_core::ModelDownloadTarget::Fcpe),
            "Optional challenger"
        );
        assert_eq!(
            model_install_role(&config, app_core::ModelDownloadTarget::Game),
            "Candidate requirement"
        );
        assert_eq!(
            model_install_role(&config, app_core::ModelDownloadTarget::Pitch),
            "Baseline resource"
        );
    }

    #[test]
    fn optional_expert_copy_uses_the_current_runtime_validation_fact() {
        let promoted = app_core::ModelInstallStatus {
            target: app_core::ModelDownloadTarget::FireRed,
            label: "FireRed".to_string(),
            description: "Optional transcription challenger.".to_string(),
            available: true,
            backend: "vulkan".to_string(),
            validation: "production_pinned".to_string(),
        };
        let copy = model_status_copy(&promoted, "Transcription resources");
        assert!(copy.contains("production_pinned"));
        assert!(!copy.contains("benchmark_candidate"));
    }
}
