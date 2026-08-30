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
    let (title, description) = if request.target.is_some() {
        (
            "Install selected native component?",
            "Uta! Studio will install only the selected audited artifact after confirmation. Existing model directories and source songs are never removed or replaced by this check.",
        )
    } else {
        (
            "Verify native runtime?",
            "Uta! Studio will verify packaged workers, the runtime lock, ffmpeg, and existing model files without downloading anything.",
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

pub(crate) fn spawn_model_downloads_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    setup: &NativeSetup,
) {
    parent
        .spawn((
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
            ZIndex(70),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    ScrollPosition::default(),
                    Node {
                        width: px(820),
                        max_width: percent(92),
                        height: vh(84.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(22)),
                        row_gap: px(12),
                        overflow: Overflow::scroll_y(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(10)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        })
                        .with_children(|header| {
                            spawn_icon(header, icons.clone(), UiIcon::Box, 18.0, theme.primary);
                            header
                                .spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(2),
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        "Model downloads",
                                        18.0,
                                        theme.foreground,
                                    );
                                    spawn_wrapped_text(
                                        copy,
                                        font.clone(),
                                        "Install and repair local model files here. This screen never changes analysis output, workflow logic, or per-song model choice.",
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                });
                            spawn_text_button(
                                header,
                                font.clone(),
                                theme,
                                "Close",
                                10.0,
                                UiAction::from(SettingsCommand::CloseModelDownloads),
                            );
                        });

                    if setup.receiver.is_some() || setup.progress.is_some() {
                        spawn_setup_progress_panel(
                            dialog,
                            font.clone(),
                            icons.clone(),
                            setup,
                            theme,
                        );
                    }

                    if let Some(snapshot) = session.model_settings_job.current.as_ref() {
                        let installed = snapshot
                            .runtime_status
                            .models
                            .iter()
                            .filter(|model| model.available)
                            .count();
                        spawn_setting_row(
                            dialog,
                            font.clone(),
                            theme,
                            "Local model status",
                            format!(
                                "{} of {} packaged model groups are available. Refresh only re-checks local files; it does not download anything.",
                                installed,
                                snapshot.runtime_status.models.len()
                            ),
                            Some((
                                "Refresh",
                                UiAction::from(SettingsCommand::RefreshRuntimeStatus),
                            )),
                        );

                        spawn_settings_section(
                            dialog,
                            font.clone(),
                            theme,
                            "PACKAGED MODELS",
                            "Required and optional packaged resources are shown together with a direct action. Repair re-runs setup only for that model group.",
                        );
                        for model in &snapshot.runtime_status.models {
                            let role = super::models::model_install_role(session.config, model.target);
                            spawn_setting_row(
                                dialog,
                                font.clone(),
                                theme,
                                model.label.clone(),
                                format!(
                                    "{} · {} · backend {} · {}",
                                    role, model.validation, model.backend, model.description
                                ),
                                Some((
                                    if model.available { "Repair" } else { "Install" },
                                    UiAction::from(SettingsCommand::RequestSetup(Some(model.target))),
                                )),
                            );
                        }

                        spawn_settings_section(
                            dialog,
                            font.clone(),
                            theme,
                            "OPTIONAL MODEL ARTIFACTS",
                            "Catalog models are optional local weights for separation and note analysis. Installing or removing one changes only local model files.",
                        );
                        if let Some(error) = snapshot.audio_catalog_error.as_deref() {
                            spawn_wrapped_text(
                                dialog,
                                font.clone(),
                                format!("Model artifact catalog unavailable: {error}"),
                                9.0,
                                theme.destructive,
                            );
                        }
                        for model in &snapshot.audio_catalog.models {
                            let backends = model.supported_backends.join(" / ");
                            spawn_setting_row(
                                dialog,
                                font.clone(),
                                theme,
                                model.display_name.clone(),
                                format!(
                                    "{} · {} · {} · {}",
                                    model.purpose,
                                    model.architecture,
                                    backends,
                                    model.license.source_attribution
                                ),
                                Some((
                                    if model.state == "installed" {
                                        "Remove"
                                    } else {
                                        "Install"
                                    },
                                    if model.state == "installed" {
                                        UiAction::from(SettingsCommand::RemoveAudioModel(
                                            model.model_id.clone(),
                                        ))
                                    } else {
                                        UiAction::from(SettingsCommand::InstallAudioModel(
                                            model.model_id.clone(),
                                        ))
                                    },
                                )),
                            );
                        }
                    } else {
                        spawn_setting_row(
                            dialog,
                            font.clone(),
                            theme,
                            "Loading model inventory…",
                            "The download manager needs the local runtime inventory before it can show model-specific actions.",
                            Some((
                                "Refresh",
                                UiAction::from(SettingsCommand::RefreshRuntimeStatus),
                            )),
                        );
                    }
                });
        });
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
            "Generated stems, charts, previews, and authoring variants will be removed. Indexed source songs and installed models remain untouched.",
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
