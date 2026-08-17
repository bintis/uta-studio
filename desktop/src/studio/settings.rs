//! Settings route: general, storage, models & runtime, and analysis.

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

pub(crate) fn spawn_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    native_setup: &NativeSetup,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_width: px(0),
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|settings| {
            settings
                .spawn((
                    Node {
                        width: px(224),
                        height: percent(100),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(24), px(28)),
                        row_gap: px(4),
                        border: UiRect::right(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.38)),
                    BorderColor::all(theme.border.with_alpha(0.26)),
                ))
                .with_children(|nav| {
                    spawn_text(nav, font.clone(), "UTA STUDIO", 8.0, theme.primary);
                    spawn_text(nav, font.clone(), "Settings", 20.0, theme.foreground);
                    spawn_wrapped_text(
                        nav,
                        font.clone(),
                        "Workspace, library, and generation.",
                        10.0,
                        theme.muted_foreground,
                    );
                    nav.spawn(Node {
                        height: px(18),
                        ..default()
                    });
                    for (tab, icon, label) in [
                        (SettingsTab::General, UiIcon::Monitor, "General"),
                        (SettingsTab::Storage, UiIcon::Database, "Storage"),
                        (SettingsTab::Models, UiIcon::Box, "Models & runtime"),
                        (SettingsTab::Analysis, UiIcon::Sparkles, "Analysis"),
                    ] {
                        spawn_settings_tab(
                            nav,
                            font.clone(),
                            icons.clone(),
                            theme,
                            tab,
                            icon,
                            label,
                            session.settings_tab == tab,
                        );
                    }
                    nav.spawn(Node {
                        min_height: px(4),
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text_button(
                        nav,
                        font.clone(),
                        theme,
                        "About Uta! Studio",
                        10.0,
                        UiAction::OpenAbout,
                    );
                });

            settings
                .spawn((
                    SettingsContent,
                    ScrollPosition(Vec2::new(
                        0.0,
                        session.settings_scroll_offsets[session.settings_tab.index()],
                    )),
                    Node {
                        min_width: px(0),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::axes(px(40), px(34)),
                        overflow: Overflow::scroll_y(),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.54)),
                ))
                .with_children(|content| {
                    // A scrollable flex column otherwise shrinks its direct
                    // children to the viewport height before measuring overflow.
                    // Keep one intrinsic-height page so setting rows retain their
                    // intended height and the content scrolls instead of stacking.
                    content
                        .spawn(Node {
                            width: percent(100),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|page| {
                            match session.settings_tab {
                                SettingsTab::General => spawn_general_settings(
                                    page,
                                    font.clone(),
                                    icons.clone(),
                                    session,
                                    theme,
                                ),
                                SettingsTab::Storage => spawn_storage_settings(
                                    page,
                                    font.clone(),
                                    session,
                                    &cache_stats,
                                    theme,
                                ),
                                SettingsTab::Models => spawn_model_settings(
                                    page,
                                    font.clone(),
                                    icons.clone(),
                                    session,
                                    native_setup,
                                    theme,
                                ),
                                SettingsTab::Analysis => spawn_analysis_settings(
                                    page,
                                    font.clone(),
                                    icons.clone(),
                                    session,
                                    theme,
                                ),
                            }
                            if let Some(notice) = session.notice.as_deref() {
                                page.spawn(Node {
                                    height: px(14),
                                    ..default()
                                });
                                spawn_wrapped_text(
                                    page,
                                    font.clone(),
                                    notice,
                                    10.0,
                                    theme.muted_foreground,
                                );
                            }
                        });
                });
            if let Some(request) = session.pending_setup {
                spawn_setup_confirmation(settings, font.clone(), theme, request);
            }
            if let Some(scope) = session.pending_cache_clear {
                spawn_global_cache_confirmation(settings, font.clone(), theme, scope);
            }
            if let Some(path) = session.folder_browser.pending_remove.as_deref() {
                spawn_remove_folder_confirmation(settings, font, theme, path);
            }
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_settings_tab(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    tab: SettingsTab,
    icon: UiIcon,
    label: &'static str,
    active: bool,
) {
    parent
        .spawn((
            Button,
            UiAction::SettingsTab(tab),
            Node {
                width: percent(100),
                height: px(36),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12)),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(Color::NONE),
            BorderColor::all(if active { theme.primary } else { Color::NONE }),
        ))
        .with_children(|row| {
            let color = if active {
                theme.primary
            } else {
                theme.muted_foreground
            };
            spawn_icon(row, icons, icon, 15.0, color);
            row.spawn(Node {
                width: px(9),
                ..default()
            });
            spawn_text(
                row,
                font,
                label,
                11.0,
                if active {
                    theme.foreground.with_alpha(0.78)
                } else {
                    theme.muted_foreground
                },
            );
        });
}

pub(crate) fn spawn_settings_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
    description: &'static str,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::bottom(px(20)),
                margin: UiRect::bottom(px(6)),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|header| {
            spawn_text(header, font.clone(), eyebrow, 8.0, theme.primary);
            spawn_text(header, font.clone(), title, 20.0, theme.foreground);
            spawn_wrapped_text(header, font, description, 10.0, theme.muted_foreground);
        });
}

pub(crate) fn spawn_general_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "WORKSPACE",
        "General",
        "Window behavior and diagnostic tools.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Dark mode",
        "Enable a dark palette across the application.",
        session.config.dark_mode.unwrap_or(false),
        UiAction::ToggleTheme,
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Fullscreen workspace",
        if session.config.fullscreen.unwrap_or(false) {
            "The editor fills this display."
        } else {
            "The app uses a standard window."
        },
        session.config.fullscreen.unwrap_or(false),
        UiAction::ToggleFullscreen,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons,
        theme,
        "Interface language",
        "Choose the language used by Uta Studio. System default follows the locale provided by your operating environment.",
        SettingsSelectKind::UiLanguage,
        session,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Application log",
        "Review recent events when analysis, editing, or export needs troubleshooting.",
        Some(("View log", UiAction::OpenLog)),
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Feature API diagnostics",
        "Verify local APIs, native audio, and real UTZ/UltraStar exports in a unique temporary folder that is always removed.",
        Some(("Run checks", UiAction::RunDiagnostics)),
    );
    spawn_shift_setting_row(
        parent,
        font.clone(),
        theme,
        "Font size",
        "Set the base UI font size. The interface is scaled using this size (10px–18px), which maps to 80%–140%.",
        format!(
            "{}px",
            ui_font_size_percent_to_points(session.config.font_scale_percent())
        ),
        UiAction::AdjustUiFontScale(-1),
        UiAction::AdjustUiFontScale(1),
    );
    if let Some(report) = session.diagnostic_report.as_ref() {
        spawn_diagnostics_report(parent, font.clone(), theme, report);
    }
}

pub(crate) fn spawn_diagnostics_report(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    report: &uta_studio_diagnostics::DiagnosticReport,
) {
    let status_text = if report.ok {
        "Passed"
    } else {
        "Needs attention"
    };
    let status_color = if report.ok {
        theme.primary
    } else {
        theme.destructive
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(132),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(14)),
                flex_direction: FlexDirection::Column,
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.42)),
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|summary| {
                    spawn_text(
                        summary,
                        font.clone(),
                        "Diagnostic results",
                        10.0,
                        theme.muted_foreground,
                    );
                    summary.spawn((
                        Node {
                            padding: UiRect::axes(px(9), px(3)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(status_color.with_alpha(0.16)),
                        BorderColor::all(status_color.with_alpha(0.45)),
                        children![(
                            Text::new(status_text),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(status_color),
                        )],
                    ));
                });
            spawn_text(
                panel,
                font.clone(),
                format!(
                    "{} passed · {} failed · {} skipped · {} APIs",
                    report.passed, report.failed, report.skipped, report.capabilities
                ),
                10.0,
                theme.foreground,
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|checks| {
                    for check in &report.checks {
                        spawn_diagnostic_check_row(checks, font.clone(), theme, check);
                    }
                });
        });
}

