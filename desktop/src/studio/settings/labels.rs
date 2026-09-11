use super::*;
use crate::studio::*;

pub(crate) fn readiness_reason_label(reason: &app_core::ReadinessReasonWire) -> &'static str {
    match reason {
        app_core::ReadinessReasonWire::UnknownResource => "Unknown resource",
        app_core::ReadinessReasonWire::Absent => "Not configured",
        app_core::ReadinessReasonWire::Incomplete => "Configuration incomplete",
        app_core::ReadinessReasonWire::Corrupt => "Configuration corrupt",
        app_core::ReadinessReasonWire::Legacy => "Legacy configuration",
        app_core::ReadinessReasonWire::DependencyMissing => "Dependency missing",
        app_core::ReadinessReasonWire::RuntimeMissing => "Runtime missing",
        app_core::ReadinessReasonWire::ExecutableMissing => "Executable missing",
        app_core::ReadinessReasonWire::WorkerCapabilityMissing => "Capability missing",
        app_core::ReadinessReasonWire::ProtocolMismatch => "Protocol mismatch",
        app_core::ReadinessReasonWire::BackendUnvalidated => "Backend unvalidated",
        app_core::ReadinessReasonWire::CpuProductionForbidden => "CPU production forbidden",
        app_core::ReadinessReasonWire::UnsupportedPlatform => "Unsupported platform",
        app_core::ReadinessReasonWire::NativeLibraryMissing => "Native library not installed",
    }
}

pub(crate) fn settings_select_value(kind: SettingsSelectKind, config: &AppConfig) -> &str {
    match kind {
        SettingsSelectKind::UiLanguage => config.ui_language(),
        SettingsSelectKind::AnalysisTarget => config.analysis_default_target().as_str(),
        SettingsSelectKind::ComputeBackend => config.compute_backend.as_deref().unwrap_or("auto"),
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
        SettingsSelectKind::ComputeBackend => match value {
            "ggml" | "ggml_vulkan" | "vulkan" => "GGML Vulkan",
            "libtorch_xpu" => "LibTorch XPU",
            _ => "Pinned default (GGML Vulkan)",
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
        SettingsSelectKind::ComputeBackend => &[
            ("auto", "Pinned default (GGML Vulkan)"),
            ("ggml", "GGML Vulkan"),
            ("libtorch_xpu", "LibTorch XPU"),
        ],
    }
}
