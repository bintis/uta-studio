//! Compact context status bar above the DAG canvas (§1). Every fact shown
//! here is read directly from the same `authoritative_render_graph` and
//! live/history task data the canvas itself renders from -- nothing here
//! derives readiness, availability, or a success rate.

use crate::studio::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AnalysisGraphMode {
    Live,
    History,
    Draft,
}

impl AnalysisGraphMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Live => "Live execution",
            Self::History => "Historical run",
            Self::Draft => "Current workflow preview",
        }
    }
}

/// Mirrors the exact `history_task`/`active_task` precedence
/// `spawn_analysis_session_surface` already uses to pick a source snapshot
/// (`select_context_snapshot`), so the displayed mode always matches the
/// data actually rendered.
pub(crate) fn analysis_graph_mode(
    viewing_history: bool,
    has_active_task: bool,
) -> AnalysisGraphMode {
    if viewing_history {
        AnalysisGraphMode::History
    } else if has_active_task {
        AnalysisGraphMode::Live
    } else {
        AnalysisGraphMode::Draft
    }
}

/// Follow (§10) is only meaningful while a live run can actually change
/// which node is running -- History is a frozen past run and Draft has no
/// active task at all, so both read as unavailable rather than "off".
pub(crate) fn analysis_graph_follow_available(mode: AnalysisGraphMode) -> bool {
    mode == AnalysisGraphMode::Live
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct AnalysisGraphNodeCounts {
    pub(crate) complete: usize,
    pub(crate) running: usize,
    pub(crate) waiting: usize,
    pub(crate) deferred: usize,
    pub(crate) failed: usize,
    pub(crate) not_requested: usize,
}

/// Tally of the exact `GraphNodeState` already computed for every rendered
/// node. `Deferred` stays distinct from Waiting: a conditional node that was
/// not triggered in a completed run is not unfinished work. `Cancelled`
/// groups with Failed (did not complete); `Disabled`/`ProfileSkipped` group
/// with `NotRequested` (not part of this exact run). No new status is
/// invented; the bar mirrors the existing real states.
pub(crate) fn analysis_graph_node_counts(nodes: &[RenderNode]) -> AnalysisGraphNodeCounts {
    let mut counts = AnalysisGraphNodeCounts::default();
    for node in nodes {
        match node.state {
            GraphNodeState::Complete => counts.complete += 1,
            GraphNodeState::Running => counts.running += 1,
            GraphNodeState::Waiting => counts.waiting += 1,
            GraphNodeState::Deferred => counts.deferred += 1,
            GraphNodeState::Failed | GraphNodeState::Cancelled => counts.failed += 1,
            GraphNodeState::NotRequested
            | GraphNodeState::Disabled
            | GraphNodeState::ProfileSkipped => counts.not_requested += 1,
        }
    }
    counts
}

fn spawn_context_bar_chip(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    emphasized: bool,
) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(px(7.0), px(3.0)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(if emphasized {
                theme.primary.with_alpha(0.14)
            } else {
                theme.background.with_alpha(0.3)
            }),
            BorderColor::all(if emphasized {
                theme.primary.with_alpha(0.5)
            } else {
                theme.border.with_alpha(0.4)
            }),
        ))
        .with_children(|chip| {
            spawn_text(
                chip,
                font,
                label,
                8.5,
                if emphasized {
                    theme.primary
                } else {
                    theme.foreground
                },
            );
        });
}

fn spawn_context_bar_status_chip(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: usize,
    accent: Color,
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(4.0),
            ..default()
        })
        .with_children(|item| {
            item.spawn((
                Node {
                    width: px(7.0),
                    height: px(7.0),
                    flex_shrink: 0.0,
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent),
            ));
            spawn_text(
                item,
                font,
                format!("{label} {value}"),
                8.0,
                theme.muted_foreground,
            );
        });
}