pub(crate) fn spawn_diagnostic_check_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    check: &uta_studio_diagnostics::DiagnosticCheck,
) {
    let status_color = diagnostic_status_color(check.status, theme);
    let status_label = diagnostic_status_label(check.status);
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(7),
            padding: UiRect::bottom(px(10)),
            border: UiRect::bottom(px(1)),
            margin: UiRect::bottom(px(4)),
            ..default()
        })
        .with_children(|row| {
            row.spawn(Node {
                width: percent(100),
                align_items: AlignItems::FlexStart,
                justify_content: JustifyContent::SpaceBetween,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(10),
                ..default()
            })
            .with_children(|heading| {
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|labels| {
                        labels
                            .spawn(Node {
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|id_line| {
                                spawn_text(id_line, font.clone(), check.id, 9.0, theme.foreground);
                                id_line.spawn((
                                    Node {
                                        padding: UiRect::axes(px(6), px(2)),
                                        border_radius: BorderRadius::all(px(999.0)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted.with_alpha(0.16)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                    children![(
                                        Text::new(status_label),
                                        ui_text_font(font.clone(), 8.0),
                                        TextColor(status_color),
                                    )],
                                ));
                            });
                        spawn_text(
                            labels,
                            font.clone(),
                            format!("{}ms", check.elapsed_ms),
                            8.0,
                            theme.muted_foreground,
                        );
                    });
                spawn_text(heading, font.clone(), check.status, 8.0, status_color);
            });
            row.spawn(Node {
                width: percent(100),
                min_width: px(0),
                ..default()
            })
            .with_children(|details| {
                spawn_wrapped_text(
                    details,
                    font.clone(),
                    format!("{} • {}", status_label, check.detail),
                    8.8,
                    theme.muted_foreground,
                );
            });
        });
}

pub(crate) fn diagnostic_status_color<'a>(status: &str, theme: &'a StudioTheme) -> Color {
    match status {
        "passed" => theme.primary,
        "failed" => theme.destructive,
        _ => theme.muted_foreground,
    }
}

pub(crate) fn diagnostic_status_label(status: &str) -> &'static str {
    match status {
        "passed" => "OK",
        "failed" => "FAIL",
        _ => "SKIP",
    }
}

pub(crate) fn spawn_storage_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    cache_stats: &CacheStatsJob,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "LIBRARY",
        "Storage",
        "Manage watched folders and generated data. Your source media is never moved or deleted.",
    );
    spawn_watched_folders_setting(parent, font.clone(), session, theme);
    let export_path = session
        .config
        .export_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|| "Use the last folder chosen by the system dialog".to_string());
    spawn_setting_row_with_actions(
        parent,
        font.clone(),
        theme,
        "Default export folder",
        format!(
            "Every format opens Save As here first. You can still choose another folder for each export.\n\n{export_path}"
        ),
        vec![
            ("Choose…".to_string(), UiAction::ChooseExportFolder),
            (
                "Use system default".to_string(),
                UiAction::ClearExportFolder,
            ),
        ],
    );
    spawn_storage_usage_row(parent, font.clone(), theme, cache_stats);
}

pub(crate) fn spawn_storage_usage_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    cache_stats: &CacheStatsJob,
) {
    let (status, status_color, status_summary) =
        match (cache_stats.current.as_ref(), cache_stats.receiver.is_some()) {
            (Some(stats), false) => {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                (
                    "Current",
                    theme.foreground,
                    format!("Latest scan: {}", format_bytes(total)),
                )
            }
            (Some(stats), true) => {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                (
                    "Recalculating",
                    theme.primary,
                    format!(
                        "Recalculating in background. Latest scan: {}",
                        format_bytes(total)
                    ),
                )
            }
            (None, true) => (
                "Calculating",
                theme.primary,
                "Calculating generated storage usage. This may scan configured cache folders."
                    .to_string(),
            ),
            (None, false) => (
                "Not calculated",
                theme.muted_foreground,
                "Open Storage again or clear one cache entry to start a scan.".to_string(),
            ),
        };
    let mut status_description = status_summary;
    if let Some(error) = cache_stats.error.as_deref() {
        status_description = format!("Cache stats failed to calculate: {error}");
    }
    let status_text_color = if cache_stats.error.is_some() {
        theme.destructive
    } else {
        theme.muted_foreground
    };

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(224),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(20), px(16)),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(32),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                "Generated storage",
                                12.0,
                                theme.foreground,
                            );
                            copy.spawn(Node {
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                flex_wrap: FlexWrap::Wrap,
                                ..default()
                            })
                            .with_children(|status_row| {
                                spawn_text(
                                    status_row,
                                    font.clone(),
                                    "Usage",
                                    9.0,
                                    theme.muted_foreground,
                                );
                                status_row.spawn((
                                    Node {
                                        padding: UiRect::axes(px(8), px(3)),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(status_color.with_alpha(0.16)),
                                    BorderColor::all(status_color.with_alpha(0.45)),
                                    children![(
                                        Text::new(status),
                                        ui_text_font(font.clone(), 9.0),
                                        TextColor(status_color),
                                    )],
                                ));
                            });
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "Cached stems, charts, previews, models, and temporary authoring files.",
                                10.0,
                                theme.muted_foreground,
                            );
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                status_description,
                                10.0,
                                status_text_color,
                            );
                        });
                    spawn_setting_actions(
                        header,
                        font.clone(),
                        theme,
                        vec![
                            (
                                "Clear generated cache".to_string(),
                                UiAction::RequestClearCache(CacheClearScope::Generated),
                            ),
                            (
                                "Clear models".to_string(),
                                UiAction::RequestClearCache(CacheClearScope::Models),
                            ),
                        ],
                    );
                });

            if let Some(stats) = cache_stats.current.as_ref() {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(12),
                            padding: UiRect::all(px(12)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(7)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.24)),
                        BorderColor::all(theme.border.with_alpha(0.42)),
                    ))
                    .with_children(|bars| {
                        spawn_text(
                            bars,
                            font.clone(),
                            "Storage breakdown",
                            8.0,
                            theme.muted_foreground,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Songs",
                            stats.songs_bytes,
                            cache_category_share(stats.songs_bytes, total),
                            theme.primary,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Models",
                            stats.models_bytes,
                            cache_category_share(stats.models_bytes, total),
                            theme.editor_selection,
                        );
                        spawn_storage_usage_category(
                            bars,
                            font.clone(),
                            theme,
                            "Other",
                            stats.other_bytes,
                            cache_category_share(stats.other_bytes, total),
                            theme.waveform,
                        );
                    });
            }
        });
}

pub(crate) fn cache_category_share(part: u64, total: u64) -> f32 {
    if total == 0 {
        0.0
    } else {
        (part as f64 / total as f64) as f32
    }
}

pub(crate) fn spawn_storage_usage_category(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    bytes: u64,
    share: f32,
    color: Color,
) {
    let share = (share * 100.0).clamp(0.0, 100.0);
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        })
        .with_children(|entry| {
            entry
                .spawn(Node {
                    width: percent(100),
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|row| {
                    spawn_text(row, font.clone(), label, 8.0, theme.muted_foreground);
                    spawn_text(
                        row,
                        font.clone(),
                        format!("{} · {:.0}%", format_bytes(bytes), share),
                        9.0,
                        theme.foreground,
                    );
                });
            entry
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(7),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.muted.with_alpha(0.36)),
                    BorderColor::all(theme.border.with_alpha(0.45)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: percent(share),
                            height: px(7),
                            border_radius: BorderRadius::all(px(999.0)),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                });
        });
}

