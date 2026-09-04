use super::*;
use crate::studio::*;

pub(crate) fn spawn_model_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    _native_setup: &NativeSetup,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "MODELS & RUNTIME",
        "Models & runtime",
        "Inspect installed tools and tune runtime parameters for each model. Lifecycle actions remain explicit; these controls never choose analysis outputs or change workflow topology.",
    );

    spawn_settings_group(
        parent,
        font.clone(),
        theme,
        "MODEL RUNTIME ROUTING",
        "Choose a device and Runtime Manager route per installed model. These controls do not select workflow outputs.",
        |group| {
            if let Some(snapshot) = session.model_settings_job.current.as_ref() {
                spawn_model_backend_settings(
                    group,
                    font.clone(),
                    icons,
                    theme,
                    session.config,
                    &snapshot.runtime_models,
                    session.open_model_runtime_select.as_deref(),
                );
            } else {
                let (title, description) = if session.model_settings_job.receiver.is_some() {
                    (
                "Reading model parameters…",
                "Loading the installed model registry without changing any analysis or model files."
                    .to_string(),
            )
                } else if let Some(error) = session.model_settings_job.error.as_deref() {
                    (
                        "Could not read model parameters",
                        format!("{error} Retry to reload the local model registry."),
                    )
                } else {
                    (
                "Model parameters are not loaded",
                "Load the local model registry to show editable per-model runtime parameters."
                    .to_string(),
            )
                };
                spawn_setting_row(
                    group,
                    font.clone(),
                    theme,
                    title,
                    description,
                    Some((
                        if session.model_settings_job.receiver.is_some() {
                            "Loading…"
                        } else {
                            "Load parameters"
                        },
                        UiAction::from(SettingsCommand::RefreshRuntimeStatus),
                    )),
                );
            }
        },
    );

    spawn_settings_group(
        parent,
        font.clone(),
        theme,
        "FUSION AGENT",
        "Discover a provider CLI and its compatible Uta adapter without launching either one. Credentials and provider charges remain external.",
        |group| {
            let snapshot = session.model_settings_job.current.as_ref();
            let provider_report = snapshot.and_then(|snapshot| snapshot.fusion_providers.as_ref());
            let selected_provider =
                provider_report.and_then(|report| report.selected_provider.as_deref());
            if let Some(report) = provider_report {
                for provider in &report.providers {
                    let state = if provider.usable {
                        "Ready"
                    } else if !provider.available {
                        "Provider CLI missing"
                    } else if !provider.adapter_available {
                        "Uta adapter missing or incompatible"
                    } else {
                        "Unavailable"
                    };
                    let version = provider.adapter_version.as_deref().unwrap_or("unknown");
                    let reasons = if provider.reasons.is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", provider.reasons.join(", "))
                    };
                    let description = format!(
                        "{state} · executable {} · adapter {version}{reasons}. Credentials and provider charges remain owned by the provider CLI.",
                        provider.executable_name
                    );
                    if provider.selected {
                        spawn_setting_row(
                            group,
                            font.clone(),
                            theme,
                            format!("{} · Selected", provider.display_name),
                            description,
                            None::<(String, UiAction)>,
                        );
                    } else if provider.usable {
                        spawn_setting_row(
                            group,
                            font.clone(),
                            theme,
                            provider.display_name.clone(),
                            description,
                            Some((
                                format!("Use {}", provider.display_name),
                                UiAction::from(SettingsCommand::SelectFusionProvider(
                                    provider.provider.clone(),
                                )),
                            )),
                        );
                    } else {
                        spawn_setting_row(
                            group,
                            font.clone(),
                            theme,
                            provider.display_name.clone(),
                            description,
                            None::<(String, UiAction)>,
                        );
                    }
                }
                spawn_setting_row(
                    group,
                    font.clone(),
                    theme,
                    "External provider disclosure",
                    report.network_disclosure.clone(),
                    None::<(String, UiAction)>,
                );
            } else {
                let description = snapshot
                    .and_then(|snapshot| snapshot.fusion_providers_error.as_deref())
                    .map_or_else(
                        || "Provider discovery is loading.".to_string(),
                        |error| format!("Could not read provider discovery: {error}"),
                    );
                spawn_setting_row(
                    group,
                    font.clone(),
                    theme,
                    "Fusion providers",
                    description,
                    Some((
                        "Scan again",
                        UiAction::from(SettingsCommand::RefreshRuntimeStatus),
                    )),
                );
            }
            if selected_provider.is_some() {
                spawn_setting_row(
                    group,
                    font.clone(),
                    theme,
                    "Clear selected provider",
                    "Clears only Runtime Manager's provider identity. It does not change provider credentials or delete any executable.",
                    Some((
                        "Clear",
                        UiAction::from(SettingsCommand::ClearFusionProvider),
                    )),
                );
            }

            let adapter = snapshot.and_then(|snapshot| snapshot.fusion_agent_adapter.as_ref());
            let adapter_description = if let Some(status) = adapter {
                let state = if status.usable {
                    "Usable"
                } else if matches!(status.install_state, app_core::InstallStateWireV1::Absent) {
                    "Missing"
                } else {
                    "Unusable"
                };
                let identity = status.tool_identity.as_deref().unwrap_or("unverified");
                let version = status.tool_version.as_deref().unwrap_or("unknown version");
                let reasons = if status.reasons.is_empty() {
                    String::new()
                } else {
                    format!(
                        " · {}",
                        status
                            .reasons
                            .iter()
                            .map(readiness_reason_label)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                };
                format!(
                    "Effective tool: {state} · {identity} · {version}{reasons}. Preview checks readiness without contacting the provider."
                )
            } else {
                snapshot
                    .and_then(|snapshot| snapshot.fusion_agent_adapter_error.as_deref())
                    .map_or_else(
                        || "Effective adapter status is loading.".to_string(),
                        |error| format!("Could not read adapter status: {error}"),
                    )
            };
            let mut adapter_actions = vec![(
                "Scan again".to_string(),
                UiAction::from(SettingsCommand::RefreshRuntimeStatus),
            )];
            if selected_provider.is_none() {
                adapter_actions.push((
                    "Choose custom adapter…".to_string(),
                    UiAction::from(SettingsCommand::ChooseFusionAgentAdapter),
                ));
            }
            spawn_setting_row_with_actions(
                group,
                font.clone(),
                theme,
                "Effective Fusion Agent Adapter",
                adapter_description,
                adapter_actions,
            );
            if selected_provider.is_none()
                && adapter.is_some_and(|status| {
                    matches!(
                        status.origin,
                        app_core::ResourceOriginWireV1::ExternalConfiguration
                    )
                })
            {
                spawn_setting_row(
                    group,
                    font,
                    theme,
                    "Clear custom Fusion Agent Adapter",
                    "Clears Runtime Manager's configured external-tool path without deleting the executable.",
                    Some((
                        "Clear",
                        UiAction::from(SettingsCommand::ClearFusionAgentAdapter),
                    )),
                );
            }
        },
    );
}

