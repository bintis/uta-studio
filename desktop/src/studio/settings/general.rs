use super::*;
use crate::studio::*;

pub(crate) fn spawn_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
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
                        "Documentation",
                        10.0,
                        UiAction::from(AppCommand::Documentation),
                    );
                    spawn_text_button(
                        nav,
                        font.clone(),
                        theme,
                        "About Uta Studio",
                        10.0,
                        UiAction::from(AppCommand::OpenAbout),
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
                                    cache_stats,
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
            UiAction::from(SettingsCommand::SettingsTab(tab)),
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
    session: &StudioSessionView<'_>,
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
        UiAction::from(SettingsCommand::ToggleTheme),
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
        UiAction::from(AppCommand::ToggleFullscreen),
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
        "User guide",
        "Open the built-in offline documentation center. F1 opens context help from the current workspace.",
        Some(("Open user guide", UiAction::from(AppCommand::Documentation))),
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Application log",
        "Review recent events when analysis, editing, or export needs troubleshooting.",
        Some(("View log", UiAction::from(AppCommand::OpenLog))),
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Feature API diagnostics",
        "Verify local APIs, native audio, and real UTZ/UltraStar exports in a unique temporary folder that is always removed.",
        Some(("Run checks", UiAction::from(AppCommand::RunDiagnostics))),
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
        UiAction::from(SettingsCommand::AdjustUiFontScale(-1)),
        UiAction::from(SettingsCommand::AdjustUiFontScale(1)),
    );
    if let Some(report) = session.diagnostic_report.as_ref() {
        spawn_diagnostics_report(parent, font.clone(), session.config, theme, report);
    }
}

pub(crate) fn spawn_diagnostics_report(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    config: &AppConfig,
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
                localized_message(
                    config,
                    UiMessage::DiagnosticsSummary,
                    &[
                        ("{passed}", &report.passed.to_string()),
                        ("{failed}", &report.failed.to_string()),
                        ("{skipped}", &report.skipped.to_string()),
                        ("{apis}", &report.capabilities.to_string()),
                    ],
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

pub(crate) fn diagnostic_status_color(status: &str, theme: &StudioTheme) -> Color {
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