pub(crate) fn spawn_watched_folders_setting(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let paths = session.config.library_paths();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(104),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(20), px(16)),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(32),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(5),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                "Watched folders",
                                12.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "Add as many music locations as you need. Folder changes are merged into one library.",
                                10.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_setting_actions(
                        header,
                        font.clone(),
                        theme,
                        vec![
                            ("Add folder…".to_string(), UiAction::ChooseFolder),
                            ("Rescan all".to_string(), UiAction::RescanLibrary),
                        ],
                    );
                });

            if paths.is_empty() {
                panel
                    .spawn(Node {
                        width: percent(100),
                        min_height: px(34),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(9)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    })
                    .with_children(|empty| {
                        spawn_wrapped_text(
                            empty,
                            font.clone(),
                            "No local folders connected.",
                            9.0,
                            theme.muted_foreground,
                        );
                    });
            } else {
                for path in &paths {
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(38),
                                align_items: AlignItems::Center,
                                padding: UiRect::vertical(px(2)),
                                column_gap: px(32),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.32)),
                        ))
                        .with_children(|path_row| {
                            path_row
                                .spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    padding: UiRect::horizontal(px(9)),
                                    overflow: Overflow::clip(),
                                    ..default()
                                })
                                .with_children(|path_copy| {
                                    path_copy.spawn((
                                        Text::new(path.to_string_lossy().into_owned()),
                                        ui_text_font(font.clone(), 9.0),
                                        TextColor(theme.muted_foreground),
                                        TextLayout::no_wrap(),
                                    ));
                                });
                            path_row
                                .spawn(Node {
                                    width: px(SETTINGS_CONTROL_WIDTH),
                                    flex_shrink: 0.0,
                                    justify_content: JustifyContent::FlexEnd,
                                    ..default()
                                })
                                .with_children(|actions| {
                                    spawn_compact_action_button(
                                        actions,
                                        font.clone(),
                                        theme,
                                        "Remove",
                                        UiAction::RequestRemoveFolder(path.clone()),
                                    );
                                });
                        });
                }
            }
        });
}

pub(crate) fn spawn_settings_section(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect {
                left: px(20),
                right: px(20),
                top: px(20),
                bottom: px(7),
            },
            row_gap: px(3),
            ..default()
        })
        .with_children(|section| {
            spawn_text(section, font.clone(), label, 8.0, theme.primary);
            spawn_wrapped_text(section, font, description, 9.0, theme.muted_foreground);
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_settings_stage_header(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
    current: impl Into<String>,
    status: Option<(String, bool)>,
    action: Option<(String, UiAction)>,
) {
    let eyebrow = eyebrow.into();
    let title = title.into();
    let description = description.into();
    let current = current.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(92),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                margin: UiRect::top(px(16)),
                column_gap: px(32),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|copy| {
                    spawn_text(copy, font.clone(), eyebrow, 8.0, theme.primary);
                    spawn_text(copy, font.clone(), title, 14.0, theme.foreground);
                    spawn_wrapped_text(
                        copy,
                        font.clone(),
                        description,
                        9.0,
                        theme.muted_foreground,
                    );
                });
            header
                .spawn(Node {
                    width: px(SETTINGS_CONTROL_WIDTH),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexEnd,
                    flex_direction: FlexDirection::Column,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|summary| {
                    spawn_text(summary, font.clone(), current, 10.0, theme.foreground);
                    if let Some((label, available)) = status {
                        spawn_settings_badge(
                            summary,
                            font.clone(),
                            label,
                            if available {
                                theme.primary
                            } else {
                                theme.destructive
                            },
                        );
                    }
                    if let Some((label, action)) = action {
                        spawn_compact_action_button(summary, font, theme, label, action);
                    }
                });
        });
}

pub(crate) fn spawn_settings_badge(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    label: impl Into<String>,
    color: Color,
) {
    parent.spawn((
        Node {
            padding: UiRect::axes(px(8), px(3)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::MAX,
            ..default()
        },
        BackgroundColor(color.with_alpha(0.12)),
        BorderColor::all(color.with_alpha(0.38)),
        children![(Text::new(label), ui_text_font(font, 8.0), TextColor(color),)],
    ));
}

pub(crate) fn model_available(
    status: &app_core::AnalysisRuntimeStatus,
    target: app_core::ModelDownloadTarget,
) -> Option<bool> {
    status
        .models
        .iter()
        .find(|model| model.target == target)
        .map(|model| model.available)
}

pub(crate) fn spawn_model_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    native_setup: &NativeSetup,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "LOCAL INTELLIGENCE",
        "Models & runtime",
        "Checks are read-only; downloads start only after an explicit setup confirmation.",
    );
    if native_setup.receiver.is_some() || native_setup.progress.is_some() {
        spawn_setup_progress_panel(parent, font.clone(), icons.clone(), native_setup, theme);
    }
    let status = app_core::analysis_runtime_status();
    spawn_model_runtime_status_row(parent, font.clone(), theme, &status);
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Acceleration",
        "Choose the hardware target before installing the analysis environment.",
        SettingsSelectKind::ComputeBackend,
        session,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Shared analysis runtime",
        "Setup reuses compatible host ffmpeg, uv, Python, and existing model files. Nothing downloads until you confirm.",
        Some((
            if status.managed_runtime_available {
                "Reconfigure…"
            } else {
                "Set up…"
            },
            UiAction::RequestSetup(None),
        )),
    );
    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "MODEL FILES BY ANALYSIS STAGE",
        "This page only manages local files. Choose which engine is active in Analysis; every download still requires confirmation.",
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "01 · VOCAL SEPARATION",
        "Vocal separation",
        "Creates vocal and instrumental stems before recognition.",
        separator_label(session.config.separator()),
        &[app_core::ModelDownloadTarget::Separator],
    );
    spawn_model_stage(
        parent,
        font.clone(),
        theme,
        session,
        &status.models,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes lyrics. Compatibility and language-detection models are identified separately from the selected engine.",
        transcription_summary(&session.config),
        &[
            app_core::ModelDownloadTarget::OpenVinoWhisper,
            app_core::ModelDownloadTarget::Parakeet,
            app_core::ModelDownloadTarget::WhisperLanguageDetection,
            app_core::ModelDownloadTarget::Whisper,
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
        align_backend_label(session.config.align_backend()),
        &[
            app_core::ModelDownloadTarget::Alignment,
            app_core::ModelDownloadTarget::MmsKaraokeAlignment,
        ],
    );
    spawn_model_stage(
        parent,
        font,
        theme,
        session,
        &status.models,
        "04 · MELODY",
        "Melody & pitch",
        "Detects the sung fundamental frequency and creates note pitches.",
        pitch_model_label(session.config.pitch_model()),
        &[app_core::ModelDownloadTarget::Pitch],
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_model_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
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
        Some((
            "Configure in Analysis…".to_string(),
            UiAction::SettingsTab(SettingsTab::Analysis),
        )),
    );
    for model in models
        .iter()
        .filter(|model| targets.contains(&model.target))
    {
        spawn_model_install_row(parent, font.clone(), theme, session, model, title);
    }
}

pub(crate) fn model_install_role(
    config: &AppConfig,
    target: app_core::ModelDownloadTarget,
) -> &'static str {
    use app_core::ModelDownloadTarget;
    match target {
        ModelDownloadTarget::Whisper
            if config.asr_engine() == "parakeet"
                || config.compute_backend.as_deref() == Some("intel") =>
        {
            "Fallback"
        }
        ModelDownloadTarget::WhisperLanguageDetection => "Support",
        ModelDownloadTarget::MmsKaraokeAlignment if config.align_backend() != "mms_karaoke" => {
            "Optional"
        }
        _ => "Selected",
    }
}

pub(crate) fn spawn_model_install_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    model: &app_core::ModelInstallStatus,
    stage: &'static str,
) {
    let role = model_install_role(&session.config, model.target);
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(86),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(15)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
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
                        if role == "Optional" {
                            theme.muted_foreground
                        } else {
                            theme.primary
                        },
                    );
                    spawn_settings_badge(
                        title,
                        font.clone(),
                        if model.available {
                            "Installed"
                        } else {
                            "Missing"
                        },
                        if model.available {
                            theme.primary
                        } else {
                            theme.destructive
                        },
                    );
                });
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!("{} Used by Analysis > {stage}.", model.description),
                    9.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|actions| {
                spawn_compact_action_button(
                    actions,
                    font,
                    theme,
                    if model.available {
                        "Reinstall…"
                    } else {
                        "Download…"
                    },
                    UiAction::RequestSetup(Some(model.target)),
                );
            });
        });
}