fn backend_value(backend: app_core::RuntimeBackendPresentation) -> &'static str {
    match backend {
        app_core::RuntimeBackendPresentation::OpenVino => "openvino",
        app_core::RuntimeBackendPresentation::Vulkan => "vulkan",
        app_core::RuntimeBackendPresentation::NativeDsp => "native_dsp",
        app_core::RuntimeBackendPresentation::CpuReference => "diagnostic_cpu",
    }
}

fn backend_label(backend: app_core::RuntimeBackendPresentation) -> &'static str {
    match backend {
        app_core::RuntimeBackendPresentation::OpenVino => "OpenVINO",
        app_core::RuntimeBackendPresentation::Vulkan => "GGML",
        app_core::RuntimeBackendPresentation::NativeDsp => "Native DSP",
        app_core::RuntimeBackendPresentation::CpuReference => "Diagnostic CPU",
    }
}

const DEVICE_CLASS_OPTIONS: [(&str, &str); 3] = [
    ("cpu", "CPU"),
    ("gpu", "GPU"),
    ("integrated_gpu", "Integrated GPU"),
];

fn validation_label(validation: app_core::RuntimeValidationPresentation) -> &'static str {
    match validation {
        app_core::RuntimeValidationPresentation::ProductionPinned => "production pinned",
        app_core::RuntimeValidationPresentation::BenchmarkCandidate => "benchmark candidate",
        app_core::RuntimeValidationPresentation::Experimental => "experimental",
        app_core::RuntimeValidationPresentation::Unsupported => "unsupported",
    }
}

