use super::*;
use crate::studio::*;

pub(crate) fn readiness_reason_label(reason: &app_core::ReadinessReasonWireV1) -> &'static str {
    match reason {
        app_core::ReadinessReasonWireV1::UnknownResource => "Unknown resource",
        app_core::ReadinessReasonWireV1::Absent => "Not configured",
        app_core::ReadinessReasonWireV1::Incomplete => "Configuration incomplete",
        app_core::ReadinessReasonWireV1::Corrupt => "Configuration corrupt",
        app_core::ReadinessReasonWireV1::Legacy => "Legacy configuration",
        app_core::ReadinessReasonWireV1::DependencyMissing => "Dependency missing",
        app_core::ReadinessReasonWireV1::RuntimeMissing => "Runtime missing",
        app_core::ReadinessReasonWireV1::ExecutableMissing => "Executable missing",
        app_core::ReadinessReasonWireV1::WorkerCapabilityMissing => "Capability missing",
        app_core::ReadinessReasonWireV1::ProtocolMismatch => "Protocol mismatch",
        app_core::ReadinessReasonWireV1::BackendUnvalidated => "Backend unvalidated",
        app_core::ReadinessReasonWireV1::CpuProductionForbidden => "CPU production forbidden",
        app_core::ReadinessReasonWireV1::UnsupportedPlatform => "Unsupported platform",
    }
}

pub(crate) fn settings_select_value(kind: SettingsSelectKind, config: &AppConfig) -> &str {
    match kind {
        SettingsSelectKind::UiLanguage => config.ui_language(),
        SettingsSelectKind::AnalysisTarget => config.analysis_default_target().as_str(),
    }
}

pub(crate) fn settings_select_label(kind: SettingsSelectKind, value: &str) -> &'static str {
    match kind {
        SettingsSelectKind::UiLanguage => match value {
            "en" => "English",
            "zh-CN" => "简体中文",
            "ja" => "日本語",
            _ => "System default",
        },
        SettingsSelectKind::AnalysisTarget => match value {
            "transcript" => "Transcript",
            "alignment" => "Alignment",
            "pitch_evidence" => "Pitch evidence",
            "instrumental" => "Instrumental",
            _ => "Full candidate chart",
        },
    }
}

pub(crate) fn settings_select_options(
    kind: SettingsSelectKind,
) -> &'static [(&'static str, &'static str)] {
    match kind {
        SettingsSelectKind::UiLanguage => &[
            ("system", "System default"),
            ("en", "English"),
            ("zh-CN", "简体中文"),
            ("ja", "日本語"),
        ],
        SettingsSelectKind::AnalysisTarget => &[
            ("full_candidate", "Full candidate chart"),
            ("transcript", "Transcript"),
            ("alignment", "Alignment"),
            ("pitch_evidence", "Pitch evidence"),
            ("instrumental", "Instrumental"),
        ],
    }
}
