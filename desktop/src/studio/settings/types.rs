//! Settings route: general, storage, models & runtime, and analysis.

use std::time::Instant;

use crate::studio::*;

pub(crate) const SETTINGS_CONTROL_WIDTH: f32 = 230.0;

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
    ComputeBackend,
    Separator,
    SeparatorPreset,
    AsrEngine,
    WhisperModel,
    AlignBackend,
    PitchModel,
    AudioVocalModel,
    AudioAccompanimentModel,
    AudioKaraokeModel,
    AudioDenoise,
    AudioDereverb,
    AudioCleanupOrder,
    AudioTorchBackend,
    AudioOnnxBackend,
    AudioPrecisionPolicy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AnalysisAdvancedSection {
    Separation,
    Transcription,
    Pitch,
}

#[derive(Clone, Copy)]
pub(crate) struct SetupRequest {
    pub(crate) target: Option<app_core::ModelDownloadTarget>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CacheClearScope {
    Generated,
    Models,
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

#[derive(Component, Clone, Copy)]
pub(crate) enum NumericSetting {
    SeparatorSegmentSize,
    SeparatorOverlap,
    SeparatorBatchSize,
    SeparatorNormalization,
    DemucsShifts,
    DemucsOverlap,
    BeamSize,
    BatchSize,
    VocalThreshold,
}
