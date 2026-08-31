//! Stage-lane presentation helpers. Counts come directly from persisted
//! `ExecutionPolicy`; add/restore controls retain their existing commands.

use super::*;

#[derive(Clone, Copy)]
pub(super) struct StageCardStats {
    pub(super) total: usize,
    pub(super) enabled: usize,
    pub(super) conditional: usize,
    pub(super) disabled: usize,
}

impl StageCardStats {
    pub(super) fn from_nodes(nodes: &[&app_core::WorkflowNodeInstance]) -> Self {
        let total = nodes.len();
        let enabled = nodes
            .iter()
            .filter(|node| node.execution_policy == app_core::ExecutionPolicy::Always)
            .count();
        let disabled = nodes
            .iter()
            .filter(|node| node.execution_policy == app_core::ExecutionPolicy::Disabled)
            .count();
        let conditional = total.saturating_sub(enabled).saturating_sub(disabled);
        Self {
            total,
            enabled,
            conditional,
            disabled,
        }
    }
}

fn spawn_stat_chip(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    label: impl Into<String>,
    color: Color,
) {
    parent
        .spawn((
            Node {
                min_height: px(20),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(7), px(2)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color.with_alpha(0.08)),
            BorderColor::all(color.with_alpha(0.22)),
        ))
        .with_children(|chip| spawn_text(chip, font, label, 6.8, color));
}

pub(super) fn spawn_stage_header(
    lane: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stage: u8,
    title: &str,
    description: &str,
    stats: Option<StageCardStats>,
) {
    lane.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::new(px(2), px(2), px(2), px(8)),
            row_gap: px(6),
            border: UiRect::bottom(px(1)),
            ..default()
        },
        BorderColor::all(theme.border.with_alpha(0.38)),
    ))
    .with_children(|header| {
        header
            .spawn(Node {
                width: percent(100),
                min_width: px(0),
                align_items: AlignItems::Center,
                column_gap: px(9),
                ..default()
            })
            .with_children(|title_row| {
                title_row
                    .spawn((
                        Node {
                            width: px(28),
                            height: px(28),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.12)),
                        BorderColor::all(theme.primary.with_alpha(0.28)),
                    ))
                    .with_children(|badge| {
                        spawn_text(
                            badge,
                            font.clone(),
                            format!("{stage:02}"),
                            9.0,
                            theme.primary,
                        );
                    });
                title_row
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(1),
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_wrapped_text(copy, font.clone(), title, 11.5, theme.foreground);
                        spawn_wrapped_text(
                            copy,
                            font.clone(),
                            description,
                            7.8,
                            theme.muted_foreground,
                        );
                    });
            });
        if let Some(stats) = stats {
            header
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(4),
                    row_gap: px(4),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|chips| {
                    spawn_stat_chip(
                        chips,
                        font.clone(),
                        format!("{} cards", stats.total),
                        theme.muted_foreground,
                    );
                    if stats.enabled > 0 {
                        spawn_stat_chip(
                            chips,
                            font.clone(),
                            format!("{} enabled", stats.enabled),
                            theme.primary,
                        );
                    }
                    if stats.conditional > 0 {
                        spawn_stat_chip(
                            chips,
                            font.clone(),
                            format!("{} conditional", stats.conditional),
                            theme.editor_warning,
                        );
                    }
                    if stats.disabled > 0 {
                        spawn_stat_chip(
                            chips,
                            font.clone(),
                            format!("{} disabled", stats.disabled),
                            theme.muted_foreground,
                        );
                    }
                });
        }
    });
}

pub(super) fn spawn_lane_section_label(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
) {
    parent
        .spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(7),
            margin: UiRect::top(px(3)),
            ..default()
        })
        .with_children(|row| {
            spawn_text(row, font, label, 6.8, theme.muted_foreground);
            row.spawn((
                Node {
                    height: px(1),
                    flex_grow: 1.0,
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.32)),
            ));
        });
}

/// Pins a truthful stage hand-off summary to the bottom of a full-height lane.
/// The flexible spacer absorbs unused height; the footer describes persisted
/// workflow semantics only and never implies runtime/model readiness.
pub(super) fn spawn_stage_footer(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: &str,
    detail: &str,
) {
    parent.spawn(Node {
        min_height: px(16),
        flex_grow: 1.0,
        ..default()
    });
    parent
        .spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::new(px(2), px(2), px(9), px(3)),
                row_gap: px(3),
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.08)),
            BorderColor::all(theme.border.with_alpha(0.36)),
        ))
        .with_children(|footer| {
            footer
                .spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    align_items: AlignItems::Center,
                    column_gap: px(7),
                    ..default()
                })
                .with_children(|heading| {
                    heading.spawn((
                        Node {
                            width: px(16),
                            height: px(2),
                            flex_shrink: 0.0,
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.primary.with_alpha(0.72)),
                    ));
                    spawn_text(heading, font.clone(), label, 6.5, theme.muted_foreground);
                });
            spawn_wrapped_text(footer, font.clone(), value, 8.4, theme.foreground);
            spawn_wrapped_text(footer, font, detail, 6.8, theme.muted_foreground);
        });
}

pub(super) fn spawn_quiet_add_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(0),
                max_width: percent(100),
                min_height: px(28),
                flex_basis: px(118),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.16)),
            BorderColor::all(theme.border.with_alpha(0.32)),
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 7.8, theme.muted_foreground);
        });
}

fn spawn_quiet_present_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                max_width: percent(100),
                min_height: px(28),
                flex_basis: px(118),
                flex_grow: 1.0,
                flex_shrink: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.primary.with_alpha(0.045)),
            BorderColor::all(theme.primary.with_alpha(0.18)),
            Pickable::IGNORE,
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 7.8, theme.primary.with_alpha(0.78));
        });
}

pub(super) fn spawn_compact_toggle_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    title: &str,
    description: &str,
    enabled: bool,
    action: UiAction,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                align_items: AlignItems::Center,
                column_gap: px(8),
                padding: UiRect::axes(px(9), px(7)),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.2)),
            BorderColor::all(theme.border.with_alpha(0.34)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(2),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), title, 8.5, theme.foreground);
                spawn_wrapped_text(copy, font.clone(), description, 7.0, theme.muted_foreground);
            });
            let color = if enabled {
                theme.primary
            } else {
                theme.muted_foreground
            };
            row.spawn((
                Button,
                action,
                Node {
                    min_width: px(52),
                    min_height: px(26),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    padding: UiRect::horizontal(px(9)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color.with_alpha(if enabled { 0.14 } else { 0.06 })),
                BorderColor::all(color.with_alpha(if enabled { 0.42 } else { 0.24 })),
            ))
            .with_children(|toggle| {
                spawn_text(
                    toggle,
                    font.clone(),
                    if enabled { "ON" } else { "OFF" },
                    7.5,
                    color,
                );
            });
        });
}

pub(super) fn optional_card_add_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    definition: &app_core::WorkflowDefinition,
    source: &(app_core::WorkflowNodeId, String),
    card: app_core::OptionalWorkflowCardV1,
) {
    if app_core::workflow_has_optional_card(definition, card) {
        spawn_quiet_present_button(parent, font, theme, format!("✓ {} · present", card.label()));
    } else {
        spawn_quiet_add_button(
            parent,
            font,
            theme,
            format!("+ {}", card.label()),
            UiAction::from(AnalysisCommand::AddOptionalWorkflowCard(
                source.0.to_string(),
                source.1.clone(),
                card,
            )),
        );
    }
}
