use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use app_core::{
    AppConfig, LibraryFolderEntry, LibraryMenuFilters, LibrarySource, LoadSongsParams, Song,
    SongsStore,
};
use bevy::{
    asset::RenderAssetUsages,
    color::Mix,
    ecs::system::SystemParam,
    image::{CompressedImageFormats, ImageSampler, ImageType},
    input_focus::{
        AutoFocus, FocusCause, InputFocus, InputFocusVisible,
        tab_navigation::{NavAction, TabGroup, TabIndex, TabNavigation, TabNavigationPlugin},
    },
    log::{DEFAULT_FILTER, LogPlugin},
    prelude::*,
    text::{EditableText, TextCursorStyle},
    window::{EnabledButtons, MonitorSelection, PrimaryWindow, WindowMode, WindowTheme},
};

use crate::theme::StudioTheme;
mod actions;
mod actions_chrome;
mod actions_content;
mod actions_settings;
mod analysis;
mod analysis_layout;
mod analysis_model;
mod artifact_workbench_ui;
mod chrome;
mod commands;
mod documentation;
mod editor;
mod folders;
mod i18n;
mod library;
mod navigation;
mod processing_studio;
mod session;
mod settings;
mod song_detail;
mod song_settings;
mod startup;
mod state;
mod ui_api;
mod ui_invalidation;
mod widgets;
mod window_ops;

use self::analysis::*;

pub use startup::run;
pub(crate) use ui_api::{UiPointerApi, audit_ui_api_coverage};
pub(crate) use ui_invalidation::{UiDirtyRegion, UiInvalidated, UiRebuildMetrics};

pub(crate) use actions::*;
pub(crate) use actions_chrome::*;
pub(crate) use actions_content::*;
pub(crate) use actions_settings::*;
pub(crate) use analysis_layout::*;
pub(crate) use analysis_model::*;
pub(crate) use artifact_workbench_ui::*;
pub(crate) use chrome::*;
pub(crate) use commands::{
    AnalysisCommand, AnalysisModelCategory, AppCommand, EditorCommand, LibraryCommand,
    SettingsCommand, UiAction, UiCommand,
};
pub(crate) use documentation::*;
pub(crate) use editor::*;
pub(crate) use folders::*;
pub(crate) use i18n::*;
pub(crate) use library::*;
pub(crate) use navigation::*;
pub(crate) use processing_studio::*;
pub(crate) use session::*;
pub(crate) use settings::*;
pub(crate) use song_detail::*;
pub(crate) use song_settings::*;
pub(crate) use state::{
    AnalysisUiState, AsyncJobs, DialogState, EditorUiState, LibraryState, ModelSettingsJob,
    ModelSettingsSnapshot, PlaybackState, ShellState, StudioSessionView, StudioStateBundle,
    StudioStateMut, StudioStateRead,
};
pub(crate) use widgets::*;
pub(crate) use window_ops::*;

#[cfg(test)]
include!("studio_tests.rs");