/// The DAG page's own top-of-canvas status strip. Song title and overall
/// analysis progress remain in the existing page header above this; this
/// bar only adds mode, current activity, and real per-node counts.
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_graph_context_bar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    mode: AnalysisGraphMode,
    overall_progress: Option<usize>,
    current_label: Option<&str>,
    counts: AnalysisGraphNodeCounts,
    zoom: f32,
    follow_active: bool,
    follow_available: bool,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4.0),
                padding: UiRect::axes(px(10.0), px(7.0)),
                border: UiRect::all(px(1.0)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(STUDIO_CARD_BACKGROUND_ALPHA)),
            BorderColor::all(theme.border.with_alpha(STUDIO_CARD_BORDER_ALPHA)),
        ))
        .with_children(|strip| {
            strip
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10.0),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|row| {
                    spawn_context_bar_chip(
                        row,
                        font.clone(),
                        theme,
                        mode.label(),
                        mode == AnalysisGraphMode::Live,
                    );
                    if let Some(progress) = overall_progress {
                        spawn_text(
                            row,
                            font.clone(),
                            format!("{progress}%"),
                            9.0,
                            theme.foreground,
                        );
                    }
                    if let Some(label) = current_label {
                        row.spawn(Node {
                            min_width: px(0.0),
                            flex_shrink: 1.0,
                            ..default()
                        })
                        .with_children(|slot| {
                            spawn_bounded_wrapped_text(
                                slot,
                                font.clone(),
                                label,
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    }
                    row.spawn(Node {
                        flex_grow: 1.0,
                        min_width: px(8.0),
                        ..default()
                    });
                });
            strip
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10.0),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(4.0),
                    ..default()
                })
                .with_children(|footer| {
                    footer
                        .spawn(Node {
                            flex_grow: 1.0,
                            min_width: px(0.0),
                            ..default()
                        })
                        .with_children(|slot| {
                            spawn_text(
                                slot,
                                font.clone(),
                                "Read-only execution graph. Edit the workflow in Processing Studio.",
                                7.5,
                                theme.muted_foreground.with_alpha(0.75),
                            );
                        });
                    // Same fixed status colors `spawn_workflow_graph_node`
                    // assigns to every card regardless of its own category,
                    // so this key and the canvas always agree. This replaces
                    // the DAG canvas's old floating color-key overlay: it
                    // lived on top of the graph itself, which crowded the
                    // canvas at high transparency. Sharing the footer row
                    // with Fit/Zoom/Follow (rather than the mode/progress
                    // row above) keeps that row from wrapping the live
                    // operation label on top of the counts on a narrow
                    // window -- confirmed against a real screenshot where
                    // it did exactly that.
                    let complete_accent =
                        analysis_graph_category_accent(GraphNodeCategory::Output, theme);
                    for (label, value, accent) in [
                        ("Complete", counts.complete, complete_accent),
                        ("Running", counts.running, theme.primary),
                        ("Waiting", counts.waiting, theme.muted_foreground),
                        ("Deferred", counts.deferred, theme.editor_warning),
                        ("Failed", counts.failed, theme.destructive),
                        (
                            "Not requested",
                            counts.not_requested,
                            theme.muted_foreground.with_alpha(0.6),
                        ),
                    ] {
                        spawn_context_bar_status_chip(
                            footer,
                            font.clone(),
                            theme,
                            label,
                            value,
                            accent,
                        );
                    }
                    spawn_analysis_graph_viewport_controls(
                        footer,
                        font,
                        theme,
                        zoom,
                        follow_active,
                        follow_available,
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(state: GraphNodeState) -> RenderNode {
        RenderNode {
            id: app_core::AnalysisNodeId::new("n"),
            kind: RenderNodeKind::Compute,
            label: String::new(),
            model_ids: Vec::new(),
            detail: String::new(),
            state,
            category: GraphNodeCategory::Audio,
            capability_id: None,
            terminal_outputs: Vec::new(),
            members: Vec::new(),
        }
    }

    #[test]
    fn mode_prefers_history_over_an_active_task() {
        assert_eq!(analysis_graph_mode(true, true), AnalysisGraphMode::History);
        assert_eq!(analysis_graph_mode(true, false), AnalysisGraphMode::History);
        assert_eq!(analysis_graph_mode(false, true), AnalysisGraphMode::Live);
        assert_eq!(analysis_graph_mode(false, false), AnalysisGraphMode::Draft);
    }

    #[test]
    fn follow_is_available_only_in_live_mode() {
        assert!(analysis_graph_follow_available(AnalysisGraphMode::Live));
        assert!(!analysis_graph_follow_available(AnalysisGraphMode::History));
        assert!(!analysis_graph_follow_available(AnalysisGraphMode::Draft));
    }

    #[test]
    fn counts_bucket_every_real_state_without_inventing_one() {
        let nodes = vec![
            node(GraphNodeState::Complete),
            node(GraphNodeState::Complete),
            node(GraphNodeState::Running),
            node(GraphNodeState::Waiting),
            node(GraphNodeState::Deferred),
            node(GraphNodeState::Failed),
            node(GraphNodeState::Cancelled),
            node(GraphNodeState::NotRequested),
            node(GraphNodeState::Disabled),
            node(GraphNodeState::ProfileSkipped),
        ];
        let counts = analysis_graph_node_counts(&nodes);
        assert_eq!(counts.complete, 2);
        assert_eq!(counts.running, 1);
        assert_eq!(counts.waiting, 1);
        assert_eq!(counts.deferred, 1);
        assert_eq!(counts.failed, 2);
        assert_eq!(counts.not_requested, 3);
        assert_eq!(
            counts.complete
                + counts.running
                + counts.waiting
                + counts.deferred
                + counts.failed
                + counts.not_requested,
            nodes.len()
        );
    }
}