fn model_backend_display_name(model_id: &str) -> String {
    match model_id {
        "bs_roformer_leap_xe90_vocals" => "BS-RoFormer Leap XE90 Vocals".to_string(),
        "bs_polarformer_public_instrumental" => "BS-PolarFormer Public Instrumental".to_string(),
        "jbm555_cectc_80" => "JBM555 CE-CTC 80".to_string(),
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
    icons: Handle<Image>,
    theme: &StudioTheme,
    config: &AppConfig,
    registry: &[app_core::RuntimeModelPresentation],
    open_runtime_select: Option<&str>,
) {
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
        spawn_model_runtime_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            config,
            model,
            open_runtime_select == Some(model.model_id.as_str()),
        );
    }
}

/// GGML's Vulkan route resolves GPU/iGPU to the matching physical adapter at
/// run start; every other route still only records the preference (Runtime
/// Manager has no per-device resolution for OpenVINO/native-DSP/CPU yet).
fn device_preference_caption(
    selected_backend: Option<app_core::RuntimeBackendPresentation>,
) -> &'static str {
    if selected_backend == Some(app_core::RuntimeBackendPresentation::Vulkan) {
        "Device selects the matching physical adapter (GPU = discrete, iGPU = integrated) when this model runs."
    } else {
        "Device is a request preference for upcoming multi-device routing; it does not yet force a physical adapter."
    }
}

fn selected_device_label(selected: Option<&str>) -> &'static str {
    match selected {
        Some("cpu") => "CPU",
        Some("gpu") => "GPU",
        Some("integrated_gpu") => "iGPU",
        _ => "Auto",
    }
}

fn selected_runtime_label(
    model: &app_core::RuntimeModelPresentation,
    selected: Option<&str>,
) -> String {
    if let Some(selected) = selected
        && let Some(capability) = model
            .backends
            .iter()
            .find(|capability| backend_value(capability.backend) == selected)
    {
        return backend_label(capability.backend).to_string();
    }
    format!(
        "Default · {}",
        model
            .selected_backend
            .map(backend_label)
            .unwrap_or("Unresolved")
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_model_runtime_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    config: &AppConfig,
    model: &app_core::RuntimeModelPresentation,
    runtime_open: bool,
) {
    let selected_backend = config
        .model_backend_overrides
        .get(&model.model_id)
        .map(String::as_str);
    let selected_device = config
        .model_device_overrides
        .get(&model.model_id)
        .map(String::as_str);
    let capabilities = model
        .backends
        .iter()
        .filter(|capability| {
            capability.validation != app_core::RuntimeValidationPresentation::Unsupported
        })
        .map(|capability| {
            format!(
                "{} · {}",
                backend_label(capability.backend),
                validation_label(capability.validation)
            )
        })
        .collect::<Vec<_>>()
        .join("  /  ");

    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                min_height: px(116),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(
                    px(SETTINGS_ROW_HORIZONTAL_PADDING),
                    px(SETTINGS_ROW_VERTICAL_PADDING),
                ),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
            ZIndex(if runtime_open { 60 } else { 0 }),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(SETTINGS_COPY_MIN_WIDTH),
                flex_basis: px(SETTINGS_COPY_BASIS),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(5),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(
                    copy,
                    font.clone(),
                    model_backend_display_name(&model.model_id),
                    11.5,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!("Model ID · {}", model.model_id),
                    8.8,
                    theme.muted_foreground,
                );
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!("Available runtimes · {capabilities}"),
                    8.8,
                    theme.muted_foreground,
                );
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    device_preference_caption(model.selected_backend),
                    8.0,
                    theme.muted_foreground.with_alpha(0.78),
                );
            });

            row.spawn(Node {
                position_type: PositionType::Relative,
                min_width: px(280),
                max_width: px(SETTINGS_WIDE_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_WIDE_CONTROL_WIDTH),
                flex_grow: 0.0,
                margin: UiRect::top(px(1)),
                flex_direction: FlexDirection::Column,
                row_gap: px(7),
                ..default()
            })
            .with_children(|controls| {
                controls
                    .spawn(Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|label_row| {
                        spawn_text(
                            label_row,
                            font.clone(),
                            "DEVICE",
                            8.0,
                            theme.muted_foreground,
                        );
                        spawn_text(
                            label_row,
                            font.clone(),
                            selected_device_label(selected_device),
                            8.0,
                            theme.primary,
                        );
                    });
                controls
                    .spawn(Node {
                        width: percent(100),
                        height: px(32),
                        column_gap: px(4),
                        ..default()
                    })
                    .with_children(|devices| {
                        for (value, label) in DEVICE_CLASS_OPTIONS {
                            let active = selected_device == Some(value);
                            devices
                                .spawn((
                                    Button,
                                    UiAction::from(SettingsCommand::SetModelDevice(
                                        model.model_id.clone(),
                                        (!active).then(|| value.to_string()),
                                    )),
                                    Node {
                                        min_width: px(0),
                                        height: percent(100),
                                        flex_grow: 1.0,
                                        align_items: AlignItems::Center,
                                        justify_content: JustifyContent::Center,
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(5)),
                                        ..default()
                                    },
                                    BackgroundColor(if active {
                                        theme.primary.with_alpha(0.14)
                                    } else {
                                        theme.background.with_alpha(0.38)
                                    }),
                                    BorderColor::all(if active {
                                        theme.primary.with_alpha(0.68)
                                    } else {
                                        theme.border.with_alpha(0.48)
                                    }),
                                ))
                                .with_children(|button| {
                                    spawn_text(
                                        button,
                                        font.clone(),
                                        if value == "integrated_gpu" {
                                            "iGPU"
                                        } else {
                                            label
                                        },
                                        9.0,
                                        if active {
                                            theme.primary
                                        } else {
                                            theme.foreground
                                        },
                                    );
                                });
                        }
                    });

                controls
                    .spawn(Node {
                        position_type: PositionType::Relative,
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    })
                    .with_children(|runtime| {
                        runtime
                            .spawn((
                                Button,
                                UiAction::from(SettingsCommand::ToggleModelRuntimeSelect(
                                    model.model_id.clone(),
                                )),
                                Node {
                                    width: percent(100),
                                    height: px(36),
                                    align_items: AlignItems::Center,
                                    padding: UiRect::horizontal(px(11)),
                                    column_gap: px(8),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(6)),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(if runtime_open {
                                    0.74
                                } else {
                                    0.42
                                })),
                                BorderColor::all(if runtime_open {
                                    theme.primary.with_alpha(0.68)
                                } else {
                                    theme.border.with_alpha(0.54)
                                }),
                            ))
                            .with_children(|button| {
                                spawn_text(
                                    button,
                                    font.clone(),
                                    "RUNTIME",
                                    8.0,
                                    theme.muted_foreground,
                                );
                                button.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                spawn_text(
                                    button,
                                    font.clone(),
                                    selected_runtime_label(model, selected_backend),
                                    9.0,
                                    theme.foreground,
                                );
                                spawn_icon(
                                    button,
                                    icons.clone(),
                                    UiIcon::ChevronDown,
                                    13.0,
                                    theme.muted_foreground,
                                );
                            });
                        if runtime_open {
                            spawn_runtime_options(
                                runtime,
                                font.clone(),
                                icons.clone(),
                                theme,
                                model,
                                selected_backend,
                            );
                        }
                    });
            });
        });
}