pub(crate) fn spawn_model_runtime_status_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    status: &app_core::AnalysisRuntimeStatus,
) {
    let (headline, status_color, status_hint) = if status.ready {
        (
            "Ready to analyze",
            theme.primary,
            "The selected runtime and every required model are available locally.",
        )
    } else {
        (
            "Setup required",
            theme.destructive,
            "Some required components are missing. Open setup to install or repair.",
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
                                            Text::new(if status.ready { "OK" } else { "MISSING" }),
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
                                    format!("Missing components: {}", status.missing.join(" · ")),
                                    8.5,
                                    theme.destructive,
                                );
                            }
                        });
                    spawn_setting_actions(
                        status_row,
                        font.clone(),
                        theme,
                        vec![("Check again".to_string(), UiAction::RefreshRuntimeStatus)],
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
                        "uv",
                        status.uv_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Python",
                        status.system_python_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Analyzer",
                        status.analyzer_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Pitch model",
                        status.pitch_model_available,
                    );
                    spawn_runtime_component_row(
                        stack,
                        font.clone(),
                        theme,
                        "Selected models",
                        status.selected_models_available,
                    );
                });
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

pub(crate) fn transcription_summary(config: &AppConfig) -> String {
    if config.asr_engine() == "parakeet" {
        "Parakeet v3".to_string()
    } else if config.compute_backend.as_deref() == Some("intel") {
        "OpenVINO Whisper large-v3-turbo".to_string()
    } else {
        format!(
            "Whisper {}",
            settings_select_label(SettingsSelectKind::WhisperModel, config.whisper_model(),)
        )
    }
}

pub(crate) fn transcription_model_target(config: &AppConfig) -> app_core::ModelDownloadTarget {
    if config.asr_engine() == "parakeet" {
        app_core::ModelDownloadTarget::Parakeet
    } else if config.compute_backend.as_deref() == Some("intel") {
        app_core::ModelDownloadTarget::OpenVinoWhisper
    } else {
        app_core::ModelDownloadTarget::Whisper
    }
}

pub(crate) fn alignment_model_target(config: &AppConfig) -> Option<app_core::ModelDownloadTarget> {
    match config.align_backend() {
        "qwen" => Some(app_core::ModelDownloadTarget::Alignment),
        "mms_karaoke" => Some(app_core::ModelDownloadTarget::MmsKaraokeAlignment),
        _ => None,
    }
}

pub(crate) fn analysis_stage_status(
    status: &app_core::AnalysisRuntimeStatus,
    target: Option<app_core::ModelDownloadTarget>,
) -> (String, bool) {
    match target.and_then(|target| model_available(status, target)) {
        Some(true) => ("Installed".to_string(), true),
        Some(false) => ("Model missing".to_string(), false),
        None if status.analyzer_available => ("Runtime managed".to_string(), true),
        None => ("Runtime missing".to_string(), false),
    }
}

pub(crate) fn spawn_analysis_pipeline(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSession,
    status: &app_core::AnalysisRuntimeStatus,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.3)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|panel| {
            spawn_text(
                panel,
                font.clone(),
                "CURRENT ANALYSIS PIPELINE",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                panel,
                font.clone(),
                "The same four stages and names are used on Models & runtime.",
                9.0,
                theme.muted_foreground,
            );
            panel
                .spawn(Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|pipeline| {
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "01 · Vocals",
                        separator_label(session.config.separator()),
                        analysis_stage_status(
                            status,
                            Some(app_core::ModelDownloadTarget::Separator),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "02 · Lyrics",
                        transcription_summary(&session.config),
                        analysis_stage_status(
                            status,
                            Some(transcription_model_target(&session.config)),
                        ),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "03 · Timing",
                        align_backend_label(session.config.align_backend()),
                        analysis_stage_status(status, alignment_model_target(&session.config)),
                    );
                    spawn_analysis_pipeline_stage(
                        pipeline,
                        font.clone(),
                        theme,
                        "04 · Pitch",
                        pitch_model_label(session.config.pitch_model()),
                        analysis_stage_status(status, Some(app_core::ModelDownloadTarget::Pitch)),
                    );
                });
        });
}

pub(crate) fn spawn_analysis_pipeline_stage(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stage: &'static str,
    selected: impl Into<String>,
    status: (String, bool),
) {
    parent
        .spawn((
            Node {
                min_width: px(190),
                min_height: px(70),
                flex_basis: px(220),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(11)),
                row_gap: px(5),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.48)),
            BorderColor::all(theme.border.with_alpha(0.46)),
        ))
        .with_children(|card| {
            spawn_text(card, font.clone(), stage, 8.0, theme.muted_foreground);
            spawn_text(card, font.clone(), selected, 10.0, theme.foreground);
            spawn_settings_badge(
                card,
                font,
                status.0,
                if status.1 {
                    theme.primary
                } else {
                    theme.destructive
                },
            );
        });
}

