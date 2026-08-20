use super::*;
use crate::studio::*;

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
                            UiAction::from(SettingsCommand::CancelSetup),
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
                            UiAction::from(SettingsCommand::ConfirmSetup),
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
                            UiAction::from(SettingsCommand::CancelClearCache),
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
                            UiAction::from(SettingsCommand::ConfirmClearCache),
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
