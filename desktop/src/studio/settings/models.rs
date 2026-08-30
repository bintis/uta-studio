use super::*;
use crate::studio::*;

pub(crate) fn spawn_model_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    _icons: Handle<Image>,
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

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "PER-MODEL RUNTIME PARAMETERS",
        "Each row belongs to one concrete model. Leaving a backend on Default keeps the Runtime Manager-pinned route; an explicit value changes only that model's runtime parameter.",
    );

    if let Some(snapshot) = session.model_settings_job.current.as_ref() {
        spawn_model_backend_settings(
            parent,
            font.clone(),
            theme,
            session.config,
            &snapshot.runtime_models,
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
            parent,
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

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "FUSION AGENT ADAPTER",
        "Runtime Manager automatically scans PATH for Uta Fusion Agent Adapters with a protocol-compatible sidecar manifest. A plain Pi, Codex, Claude, Gemini, or other coding-agent CLI cannot be selected directly because it does not implement Uta's bounded fusion protocol. An adapter may use one of those agents internally, may contact its external provider, and runs with your OS permissions.",
    );
    let adapter = session
        .model_settings_job
        .current
        .as_ref()
        .and_then(|snapshot| snapshot.fusion_agent_adapter.as_ref());
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
            "Status: {state} · {identity} · {version}{reasons}. Candidate metadata may be sent to the adapter's external AI provider only when AI judgment is selected."
        )
    } else {
        session
            .model_settings_job
            .current
            .as_ref()
            .and_then(|snapshot| snapshot.fusion_agent_adapter_error.as_deref())
            .map_or_else(
                || "Status is loading. AI judgment remains unavailable until Runtime Manager reports the adapter usable.".to_string(),
                |error| format!("Could not read adapter status: {error}"),
            )
    };
    spawn_setting_row_with_actions(
        parent,
        font.clone(),
        theme,
        "Fusion Agent Adapter",
        adapter_description,
        vec![
            (
                "Scan again".to_string(),
                UiAction::from(SettingsCommand::RefreshRuntimeStatus),
            ),
            (
                "Choose compatible adapter…".to_string(),
                UiAction::from(SettingsCommand::ChooseFusionAgentAdapter),
            ),
        ],
    );
    if adapter.is_some_and(|status| {
        matches!(
            status.origin,
            app_core::ResourceOriginWireV1::ExternalConfiguration
        )
    }) {
        spawn_setting_row(
            parent,
            font,
            theme,
            "Clear Fusion Agent Adapter",
            "Clears Runtime Manager's configured external-tool path. AI workflows then fail closed; Algorithm workflows are unaffected.",
            Some((
                "Clear",
                UiAction::from(SettingsCommand::ClearFusionAgentAdapter),
            )),
        );
    }
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
    theme: &StudioTheme,
    config: &AppConfig,
    registry: &[app_core::RuntimeModelPresentation],
) {
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Runtime and device",
        "Runtime is recommended on Default. Device is captured as a preference for upcoming multi-device routing and does not yet change which physical device Runtime Manager selects. Neither changes requested outputs, workflow topology, or provider selection.",
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
            .selected_backend
            .map(backend_label)
            .unwrap_or("unresolved");
        let capabilities = model
            .backends
            .iter()
            .filter(|capability| {
                capability.validation != app_core::RuntimeValidationPresentation::Unsupported
            })
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
                    capability.validation != app_core::RuntimeValidationPresentation::Unsupported
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
            format!("{} · Runtime", model_backend_display_name(&model.model_id)),
            format!(
                "Model ID: {} · Available runtimes: {}",
                model.model_id, capabilities
            ),
            actions,
        );

        let selected_device = config.model_device_overrides.get(&model.model_id);
        let mut device_actions = vec![(
            if selected_device.is_none() {
                "✓ Default".to_string()
            } else {
                "Default".to_string()
            },
            UiAction::from(SettingsCommand::SetModelDevice(
                model.model_id.clone(),
                None,
            )),
        )];
        device_actions.extend(DEVICE_CLASS_OPTIONS.iter().map(|(value, label)| {
            (
                if selected_device.is_some_and(|selected| selected == value) {
                    format!("✓ {label}")
                } else {
                    label.to_string()
                },
                UiAction::from(SettingsCommand::SetModelDevice(
                    model.model_id.clone(),
                    Some((*value).to_string()),
                )),
            )
        }));
        spawn_setting_row_with_actions(
            parent,
            font.clone(),
            theme,
            format!("{} · Device", model_backend_display_name(&model.model_id)),
            "Preferred device class for this model. Recorded for upcoming multi-device routing.",
            device_actions,
        );
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