pub(crate) fn spawn_analysis_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "GENERATION",
        "Analysis",
        "Configure each stage of newly generated stems, lyrics, timing, and pitch. Existing charts change only after re-analysis.",
    );
    let status = app_core::analysis_runtime_status();
    spawn_analysis_pipeline(parent, font.clone(), theme, session, &status);

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · VOCAL SEPARATION",
        "Vocal separation",
        "Creates a clean vocal source before lyrics and pitch are analyzed.",
        separator_label(session.config.separator()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Separator),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Separation engine",
        "Choose the model family that creates vocal and instrumental stems.",
        SettingsSelectKind::Separator,
        session,
    );
    let separation_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Separation);
    if session.config.separator() != "openvino_demucs" {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "Separation profile",
            if session.config.separator() == "karaoke" {
                "Balanced is recommended. Memory saver uses shorter RoFormer segments; Quality increases segment context and overlap."
            } else {
                "Balanced is recommended. Quality adds shifts and overlap, increasing processing time substantially."
            },
            SettingsSelectKind::SeparatorPreset,
            session,
        );
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            "Advanced separation tuning",
            "Model-specific memory, quality, and overlap controls. Existing stems change only after re-analysis.",
            Some((
                if separation_advanced {
                    "Hide advanced"
                } else {
                    "Show advanced"
                },
                UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Separation),
            )),
        );
    } else {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "FIXED OPENVINO PROFILE",
            "Segment dimensions and overlap are compiled into the installed OpenVINO Demucs graph. Select UVR Karaoke or Demucs to use adjustable separation profiles.",
        );
    }
    if separation_advanced && session.config.separator() == "karaoke" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer segment size",
            "Model default is used until edited. Smaller values reduce memory; larger values may improve continuity. Range: 64–1024.",
            session.config.separator_segment_size(),
            NumericSetting::SeparatorSegmentSize,
            UiAction::AdjustSeparatorSegmentSize(-32),
            UiAction::AdjustSeparatorSegmentSize(32),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer overlap",
            "More overlap can reduce chunk seams at the cost of additional processing. Range: 2–32.",
            session.config.separator_overlap(),
            NumericSetting::SeparatorOverlap,
            UiAction::AdjustSeparatorOverlap(-1),
            UiAction::AdjustSeparatorOverlap(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer batch size",
            "Lower this first if separation runs out of system or accelerator memory. Range: 1–8.",
            session.config.separator_batch_size(),
            NumericSetting::SeparatorBatchSize,
            UiAction::AdjustSeparatorBatchSize(-1),
            UiAction::AdjustSeparatorBatchSize(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Output normalization",
            "Peak normalization applied by the separator before stems enter the lossless cache. Range: 1–100%.",
            session.config.separator_normalization_pct(),
            NumericSetting::SeparatorNormalization,
            UiAction::AdjustSeparatorNormalization(-1),
            UiAction::AdjustSeparatorNormalization(1),
        );
    } else if separation_advanced && session.config.separator() == "demucs" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs shifts",
            "More random shifts can improve separation quality but multiply inference cost. Range: 1–8.",
            session.config.demucs_shifts(),
            NumericSetting::DemucsShifts,
            UiAction::AdjustDemucsShifts(-1),
            UiAction::AdjustDemucsShifts(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs overlap",
            "Overlap between inference windows. Range: 1–95%.",
            session.config.demucs_overlap_pct(),
            NumericSetting::DemucsOverlap,
            UiAction::AdjustDemucsOverlap(-1),
            UiAction::AdjustDemucsOverlap(1),
        );
    }

    let parakeet = session.config.asr_engine() == "parakeet";
    let intel_whisper = !parakeet && session.config.compute_backend.as_deref() == Some("intel");
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes sung words. Fallback settings appear separately when the primary engine needs them.",
        transcription_summary(&session.config),
        Some(analysis_stage_status(
            &status,
            Some(transcription_model_target(&session.config)),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Primary transcription engine",
        "Whisper is broadly compatible; Parakeet is faster for its supported languages.",
        SettingsSelectKind::AsrEngine,
        session,
    );
    if parakeet || intel_whisper {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "COMPATIBILITY FALLBACK",
            if parakeet {
                "Whisper is used only for unsupported languages or when Parakeet returns no usable words."
            } else {
                "Standard Whisper is retained for cases the Intel OpenVINO path cannot process."
            },
        );
    }
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        if parakeet || intel_whisper {
            "Whisper fallback model"
        } else {
            "Whisper model"
        },
        if parakeet || intel_whisper {
            "This does not replace the primary engine; it is loaded only when compatibility fallback is needed."
        } else {
            "Turbo is the balanced default; larger models trade speed for detail."
        },
        SettingsSelectKind::WhisperModel,
        session,
    );
    let transcription_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Transcription);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced transcription tuning",
        "Memory and search controls for this transcription stage.",
        Some((
            if transcription_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Transcription),
        )),
    );
    if transcription_advanced {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            if parakeet || intel_whisper {
                "Whisper fallback precision"
            } else {
                "Recognition precision"
            },
            "Whisper search breadth. Values are clamped between 1 and 16.",
            session.config.beam_size(),
            NumericSetting::BeamSize,
            UiAction::AdjustBeamSize(-1),
            UiAction::AdjustBeamSize(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            if parakeet {
                "Parakeet batch size"
            } else {
                "Whisper batch size"
            },
            "Lower this if this transcription engine runs out of GPU or system memory.",
            session.config.batch_size(),
            NumericSetting::BatchSize,
            UiAction::AdjustBatchSize(-1),
            UiAction::AdjustBatchSize(1),
        );
    }

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "03 · WORD TIMING",
        "Word timing & alignment",
        "Refines recognized or supplied lyrics into editable word timings.",
        align_backend_label(session.config.align_backend()),
        Some(analysis_stage_status(
            &status,
            alignment_model_target(&session.config),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Alignment engine",
        if session.config.align_backend() == "mms_karaoke" {
            "MMS Karaoke targets known Japanese lyrics. Automatic transcription retains its compatible timing path."
        } else if parakeet {
            "Used for compatibility fallback and supplied lyrics; Parakeet's direct timestamps can skip this stage."
        } else {
            "Choose how recognized or supplied lyrics are refined into word timings."
        },
        SettingsSelectKind::AlignBackend,
        session,
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "04 · MELODY",
        "Melody & pitch",
        "Detects sung pitch after vocal separation and creates editable notes.",
        pitch_model_label(session.config.pitch_model()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Pitch),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons,
        theme,
        "Pitch detection model",
        "Detects the sung fundamental frequency used to create note pitches.",
        SettingsSelectKind::PitchModel,
        session,
    );
    let pitch_advanced = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Pitch);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced pitch tuning",
        "Controls how strongly detected vocals are filtered before notes are created.",
        Some((
            if pitch_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Pitch),
        )),
    );
    if pitch_advanced {
        let threshold = (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32;
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Vocal detection sensitivity",
            "Lower for soft singing; raise to remove more silence. Range: 0–60%.",
            threshold,
            NumericSetting::VocalThreshold,
            UiAction::AdjustVocalThreshold(-1),
            UiAction::AdjustVocalThreshold(1),
        );
    }

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "AUTOMATION",
        "Controls when the four-stage pipeline starts; these are not model settings.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Auto-analyze",
        if session.config.auto_analyze() {
            "On · Unanalyzed songs are queued after a library scan."
        } else {
            "Off · New songs wait for an explicit analysis action."
        },
        session.config.auto_analyze(),
        UiAction::ToggleAutoAnalyze,
    );
    spawn_setting_row(
        parent,
        font,
        theme,
        "Analysis defaults",
        "Restore every stage and its advanced controls to the recommended starting values.",
        Some(("Restore defaults", UiAction::RestoreAnalysisDefaults)),
    );
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_number_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: u32,
    setting: NumericSetting,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control.spawn((
                            EditableText {
                                max_characters: Some(2),
                                ..EditableText::new(value.to_string())
                            },
                            setting,
                            Node {
                                min_width: px(56),
                                height: px(20),
                                flex_grow: 1.0,
                                align_self: AlignSelf::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            ui_text_font(font.clone(), 11.0),
                            TextColor(theme.foreground),
                            TextLayout::justify(Justify::Center),
                            TextCursorStyle {
                                color: theme.primary,
                                selected_text_color: Some(theme.primary_foreground),
                                ..default()
                            },
                            TabIndex(0),
                        ));
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

pub(crate) fn spawn_switch_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    enabled: bool,
    action: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Button,
                        action,
                        Node {
                            width: px(42),
                            height: px(24),
                            align_items: AlignItems::Center,
                            justify_content: if enabled {
                                JustifyContent::FlexEnd
                            } else {
                                JustifyContent::FlexStart
                            },
                            padding: UiRect::horizontal(px(3)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(if enabled {
                            theme.primary.with_alpha(0.86)
                        } else {
                            theme.background.with_alpha(0.7)
                        }),
                        BorderColor::all(if enabled {
                            theme.primary.with_alpha(0.9)
                        } else {
                            theme.border.with_alpha(0.75)
                        }),
                    ))
                    .with_children(|switch| {
                        switch.spawn((
                            Node {
                                width: px(16),
                                height: px(16),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(if enabled {
                                theme.primary_foreground
                            } else {
                                theme.muted_foreground.with_alpha(0.8)
                            }),
                        ));
                    });
            });
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_shift_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: impl Into<String>,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    let value = value.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(68),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(13)),
                column_gap: px(22),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(copy, font.clone(), description, 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control
                            .spawn(Node {
                                min_width: px(68),
                                flex_grow: 1.0,
                                height: percent(100),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            })
                            .with_children(|value_node| {
                                spawn_text(value_node, font.clone(), value, 10.0, theme.foreground);
                            });
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

pub(crate) fn spawn_setup_progress_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    setup: &NativeSetup,
    theme: &StudioTheme,
) {
    let progress_percent = setup
        .progress
        .as_ref()
        .map_or(0, |progress| progress.percent);
    let action = setup
        .progress
        .as_ref()
        .map(|progress| progress.action.as_str())
        .unwrap_or("Starting setup…");
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(px(18)),
                padding: UiRect::all(px(16)),
                row_gap: px(9),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.44)),
            BorderColor::all(theme.primary.with_alpha(0.34)),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(9),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons.clone(), UiIcon::Repair, 17.0, theme.primary);
                    spawn_text(
                        header,
                        font.clone(),
                        "Setting up models & runtime",
                        12.0,
                        theme.foreground,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{progress_percent}%"),
                        10.0,
                        theme.primary,
                    );
                });
            spawn_wrapped_text(panel, font.clone(), action, 10.0, theme.muted_foreground);
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(4),
                        overflow: Overflow::clip(),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.border.with_alpha(0.38)),
                ))
                .with_children(|track| {
                    track.spawn((
                        Node {
                            width: percent(progress_percent as f32),
                            height: percent(100),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary),
                    ));
                });
            if let Some(progress) = setup.progress.as_ref() {
                for task in &progress.tasks {
                    let (icon, color) = match task.state {
                        app_core::SetupTaskState::Done => (UiIcon::Check, theme.primary),
                        app_core::SetupTaskState::Running => (UiIcon::Repair, theme.foreground),
                        app_core::SetupTaskState::Pending => {
                            (UiIcon::CircleCheck, theme.muted_foreground.with_alpha(0.45))
                        }
                    };
                    panel
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|task_row| {
                            spawn_icon(task_row, icons.clone(), icon, 13.0, color);
                            spawn_text(task_row, font.clone(), task.label.clone(), 9.0, color);
                            if let Some(bytes) = task.downloaded_bytes {
                                task_row.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                spawn_text(
                                    task_row,
                                    font.clone(),
                                    match task.total_bytes {
                                        Some(total) => format!(
                                            "{} / {}",
                                            format_bytes(bytes),
                                            format_bytes(total)
                                        ),
                                        None => format_bytes(bytes),
                                    },
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                        });
                }
            }
            for line in setup.logs.iter().rev().take(4).rev() {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    line,
                    8.0,
                    theme.muted_foreground.with_alpha(0.76),
                );
            }
        });
}

