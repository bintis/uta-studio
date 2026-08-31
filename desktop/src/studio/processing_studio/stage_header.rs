//! Stage lane heading (number + title + one-line purpose + real card stats)
//! and the quiet Add/Restore action row used by stages 01-03. Presentation
//! only: stats are computed directly from `ExecutionPolicy`, never a
//! fabricated readiness/success metric, and Add/Restore still dispatch the
//! existing `AnalysisCommand::AddOptionalWorkflowCard`.

use super::*;

/// Real per-stage card counts by `ExecutionPolicy`, computed directly from
/// the stage's own node list -- never a readiness/success percentage.
pub(super) struct StageCardStats {
    total: usize,
    enabled: usize,
    conditional: usize,
    disabled: usize,
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

pub(super) fn spawn_stage_header(
    lane: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    heading: &str,
    description: &str,
    stats: Option<StageCardStats>,
) {
    spawn_text(lane, font.clone(), heading, 13.0, theme.primary);
    spawn_wrapped_text(lane, font.clone(), description, 9.0, theme.muted_foreground);
    if let Some(stats) = stats {
        spawn_wrapped_text(
            lane,
            font,
            format!(
                "{} cards · {} enabled · {} conditional · {} disabled",
                stats.total, stats.enabled, stats.conditional, stats.disabled
            ),
            8.0,
            theme.muted_foreground.with_alpha(0.85),
        );
    }
}

/// Quieter, more compact than `action_button` so a row of Add/Restore
/// actions does not out-weigh the capability cards it sits above.
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
                min_height: px(26),
                flex_shrink: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(9), px(5)),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.18)),
            BorderColor::all(theme.border.with_alpha(0.34)),
        ))
        .with_children(|button| {
            spawn_wrapped_text(button, font, label, 8.0, theme.muted_foreground);
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
        disabled_action_button(parent, font, theme, format!("✓ {} · present", card.label()));
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
