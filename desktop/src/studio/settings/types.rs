//! Settings route: general, storage, models & runtime, and analysis.

use std::time::Instant;

use crate::studio::*;

pub(crate) const SETTINGS_CONTROL_WIDTH: f32 = 230.0;
pub(crate) const SETTINGS_CONTENT_HORIZONTAL_PADDING: f32 = 40.0;
pub(crate) const SETTINGS_CONTENT_VERTICAL_PADDING: f32 = 24.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SettingsTab {
    #[default]
    General,
    Storage,
    Models,
    Analysis,
}

impl SettingsTab {
    pub(crate) fn index(self) -> usize {
        match self {
            Self::General => 0,
            Self::Storage => 1,
            Self::Models => 2,
            Self::Analysis => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsSelectKind {
    UiLanguage,
    AnalysisTarget,
}

#[derive(Clone, Copy)]
pub(crate) struct SetupRequest {
    pub(crate) target: Option<app_core::ModelDownloadTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheClearScope {
    Generated,
}

#[derive(Resource, Default)]
pub(crate) struct NativeSetup {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<SetupEvent>>>,
    pub(crate) progress: Option<app_core::SetupProgress>,
    pub(crate) logs: Vec<String>,
    /// Last time the setup panel was rebuilt. Progress and log lines arrive
    /// faster than a full settings-page rebuild can keep up, so intermediate
    /// ticks are coalesced instead of invalidating on every line.
    pub(crate) last_ui_refresh: Option<Instant>,
}

pub(crate) enum SetupEvent {
    Progress(app_core::SetupProgress),
    Log(String),
    Complete(Result<(), String>),
}

#[derive(Resource, Default)]
pub(crate) struct NativeDiagnostics {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<uta_studio_diagnostics::DiagnosticReport>>>,
}

#[derive(Resource, Default)]
pub(crate) struct CacheStatsJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<app_core::CacheStats>>>,
    pub(crate) current: Option<app_core::CacheStats>,
    pub(crate) error: Option<String>,
}

#[derive(Component)]
pub(crate) struct SettingsContent;

#[derive(Component)]
pub(crate) struct SettingsPageContent;