pub(crate) fn spawn_setup_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    request: SetupRequest,
) {
    let mms_karaoke_selected = app_core::AppConfig::load().align_backend() == "mms_karaoke";
    let mms_karaoke_download = matches!(
        request.target,
        Some(app_core::ModelDownloadTarget::MmsKaraokeAlignment)
    ) || (mms_karaoke_selected
        && matches!(
            request.target,
            None | Some(app_core::ModelDownloadTarget::Alignment)
        ));
    let (title, description) = if mms_karaoke_download {
        if request.target.is_some() {
            (
                "Download MMS Karaoke model?",
                "Uta Studio will download the optional 1.26 GB Japanese alignment model from NextFire. The model is currently published under AGPL-3.0; confirming means you choose to install and use that separately licensed artifact.",
            )
        } else {
            (
                "Set up runtime and MMS Karaoke?",
                "Uta Studio will prepare the analysis runtime and download the selected optional 1.26 GB Japanese alignment model. The NextFire model is currently published under AGPL-3.0; confirming means you choose to install and use that separately licensed artifact.",
            )
        }
    } else if request.target.is_some() {
        (
            "Download selected model?",
            "Uta Studio will use the configured host tools and download only the selected artifact after you confirm.",
        )
    } else {
        (
            "Set up analysis runtime?",
            "Uta Studio will reuse compatible host tools and existing artifacts, then install only missing runtime packages and models.",
        )
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.74)),
        ZIndex(80),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "{description}\n\nDownloads never start merely because Settings was opened. You can cancel now without changing any runtime or model data."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelSetup,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmSetup,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.primary),
                            children![(
                                Text::new(if request.target.is_some() {
                                    "Download"
                                } else {
                                    "Set up"
                                }),
                                ui_text_font(font, 10.0),
                                TextColor(theme.primary_foreground),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

pub(crate) fn spawn_global_cache_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    scope: CacheClearScope,
) {
    let (title, description) = match scope {
        CacheClearScope::Generated => (
            "Clear generated cache?",
            "Generated stems, charts, previews, and authoring variants will be removed. Indexed source songs remain untouched.",
        ),
        CacheClearScope::Models => (
            "Clear downloaded models?",
            "Downloaded model artifacts will be removed. Existing configured directories remain in place, and analysis stays disabled until an explicit download.",
        ),
    };
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(90),
        children![(
            Node {
                width: px(470),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new(title),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(description),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelClearCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmClearCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Clear now"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

pub(crate) fn compute_backend_label(value: &str) -> &'static str {
    match value {
        "cuda" => "NVIDIA CUDA",
        "intel" => "Intel Arc",
        _ => "CPU",
    }
}

pub(crate) fn separator_label(value: &str) -> &'static str {
    match value {
        "demucs" => "Demucs",
        "openvino_demucs" => "OpenVINO Demucs v4 (Intel GPU)",
        _ => "UVR Karaoke",
    }
}

pub(crate) fn asr_engine_label(value: &str) -> &'static str {
    if value == "parakeet" {
        "Parakeet v3 (Experimental)"
    } else {
        "Whisper"
    }
}

pub(crate) fn align_backend_label(value: &str) -> &'static str {
    match value {
        "ctc" => "CTC Forced Alignment",
        "qwen" => "Qwen Forced Alignment",
        "mms_karaoke" => "MMS Karaoke (Japanese)",
        _ => "WhisperX",
    }
}

pub(crate) fn pitch_model_label(value: &str) -> &'static str {
    match value {
        "rmvpe" => "RMVPE",
        _ => "RMVPE",
    }
}

pub(crate) fn settings_select_value(kind: SettingsSelectKind, config: &AppConfig) -> &str {
    match kind {
        SettingsSelectKind::UiLanguage => config.ui_language(),
        SettingsSelectKind::ComputeBackend => config.compute_backend.as_deref().unwrap_or("cpu"),
        SettingsSelectKind::Separator => config.separator(),
        SettingsSelectKind::SeparatorPreset => separator_preset(config),
        SettingsSelectKind::AsrEngine => config.asr_engine(),
        SettingsSelectKind::WhisperModel => config.whisper_model(),
        SettingsSelectKind::AlignBackend => config.align_backend(),
        SettingsSelectKind::PitchModel => config.pitch_model(),
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
        SettingsSelectKind::ComputeBackend => compute_backend_label(value),
        SettingsSelectKind::Separator => separator_label(value),
        SettingsSelectKind::SeparatorPreset => match value {
            "memory" => "Memory saver",
            "quality" => "Quality",
            "custom" => "Custom",
            _ => "Balanced",
        },
        SettingsSelectKind::AsrEngine => asr_engine_label(value),
        SettingsSelectKind::WhisperModel => match value {
            "large-v3" => "Large v3",
            "large-v3-turbo" => "Large v3 Turbo",
            "medium" => "Medium",
            "small" => "Small",
            "base" => "Base",
            "tiny" => "Tiny",
            _ => "Large v3",
        },
        SettingsSelectKind::AlignBackend => align_backend_label(value),
        SettingsSelectKind::PitchModel => pitch_model_label(value),
    }
}

pub(crate) fn settings_select_options(
    kind: SettingsSelectKind,
    intel_backend: bool,
) -> &'static [(&'static str, &'static str)] {
    match kind {
        SettingsSelectKind::UiLanguage => &[
            ("system", "System default"),
            ("en", "English"),
            ("zh-CN", "简体中文"),
            ("ja", "日本語"),
        ],
        SettingsSelectKind::ComputeBackend => &[
            ("cpu", "CPU"),
            ("cuda", "NVIDIA CUDA"),
            ("intel", "Intel Arc"),
        ],
        SettingsSelectKind::Separator if intel_backend => &[
            ("karaoke", "UVR Karaoke"),
            ("demucs", "Demucs"),
            ("openvino_demucs", "OpenVINO Demucs v4"),
        ],
        SettingsSelectKind::Separator => &[("karaoke", "UVR Karaoke"), ("demucs", "Demucs")],
        SettingsSelectKind::SeparatorPreset => &[
            ("balanced", "Balanced · recommended"),
            ("memory", "Memory saver · lower peak usage"),
            ("quality", "Quality · slower, more context"),
        ],
        SettingsSelectKind::AsrEngine => &[
            ("whisper", "Whisper"),
            ("parakeet", "Parakeet v3 (Experimental)"),
        ],
        SettingsSelectKind::WhisperModel => &[
            ("large-v3", "Large v3"),
            ("large-v3-turbo", "Large v3 Turbo"),
            ("medium", "Medium"),
            ("small", "Small"),
            ("base", "Base"),
            ("tiny", "Tiny"),
        ],
        SettingsSelectKind::AlignBackend => &[
            ("whisperx", "WhisperX"),
            ("ctc", "CTC Forced Alignment"),
            ("qwen", "Qwen Forced Alignment"),
            ("mms_karaoke", "MMS Karaoke (Japanese)"),
        ],
        SettingsSelectKind::PitchModel => &[("rmvpe", "RMVPE")],
    }
}