fn spawn_runtime_options(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    model: &app_core::RuntimeModelPresentation,
    selected: Option<&str>,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(5)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border.with_alpha(0.9)),
            ZIndex(61),
        ))
        .with_children(|menu| {
            let default = model
                .selected_backend
                .map(backend_label)
                .unwrap_or("Unresolved");
            spawn_runtime_option(
                menu,
                font.clone(),
                icons.clone(),
                theme,
                format!("Default · {default}"),
                selected.is_none(),
                UiAction::from(SettingsCommand::SetModelBackend(
                    model.model_id.clone(),
                    None,
                )),
            );
            for capability in model.backends.iter().filter(|capability| {
                capability.validation != app_core::RuntimeValidationPresentation::Unsupported
            }) {
                let value = backend_value(capability.backend);
                spawn_runtime_option(
                    menu,
                    font.clone(),
                    icons.clone(),
                    theme,
                    format!(
                        "{} · {}",
                        backend_label(capability.backend),
                        validation_label(capability.validation)
                    ),
                    selected == Some(value),
                    UiAction::from(SettingsCommand::SetModelBackend(
                        model.model_id.clone(),
                        Some(value.to_string()),
                    )),
                );
            }
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_runtime_option(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    label: String,
    selected: bool,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100),
                min_height: px(31),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(9), px(7)),
                column_gap: px(7),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(if selected {
                theme.primary.with_alpha(0.12)
            } else {
                Color::NONE
            }),
        ))
        .with_children(|option| {
            spawn_wrapped_text(
                option,
                font,
                label,
                9.0,
                if selected {
                    theme.primary
                } else {
                    theme.foreground
                },
            );
            option.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if selected {
                spawn_icon(option, icons, UiIcon::Check, 13.0, theme.primary);
            }
        });
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

#[cfg(test)]
mod tests {
    use super::model_install_role;

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
}
