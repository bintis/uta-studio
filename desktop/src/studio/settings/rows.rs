use super::*;
use crate::studio::*;

#[expect(
    clippy::too_many_arguments,
    reason = "the row builder keeps its label, description, selection kind, and UI assets explicit"
)]
pub(crate) fn spawn_select_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    kind: SettingsSelectKind,
    session: &StudioSessionView<'_>,
) {
    let label = label.into();
    let description = description.into();
    let current = settings_select_value(kind, session.config);
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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
            ZIndex(if open { 60 } else { 0 }),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
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
                min_width: px(180),
                max_width: px(SETTINGS_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_CONTROL_WIDTH),
                flex_grow: 1.0,
                height: if open { Val::Auto } else { px(36) },
                margin: UiRect::top(px(2)),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|control| {
                control
                    .spawn((
                        Button,
                        UiAction::from(SettingsCommand::OpenSettingsSelect(kind)),
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
                                    UiAction::from(SettingsCommand::SelectSettingsValue(
                                        kind,
                                        (*value).to_string(),
                                    )),
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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
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
                    min_width: px(180),
                    max_width: px(SETTINGS_CONTROL_WIDTH),
                    flex_basis: px(SETTINGS_CONTROL_WIDTH),
                    flex_grow: 1.0,
                    margin: UiRect::top(px(2)),
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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
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
            min_width: px(180),
            max_width: px(SETTINGS_CONTROL_WIDTH),
            flex_basis: px(SETTINGS_CONTROL_WIDTH),
            flex_grow: 1.0,
            margin: UiRect::top(px(2)),
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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
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
                margin: UiRect::top(px(2)),
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
                    UiAction::from(LibraryCommand::OpenSource(path.to_path_buf())),
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

#[cfg(any())]
pub(crate) fn sync_numeric_settings(
    mut inputs: Query<(&mut EditableText, &NumericSetting)>,
    mut shell: ResMut<ShellState>,
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
            NumericSetting::VocalThreshold => (0, 60),
            NumericSetting::SeparatorSegmentSize => (64, 1024),
            NumericSetting::SeparatorOverlap => (2, 32),
            NumericSetting::SeparatorBatchSize => (1, 8),
            NumericSetting::SeparatorNormalization => (1, 100),
            NumericSetting::AsrBeamSize => (1, 64),
            NumericSetting::AsrBatchSize => (1, 32),
        };
        let clamped = parsed.clamp(minimum, maximum);
        if clamped != parsed {
            input.editor_mut().set_text(&clamped.to_string());
        }
        let current = match setting {
            NumericSetting::VocalThreshold => {
                (shell.config.vocal_detection_threshold_pct() * 100.0).round() as u32
            }
            NumericSetting::SeparatorSegmentSize => shell.config.separator_segment_size(),
            NumericSetting::SeparatorOverlap => shell.config.separator_overlap(),
            NumericSetting::SeparatorBatchSize => shell.config.separator_batch_size(),
            NumericSetting::SeparatorNormalization => shell.config.separator_normalization_pct(),
            NumericSetting::AsrBeamSize => shell.config.beam_size(),
            NumericSetting::AsrBatchSize => shell.config.batch_size(),
        };
        if clamped == current {
            continue;
        }
        match setting {
            NumericSetting::VocalThreshold => {
                shell.config.vocal_detection_threshold_pct = Some(f64::from(clamped) / 100.0)
            }
            NumericSetting::SeparatorSegmentSize => {
                shell.config.separator_segment_size = Some(clamped)
            }
            NumericSetting::SeparatorOverlap => shell.config.separator_overlap = Some(clamped),
            NumericSetting::SeparatorBatchSize => shell.config.separator_batch_size = Some(clamped),
            NumericSetting::SeparatorNormalization => {
                shell.config.separator_normalization_pct = Some(clamped)
            }
            NumericSetting::AsrBeamSize => shell.config.beam_size = Some(clamped),
            NumericSetting::AsrBatchSize => shell.config.batch_size = Some(clamped),
        }
        if let Some(error) = save_config_error(&shell.config) {
            shell.notice = Some(error);
        }
    }
}