pub(crate) fn separator_preset(config: &AppConfig) -> &'static str {
    match config.separator() {
        "karaoke"
            if config.separator_segment_size.is_none()
                && config.separator_overlap() == 8
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "balanced"
        }
        "karaoke"
            if config.separator_segment_size == Some(128)
                && config.separator_overlap() == 4
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 90 =>
        {
            "memory"
        }
        "karaoke"
            if config.separator_segment_size == Some(512)
                && config.separator_overlap() == 16
                && config.separator_batch_size() == 1
                && config.separator_normalization_pct() == 95 =>
        {
            "quality"
        }
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 25 => "balanced",
        "demucs" if config.demucs_shifts() == 1 && config.demucs_overlap_pct() == 15 => "memory",
        "demucs" if config.demucs_shifts() == 2 && config.demucs_overlap_pct() == 50 => "quality",
        "openvino_demucs" => "balanced",
        _ => "custom",
    }
}

pub(crate) fn apply_separator_preset(config: &mut AppConfig, preset: &str) {
    match (config.separator(), preset) {
        ("karaoke", "balanced") => {
            config.separator_segment_size = None;
            config.separator_overlap = None;
            config.separator_batch_size = None;
            config.separator_normalization_pct = None;
        }
        ("karaoke", "memory") => {
            config.separator_segment_size = Some(128);
            config.separator_overlap = Some(4);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(90);
        }
        ("karaoke", "quality") => {
            config.separator_segment_size = Some(512);
            config.separator_overlap = Some(16);
            config.separator_batch_size = Some(1);
            config.separator_normalization_pct = Some(95);
        }
        ("demucs", "balanced") => {
            config.demucs_shifts = None;
            config.demucs_overlap_pct = None;
        }
        ("demucs", "memory") => {
            config.demucs_shifts = Some(1);
            config.demucs_overlap_pct = Some(15);
        }
        ("demucs", "quality") => {
            config.demucs_shifts = Some(2);
            config.demucs_overlap_pct = Some(50);
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_select_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    kind: SettingsSelectKind,
    session: &StudioSession,
) {
    let label = label.into();
    let description = description.into();
    let current = settings_select_value(kind, &session.config);
    let open = session.open_settings_select == Some(kind);
    let options = settings_select_options(
        kind,
        session.config.compute_backend.as_deref() == Some("intel"),
    );
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                min_height: px(76),
                align_items: if open {
                    AlignItems::FlexStart
                } else {
                    AlignItems::Center
                },
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
            ZIndex(if open { 60 } else { 0 }),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                position_type: PositionType::Relative,
                width: px(SETTINGS_CONTROL_WIDTH),
                height: if open { Val::Auto } else { px(36) },
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|control| {
                control
                    .spawn((
                        Button,
                        UiAction::OpenSettingsSelect(kind),
                        Node {
                            width: percent(100),
                            height: px(36),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(12)),
                            column_gap: px(8),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(if open { 0.76 } else { 0.5 })),
                        BorderColor::all(if open {
                            theme.primary.with_alpha(0.72)
                        } else {
                            theme.border.with_alpha(0.66)
                        }),
                    ))
                    .with_children(|button| {
                        spawn_text(
                            button,
                            font.clone(),
                            settings_select_label(kind, current),
                            10.0,
                            theme.foreground,
                        );
                        button.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_icon(
                            button,
                            icons.clone(),
                            UiIcon::ChevronDown,
                            14.0,
                            theme.muted_foreground,
                        );
                    });
                if open {
                    control
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
                            ZIndex(60),
                        ))
                        .with_children(|menu| {
                            for (value, option_label) in options {
                                let selected = *value == current;
                                menu.spawn((
                                    Button,
                                    UiAction::SelectSettingsValue(kind, (*value).to_string()),
                                    Node {
                                        width: percent(100),
                                        min_height: px(31),
                                        align_items: AlignItems::Center,
                                        padding: UiRect::axes(px(9), px(7)),
                                        column_gap: px(8),
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
                                        font.clone(),
                                        *option_label,
                                        10.0,
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
                                        spawn_icon(
                                            option,
                                            icons.clone(),
                                            UiIcon::Check,
                                            14.0,
                                            theme.primary,
                                        );
                                    }
                                });
                            }
                        });
                }
            });
        });
}

pub(crate) fn spawn_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    action: Option<(impl Into<String>, UiAction)>,
) {
    let label = label.into();
    let description = description.into();
    let action = action.map(|(label, action)| (label.into(), action));
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            if let Some((label, action)) = action {
                row.spawn(Node {
                    width: px(SETTINGS_CONTROL_WIDTH),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::FlexEnd,
                    ..default()
                })
                .with_children(|control_column| {
                    spawn_action_button(control_column, font, theme, label, action);
                });
            }
        });
}

pub(crate) fn spawn_setting_row_with_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    actions: Vec<(String, UiAction)>,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(92),
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            spawn_setting_actions(row, font, theme, actions);
        });
}

pub(crate) fn spawn_setting_actions(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    actions: Vec<(String, UiAction)>,
) {
    parent
        .spawn(Node {
            width: px(SETTINGS_CONTROL_WIDTH),
            flex_shrink: 0.0,
            justify_content: JustifyContent::FlexEnd,
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(8),
            column_gap: px(8),
            ..default()
        })
        .with_children(|controls| {
            for (label, action) in actions {
                spawn_compact_action_button(controls, font.clone(), theme, label, action);
            }
        });
}

pub(crate) fn spawn_source_file_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    path: &std::path::Path,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(82),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), "Source file", 12.0, theme.foreground);
                copy.spawn((
                    Text::new(path.to_string_lossy().into_owned()),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                ));
            });
            row.spawn(Node {
                width: px(112),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|action| {
                spawn_action_button(
                    action,
                    font,
                    theme,
                    "Open",
                    UiAction::OpenSource(path.to_path_buf()),
                );
            });
        });
}

pub(crate) fn save_config_error(config: &AppConfig) -> Option<String> {
    config
        .save()
        .err()
        .map(|error| format!("Could not save settings: {error}"))
}

