//! Truthful Processing Studio status summary. This surface presents only the
//! editable workflow revision, local compile result and the exact-preview
//! boundary; runtime/model readiness remains owned by Plan Preview.

use super::*;

fn spawn_status_badge(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    label: &str,
    color: Color,
) {
    parent
        .spawn((
            Node {
                flex_shrink: 0.0,
                min_height: px(26),
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(9), px(4)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color.with_alpha(0.11)),
            BorderColor::all(color.with_alpha(0.34)),
        ))
        .with_children(|badge| spawn_text(badge, font, label, 7.5, color));
}

fn spawn_meta_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: impl Into<String>,
) {
    parent
        .spawn(Node {
            min_width: px(76),
            flex_direction: FlexDirection::Column,
            row_gap: px(1),
            ..default()
        })
        .with_children(|item| {
            spawn_text(item, font.clone(), label, 6.5, theme.muted_foreground);
            spawn_text(item, font, value, 9.0, theme.foreground);
        });
}

pub(super) fn spawn_workflow_status_strip(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    stored: &app_core::StoredWorkflow,
) {
    let has_error = session.workflow_compile_error.is_some();
    let status_color = if has_error {
        theme.destructive
    } else {
        theme.primary
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(12), px(8)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(if has_error {
                theme.destructive.with_alpha(0.055)
            } else {
                theme.card.with_alpha(0.22)
            }),
            BorderColor::all(if has_error {
                theme.destructive.with_alpha(0.34)
            } else {
                theme.border.with_alpha(0.42)
            }),
        ))
        .with_children(|strip| {
            strip
                .spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    min_height: px(36),
                    align_items: AlignItems::Center,
                    column_gap: px(14),
                    ..default()
                })
                .with_children(|row| {
                    spawn_status_badge(
                        row,
                        font.clone(),
                        if has_error {
                            "COMPILE ERROR"
                        } else {
                            "LOCAL COMPILE VALID"
                        },
                        status_color,
                    );
                    spawn_meta_item(
                        row,
                        font.clone(),
                        theme,
                        "REVISION",
                        stored.definition.revision.to_string(),
                    );
                    spawn_meta_item(
                        row,
                        font.clone(),
                        theme,
                        "QUALITY",
                        format!("{:?}", stored.definition.quality_mode),
                    );
                    row.spawn((
                        Node {
                            width: px(1),
                            height: px(26),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        BackgroundColor(theme.border.with_alpha(0.52)),
                    ));
                    row.spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|copy| {
                        if let Some(error) = session.workflow_compile_error.as_ref() {
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                error,
                                9.0,
                                theme.destructive,
                            );
                        } else {
                            spawn_text(
                                copy,
                                font.clone(),
                                "Workflow topology is locally valid",
                                9.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                copy,
                                font.clone(),
                                "Provider, backend and resource readiness are resolved only in exact Plan Preview.",
                                7.5,
                                theme.muted_foreground,
                            );
                        }
                    });
                    spawn_status_badge(
                        row,
                        font.clone(),
                        "EXACT PREVIEW REQUIRED",
                        theme.muted_foreground,
                    );
                });
            let notice = session.notice.as_deref().filter(|notice| !notice.is_empty());
            strip
                .spawn((
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        align_items: AlignItems::Center,
                        justify_content: if notice.is_some() {
                            JustifyContent::SpaceBetween
                        } else {
                            JustifyContent::FlexEnd
                        },
                        column_gap: px(10),
                        padding: UiRect::new(px(9), px(0), px(5), px(0)),
                        border: UiRect::top(px(1)),
                        ..default()
                    },
                    BorderColor::all(theme.border.with_alpha(0.34)),
                ))
                .with_children(|action_row| {
                    if let Some(notice) = notice {
                        action_row
                            .spawn(Node {
                                min_width: px(0),
                                flex_grow: 1.0,
                                ..default()
                            })
                            .with_children(|notice_slot| {
                                spawn_wrapped_text(
                                    notice_slot,
                                    font.clone(),
                                    notice,
                                    8.0,
                                    theme.foreground,
                                );
                            });
                    }
                    spawn_compact_action_button(
                        action_row,
                        font.clone(),
                        theme,
                        "Re-run",
                        UiAction::from(AnalysisCommand::RunWorkflow),
                    );
                });
        });
}
