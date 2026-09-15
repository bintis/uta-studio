#[test]
fn super_acceleration_switch_saves_before_updating_visible_state() {
    let mut config = app_core::AppConfig::default();
    let result = super::rows::toggle_turbo_acceleration(&mut config, |_| {
        Err("isolated save failure".to_string())
    });
    assert!(result.is_err());
    assert_eq!(config.turbo_acceleration, None);
    super::rows::toggle_turbo_acceleration(&mut config, |proposed| {
        assert_eq!(proposed.turbo_acceleration, Some(true));
        Ok(())
    })
    .unwrap();
    assert_eq!(config.turbo_acceleration, Some(true));
    super::rows::toggle_turbo_acceleration(&mut config, |_| Ok(())).unwrap();
    assert_eq!(config.turbo_acceleration, Some(false));
    assert!(include_str!("models.rs").contains("SettingsCommand::ToggleTurboAcceleration"));
    assert!(!include_str!("analysis.rs").contains("SettingsCommand::ToggleTurboAcceleration"));
}

#[test]
fn routing_changes_save_before_updating_visible_state() {
    let mut config = app_core::AppConfig::default();
    let result = super::rows::save_config_change(
        &mut config,
        |proposed| proposed.compute_backend = Some("libtorch_xpu".to_string()),
        |_| Err("isolated save failure".to_string()),
    );
    assert!(result.is_err());
    assert_eq!(config.compute_backend, None);

    super::rows::save_config_change(
        &mut config,
        |proposed| {
            proposed
                .model_device_overrides
                .insert("rmvpe".to_string(), "integrated_gpu".to_string());
        },
        |_| Ok(()),
    )
    .unwrap();
    assert_eq!(
        config
            .model_device_overrides
            .get("rmvpe")
            .map(String::as_str),
        Some("integrated_gpu")
    );
}

#[test]
fn super_acceleration_visibly_disables_manual_route_controls() {
    let models = include_str!("models.rs");
    let actions = include_str!("../actions_settings.rs");
    assert!(models.contains("Automatic scheduler"));
    assert!(models.contains("if !automatic_routing"));
    assert!(actions.contains("Super acceleration owns runtime routing"));
    assert!(actions.contains("Super acceleration owns per-model routing"));
}

#[test]
fn primary_settings_pages_use_shared_contained_groups() {
    let general = include_str!("general.rs");
    let storage = include_str!("storage.rs");
    let models = include_str!("models.rs");
    let analysis = include_str!("analysis.rs");

    assert_eq!(general.matches("spawn_settings_group(").count(), 3);
    assert_eq!(storage.matches("spawn_settings_group(").count(), 4);
    assert_eq!(models.matches("spawn_settings_group(").count(), 3);
    assert_eq!(analysis.matches("spawn_settings_stage_group(").count(), 6);
    assert!(!analysis.contains("spawn_settings_stage_header("));
}

#[test]
fn settings_information_architecture_keeps_ownership_boundaries_explicit() {
    let analysis = include_str!("analysis.rs");
    let models = include_str!("models.rs");

    for required in [
        "Processing Studio owns per-song topology",
        "Models & runtime owns resources",
        "exact readiness remains visible in Plan Preview",
        "Per-song topology is configured in Processing Studio",
        "Plan Preview is authoritative",
    ] {
        assert!(
            analysis.contains(required),
            "missing ownership copy: {required}"
        );
    }
    for required in [
        "These controls do not select workflow outputs",
        "these controls never choose analysis outputs or change workflow topology",
        "Lifecycle actions remain explicit",
    ] {
        assert!(
            models.contains(required),
            "missing lifecycle boundary copy: {required}"
        );
    }
}

#[test]
fn settings_rows_share_one_right_hand_control_column() {
    let rows = include_str!("rows.rs");
    let analysis = include_str!("analysis.rs");
    let models = include_str!("models.rs");

    assert!(rows.matches("SETTINGS_CONTROL_WIDTH").count() >= 4);
    assert!(rows.matches("SETTINGS_COPY_BASIS").count() >= 3);
    assert!(analysis.matches("SETTINGS_CONTROL_WIDTH").count() >= 3);
    assert!(analysis.matches("SETTINGS_COPY_BASIS").count() >= 3);
    assert!(models.contains("SETTINGS_WIDE_CONTROL_WIDTH"));
}

#[test]
fn compute_backend_offers_both_backends_and_custom_per_model_routing() {
    use crate::studio::SettingsSelectKind;
    assert_eq!(
        super::labels::settings_select_options(SettingsSelectKind::ComputeBackend)
            .iter()
            .map(|(value, _)| *value)
            .collect::<Vec<_>>(),
        ["ggml", "libtorch_xpu", "custom"]
    );
    let config = app_core::AppConfig::default();
    assert_eq!(
        super::labels::settings_select_value(SettingsSelectKind::ComputeBackend, &config),
        "ggml"
    );
    let models = include_str!("models.rs");
    assert!(models.contains("if custom_routing && let Some(snapshot)"));
    assert!(!models.contains("Default · "));
}