pub(crate) fn sync_numeric_settings(
    mut inputs: Query<(&mut EditableText, &NumericSetting)>,
    mut session: ResMut<StudioSession>,
) {
    for (mut input, setting) in &mut inputs {
        // `Changed<EditableText>` also fires the instant the component is
        // spawned, which would wrongly treat this field respawning (e.g. the
        // settings panel switching tabs) as the user having retyped it.
        if input.is_added() || !input.is_changed() {
            continue;
        }
        let raw = input.value().to_string();
        let Ok(parsed) = raw.trim().parse::<u32>() else {
            continue;
        };
        let (minimum, maximum) = match setting {
            NumericSetting::BeamSize | NumericSetting::BatchSize => (1, 16),
            NumericSetting::VocalThreshold => (0, 60),
            NumericSetting::SeparatorSegmentSize => (64, 1024),
            NumericSetting::SeparatorOverlap => (2, 32),
            NumericSetting::SeparatorBatchSize | NumericSetting::DemucsShifts => (1, 8),
            NumericSetting::SeparatorNormalization => (1, 100),
            NumericSetting::DemucsOverlap => (1, 95),
        };
        let clamped = parsed.clamp(minimum, maximum);
        if clamped != parsed {
            input.editor_mut().set_text(&clamped.to_string());
        }
        let current = match setting {
            NumericSetting::BeamSize => session.config.beam_size(),
            NumericSetting::BatchSize => session.config.batch_size(),
            NumericSetting::VocalThreshold => {
                (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32
            }
            NumericSetting::SeparatorSegmentSize => session.config.separator_segment_size(),
            NumericSetting::SeparatorOverlap => session.config.separator_overlap(),
            NumericSetting::SeparatorBatchSize => session.config.separator_batch_size(),
            NumericSetting::SeparatorNormalization => session.config.separator_normalization_pct(),
            NumericSetting::DemucsShifts => session.config.demucs_shifts(),
            NumericSetting::DemucsOverlap => session.config.demucs_overlap_pct(),
        };
        if clamped == current {
            continue;
        }
        match setting {
            NumericSetting::BeamSize => session.config.beam_size = Some(clamped),
            NumericSetting::BatchSize => session.config.batch_size = Some(clamped),
            NumericSetting::VocalThreshold => {
                session.config.vocal_detection_threshold_pct = Some(f64::from(clamped) / 100.0)
            }
            NumericSetting::SeparatorSegmentSize => {
                session.config.separator_segment_size = Some(clamped)
            }
            NumericSetting::SeparatorOverlap => session.config.separator_overlap = Some(clamped),
            NumericSetting::SeparatorBatchSize => {
                session.config.separator_batch_size = Some(clamped)
            }
            NumericSetting::SeparatorNormalization => {
                session.config.separator_normalization_pct = Some(clamped)
            }
            NumericSetting::DemucsShifts => session.config.demucs_shifts = Some(clamped),
            NumericSetting::DemucsOverlap => session.config.demucs_overlap_pct = Some(clamped),
        }
        if let Some(error) = save_config_error(&session.config) {
            session.notice = Some(error);
        }
    }
}

pub(crate) fn start_cache_stats_job(cache_stats: &mut CacheStatsJob) {
    if cache_stats.receiver.is_some() {
        return;
    }
    cache_stats.error = None;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = sender.send(app_core::CacheStats::calculate());
    });
    cache_stats.receiver = Some(Mutex::new(receiver));
}

pub(crate) fn handle_cache_stats_request(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut session: ResMut<StudioSession>,
) {
    if !session.request_cache_stats_refresh {
        return;
    }
    session.request_cache_stats_refresh = false;
    if cache_stats.current.is_none() && cache_stats.receiver.is_none() {
        start_cache_stats_job(&mut cache_stats);
    }
}

pub(crate) fn poll_cache_stats(
    mut cache_stats: ResMut<CacheStatsJob>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = cache_stats
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(stats) => Some(Ok(stats)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Cache stats worker exited unexpectedly.".to_string()))
                }
            },
            Err(_) => Some(Err("Cache stats status channel was poisoned.".to_string())),
        });
    let Some(result) = result else {
        return;
    };
    cache_stats.receiver = None;
    match result {
        Ok(stats) => {
            cache_stats.current = Some(stats);
            cache_stats.error = None;
        }
        Err(error) => cache_stats.error = Some(error),
    }
    invalidated.0 = true;
}

pub(crate) fn start_native_setup(
    config: &AppConfig,
    request: SetupRequest,
    setup: &mut NativeSetup,
) {
    let (sender, receiver) = mpsc::channel();
    let folders = setup_folders(config, request);
    std::thread::spawn(move || {
        let progress_sender = sender.clone();
        let log_sender = sender.clone();
        let relocation_sender = sender.clone();
        let result = app_core::run_vendor_setup(
            folders,
            move |progress| {
                let _ = progress_sender.send(SetupEvent::Progress(progress));
            },
            move |line| {
                let _ = log_sender.send(SetupEvent::Log(line));
            },
            move |path| {
                relocation_sender
                    .send(SetupEvent::Log(format!(
                        "Application data relocated to {}",
                        path.display()
                    )))
                    .map_err(|error| error.to_string())
            },
        );
        let _ = sender.send(SetupEvent::Complete(result));
    });
    setup.receiver = Some(Mutex::new(receiver));
    setup.progress = None;
    setup.logs.clear();
}

pub(crate) fn setup_folders(config: &AppConfig, request: SetupRequest) -> app_core::SetupFolders {
    app_core::SetupFolders {
        data_path: None,
        cache_paths: config.cache_paths.clone(),
        compute_backend: match config.compute_backend.as_deref() {
            Some("cuda") => app_core::ComputeBackend::Cuda,
            Some("intel") => app_core::ComputeBackend::Intel,
            _ => app_core::ComputeBackend::Cpu,
        },
        model_target: request.target,
    }
}

pub(crate) fn poll_native_setup(
    mut setup: ResMut<NativeSetup>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let mut events = Vec::new();
    let mut channel_poisoned = false;
    {
        let Some(receiver) = setup.receiver.as_ref() else {
            return;
        };
        match receiver.lock() {
            Ok(receiver) => loop {
                match receiver.try_recv() {
                    Ok(event) => events.push(event),
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        events.push(SetupEvent::Complete(Err(
                            "Analysis setup worker exited unexpectedly.".to_string(),
                        )));
                        break;
                    }
                }
            },
            Err(_) => channel_poisoned = true,
        }
    }
    if channel_poisoned {
        setup.receiver = None;
        setup.progress = None;
        session.notice = Some("Analysis setup status channel was poisoned.".to_string());
        invalidated.0 = true;
        return;
    }
    for event in events {
        match event {
            SetupEvent::Progress(progress) => {
                session.notice = Some(format!("{} · {}%", progress.action, progress.percent));
                setup.progress = Some(progress);
                invalidated.0 = true;
            }
            SetupEvent::Log(line) => {
                setup.logs.push(line);
                if setup.logs.len() > 200 {
                    let excess = setup.logs.len() - 200;
                    setup.logs.drain(..excess);
                }
                invalidated.0 = true;
            }
            SetupEvent::Complete(result) => {
                setup.receiver = None;
                setup.progress = None;
                session.config = AppConfig::load();
                session.notice = Some(match result {
                    Ok(()) => "Analysis runtime setup completed.".to_string(),
                    Err(error) => format!("Analysis runtime setup failed: {error}"),
                });
                invalidated.0 = true;
            }
        }
    }
}

pub(crate) fn poll_native_diagnostics(
    mut diagnostics: ResMut<NativeDiagnostics>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let result = diagnostics
        .receiver
        .as_ref()
        .and_then(|receiver| match receiver.lock() {
            Ok(receiver) => match receiver.try_recv() {
                Ok(report) => Some(Ok(report)),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => Some(Err(
                    "Feature diagnostics worker exited unexpectedly.".to_string(),
                )),
            },
            Err(_) => Some(Err(
                "Feature diagnostics status channel was poisoned.".to_string()
            )),
        });
    let Some(result) = result else {
        return;
    };
    diagnostics.receiver = None;
    match result {
        Ok(report) => {
            session.notice = Some(format!(
                "Diagnostics {}: {} passed, {} failed, {} skipped.",
                if report.ok { "passed" } else { "completed" },
                report.passed,
                report.failed,
                report.skipped,
            ));
            session.diagnostic_report = Some(report);
        }
        Err(error) => session.notice = Some(error),
    }
    invalidated.0 = true;
}

pub(crate) fn handle_settings_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut session: ResMut<StudioSession>,
    mut contents: Query<(&ComputedNode, &mut ScrollPosition), With<SettingsContent>>,
) {
    if session.route != StudioRoute::Settings {
        wheel.clear();
        return;
    }
    let Ok((computed, mut position)) = contents.single_mut() else {
        wheel.clear();
        return;
    };
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 22.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
    let tab_index = session.settings_tab.index();
    session.settings_scroll_offsets[tab_index] = position.y;
}
