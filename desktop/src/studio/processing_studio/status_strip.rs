//! Compact workflow status bar: revision/quality/local-compile state, the
//! session notice, and Re-run. Presentation only -- every fact rendered here
//! already existed on the page; this module only gives it a shared, clearly
//! bordered container instead of loose top-level text lines.

use super::*;

/// Local compile validity is never conflated with runtime/Engine readiness:
/// the compiled-state line always states both facts explicitly, and a
/// compile error gets a low-intensity `theme.destructive` tint rather than
/// relying on color alone (the text itself also changes).
pub(super) fn spawn_workflow_status_strip(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    stored: &app_core::StoredWorkflow,
) {
    let has_error = session.workflow_compile_error.is_some();
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(10), px(8)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(if has_error {
                theme.destructive.with_alpha(0.08)
            } else {
                theme.card.with_alpha(STUDIO_CARD_BACKGROUND_ALPHA)
            }),
            BorderColor::all(if has_error {
                theme.destructive.with_alpha(0.4)
            } else {
                theme.border.with_alpha(STUDIO_CARD_BORDER_ALPHA)
            }),
        ))
        .with_children(|strip| {
            if let Some(error) = session.workflow_compile_error.as_ref() {
                spawn_wrapped_text(strip, font.clone(), error, 9.0, theme.destructive);
            } else {
                spawn_wrapped_text(
                    strip,
                    font.clone(),
                    format!(
                        "Workflow revision {} · {:?} · local compile valid; execution still requires exact Engine preview",
                        stored.definition.revision, stored.definition.quality_mode
                    ),
                    9.0,
                    theme.primary,
                );
            }
            let notice = session.notice.as_deref().filter(|notice| !notice.is_empty());
            strip
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    justify_content: if notice.is_some() {
                        JustifyContent::SpaceBetween
                    } else {
                        JustifyContent::FlexEnd
                    },
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|row| {
                    if let Some(notice) = notice {
                        row.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            ..default()
                        })
                        .with_children(|notice_slot| {
                            spawn_wrapped_text(notice_slot, font.clone(), notice, 9.0, theme.foreground);
                        });
                    }
                    spawn_compact_action_button(
                        row,
                        font.clone(),
                        theme,
                        "Re-run",
                        UiAction::from(AnalysisCommand::RunWorkflow),
                    );
                });
        });
}
