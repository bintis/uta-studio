use super::*;
use crate::studio::*;

pub(crate) fn spawn_storage_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
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
            (
                "Choose…".to_string(),
                UiAction::from(LibraryCommand::ChooseExportFolder),
            ),
            (
                "Use system default".to_string(),
                UiAction::from(LibraryCommand::ClearExportFolder),
            ),
        ],
    );
    spawn_storage_usage_row(parent, font.clone(), session.config, theme, cache_stats);
}

pub(crate) fn spawn_storage_usage_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    config: &AppConfig,
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
                    localized_message(
                        config,
                        UiMessage::LatestScan,
                        &[("{size}", &format_bytes(total))],
                    ),
                )
            }
            (Some(stats), true) => {
                let total = stats.songs_bytes + stats.models_bytes + stats.other_bytes;
                (
                    "Recalculating",
                    theme.primary,
                    localized_message(
                        config,
                        UiMessage::CacheRecalculating,
                        &[("{size}", &format_bytes(total))],
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
        status_description =
            localized_message(config, UiMessage::CacheStatsFailed, &[("{error}", error)]);
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
                                "Cached stems, charts, previews, and temporary authoring files. Model lifecycle remains in Models & runtime.",
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
                        vec![(
                            "Clear generated cache".to_string(),
                            UiAction::from(SettingsCommand::RequestClearCache(
                                CacheClearScope::Generated,
                            )),
                        )],
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
    session: &StudioSessionView<'_>,
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
                            ("Add folder…".to_string(), UiAction::from(LibraryCommand::ChooseFolder)),
                            ("Rescan all".to_string(), UiAction::from(LibraryCommand::RescanLibrary)),
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
                                        UiAction::from(LibraryCommand::RequestRemoveFolder(path.clone())),
                                    );
                                });
                        });
                }
            }
        });
}
