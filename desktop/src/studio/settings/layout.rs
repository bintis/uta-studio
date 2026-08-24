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
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                margin: UiRect::top(px(16)),
                column_gap: px(24),
                row_gap: px(12),
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
                    min_width: px(260),
                    flex_basis: px(420),
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
                    min_width: px(180),
                    max_width: px(SETTINGS_CONTROL_WIDTH),
                    flex_basis: px(SETTINGS_CONTROL_WIDTH),
                    flex_grow: 1.0,
                    margin: UiRect::top(px(2)),
                    align_items: AlignItems::FlexStart,
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
