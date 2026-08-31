use super::*;
use crate::studio::*;

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
            flex_shrink: 0.0,
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

pub(crate) fn spawn_settings_group(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    title: impl Into<String>,
    description: impl Into<String>,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let title = title.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.24)),
            BorderColor::all(theme.border.with_alpha(0.46)),
        ))
        .with_children(|card| {
            card.spawn((
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(18), px(12)),
                    row_gap: px(3),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.12)),
                BorderColor::all(theme.border.with_alpha(0.34)),
            ))
            .with_children(|header| {
                spawn_text(header, font.clone(), title, 8.0, theme.primary);
                spawn_wrapped_text(
                    header,
                    font.clone(),
                    description,
                    9.0,
                    theme.muted_foreground,
                );
            });
            card.spawn(Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(build);
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_settings_stage_group(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: impl Into<String>,
    title: impl Into<String>,
    description: impl Into<String>,
    current: impl Into<String>,
    status: Option<(String, bool)>,
    action: Option<(String, UiAction)>,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    let eyebrow = eyebrow.into();
    let title = title.into();
    let description = description.into();
    let current = current.into();
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.24)),
            BorderColor::all(theme.border.with_alpha(0.48)),
        ))
        .with_children(|card| {
            card.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(0),
                    bottom: px(0),
                    width: px(3),
                    ..default()
                },
                BackgroundColor(theme.primary.with_alpha(0.58)),
                Pickable::IGNORE,
            ));
            card.spawn((
                Node {
                    width: percent(100),
                    min_height: px(84),
                    flex_shrink: 0.0,
                    align_items: AlignItems::FlexStart,
                    flex_wrap: FlexWrap::Wrap,
                    padding: UiRect::new(px(20), px(18), px(15), px(14)),
                    column_gap: px(24),
                    row_gap: px(10),
                    border: UiRect::bottom(px(1)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.1)),
                BorderColor::all(theme.border.with_alpha(0.36)),
            ))
            .with_children(|header| {
                header
                    .spawn(Node {
                        min_width: px(260),
                        flex_basis: px(420),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(3),
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), eyebrow, 7.8, theme.primary);
                        spawn_text(copy, font.clone(), title, 13.0, theme.foreground);
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
                        min_width: px(180),
                        max_width: px(SETTINGS_CONTROL_WIDTH),
                        flex_basis: px(SETTINGS_CONTROL_WIDTH),
                        flex_grow: 1.0,
                        margin: UiRect::top(px(2)),
                        align_items: AlignItems::FlexEnd,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(7),
                        ..default()
                    })
                    .with_children(|summary| {
                        spawn_wrapped_text(summary, font.clone(), current, 9.5, theme.foreground);
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
                            spawn_compact_action_button(
                                summary,
                                font.clone(),
                                theme,
                                label,
                                action,
                            );
                        }
                    });
            });
            card.spawn(Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(build);
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
            min_height: px(22),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
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
