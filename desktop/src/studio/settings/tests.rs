#[test]
fn primary_settings_pages_use_shared_contained_groups() {
    let general = include_str!("general.rs");
    let storage = include_str!("storage.rs");
    let models = include_str!("models.rs");
    let analysis = include_str!("analysis.rs");

    assert_eq!(general.matches("spawn_settings_group(").count(), 3);
    assert_eq!(storage.matches("spawn_settings_group(").count(), 3);
    assert_eq!(models.matches("spawn_settings_group(").count(), 2);
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
