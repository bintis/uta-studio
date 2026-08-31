//! Contextual Processing Studio sidebar. The four-stage board remains a stable
//! workflow overview while the sidebar presents either truthful workflow facts
//! or the selected module's existing controls. Runtime/model readiness remains
//! exclusively owned by exact Plan Preview.

use super::*;

const WORKFLOW_SIDEBAR_WIDTH: f32 = 276.0;

fn spawn_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    title: &str,
    subtitle: Option<&str>,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(8),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.2)),
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        padding: UiRect::bottom(px(7)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BorderColor::all(theme.border.with_alpha(0.32)),
                ))
                .with_children(|header| {
                    spawn_text(header, font.clone(), title, 8.5, theme.foreground);
                    if let Some(subtitle) = subtitle {
                        spawn_wrapped_text(
                            header,
                            font.clone(),
                            subtitle,
                            6.8,
                            theme.muted_foreground,
                        );
                    }
                });
            build(panel);
        });
}

fn spawn_stat(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: impl Into<String>,
    color: Color,
) {
    parent
        .spawn((
            Node {
                min_width: px(92),
                flex_basis: px(112),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.18)),
            BorderColor::all(theme.border.with_alpha(0.3)),
        ))
        .with_children(|stat| {
            spawn_text(stat, font.clone(), label, 6.2, theme.muted_foreground);
            spawn_wrapped_text(stat, font, value, 8.4, color);
        });
}

fn spawn_stage_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stage: u8,
    label: &str,
    nodes: &[&app_core::WorkflowNodeInstance],
) {
    let stats = stage_header::StageCardStats::from_nodes(nodes);
    let accent = stage_header::stage_accent(stage, theme);
    parent
        .spawn(Node {
            width: percent(100),
            min_width: px(0),
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(24),
                    height: px(24),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.1)),
                BorderColor::all(accent.with_alpha(0.28)),
            ))
            .with_children(|badge| {
                spawn_text(badge, font.clone(), format!("{stage:02}"), 7.2, accent);
            });
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                ..default()
            })
            .with_children(|copy| {
                spawn_wrapped_text(copy, font.clone(), label, 7.8, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    format!(
                        "{} cards · {} enabled · {} conditional · {} disabled",
                        stats.total, stats.enabled, stats.conditional, stats.disabled
                    ),
                    6.2,
                    theme.muted_foreground,
                );
            });
        });
}

fn terminal_output_label(output: &app_core::WorkflowTerminalOutputWireV1) -> String {
    match (output.semantic_type.as_str(), output.audio_role.as_deref()) {
        ("canonical_singing_track", _) => "Canonical singing track".to_string(),
        ("candidate_chart", _) => "Candidate singing chart".to_string(),
        ("pitch_evidence", _) => "Pitch evidence".to_string(),
        ("boundary_evidence", _) => "Note-boundary evidence".to_string(),
        ("alignment_evidence", _) => "Lyric alignment evidence".to_string(),
        ("audio", Some("lead_vocal")) => "Lead-vocal audio".to_string(),
        ("audio", Some("instrumental")) => "Instrumental audio".to_string(),
        ("audio", Some(role)) => format!("Audio · {role}"),
        (semantic, _) => semantic.replace('_', " "),
    }
}

fn spawn_overview(
    sidebar: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    stored: &app_core::StoredWorkflow,
    capabilities: &BTreeMap<app_core::CapabilityId, app_core::NodeCapability>,
) {
    let all_nodes = stored.definition.nodes.iter().collect::<Vec<_>>();
    let stats = stage_header::StageCardStats::from_nodes(&all_nodes);
    let active = stats.enabled + stats.conditional;

    spawn_panel(
        sidebar,
        font.clone(),
        theme,
        "WORKFLOW SUMMARY",
        Some("Select a module card to edit its provider, routing and execution condition."),
        |panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    min_width: px(0),
                    column_gap: px(6),
                    row_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    ..default()
                })
                .with_children(|stats_row| {
                    spawn_stat(
                        stats_row,
                        font.clone(),
                        theme,
                        "ACTIVE MODULES",
                        format!("{active} / {}", stats.total),
                        theme.primary,
                    );
                    spawn_stat(
                        stats_row,
                        font.clone(),
                        theme,
                        "QUALITY MODE",
                        format!("{:?}", stored.definition.quality_mode),
                        theme.foreground,
                    );
                });

            for (stage, label) in [
                (1u8, "Pre-processing"),
                (2u8, "Lyrics & transcription"),
                (3u8, "Pitch & note experts"),
            ] {
                let stage_nodes = stored
                    .definition
                    .nodes
                    .iter()
                    .filter(|node| {
                        capabilities
                            .get(&node.capability_id)
                            .is_some_and(|capability| workflow_stage(capability) == stage)
                    })
                    .collect::<Vec<_>>();
                spawn_stage_row(panel, font.clone(), theme, stage, label, &stage_nodes);
            }

            let fusion_mode = match app_core::fusion_mode(&stored.definition) {
                app_core::FusionModeV1::Algorithm => "Algorithm",
                app_core::FusionModeV1::AiJudgment => "AI judgment",
            };
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(8),
                        padding: UiRect::top(px(5)),
                        border: UiRect::top(px(1)),
                        ..default()
                    },
                    BorderColor::all(theme.border.with_alpha(0.3)),
                ))
                .with_children(|row| {
                    spawn_text(
                        row,
                        font.clone(),
                        "FINAL DECISION",
                        6.2,
                        theme.muted_foreground,
                    );
                    spawn_text(row, font.clone(), fusion_mode, 7.8, theme.primary);
                });
        },
    );

    let has_error = session.workflow_compile_error.is_some();
    spawn_panel(
        sidebar,
        font.clone(),
        theme,
        "LOCAL TOPOLOGY",
        Some(
            "Local compilation checks product topology only; runtime truth remains in exact Plan Preview.",
        ),
        |panel| {
            if let Some(error) = session.workflow_compile_error.as_deref() {
                spawn_wrapped_text(panel, font.clone(), "Compile error", 8.0, theme.destructive);
                spawn_wrapped_text(panel, font.clone(), error, 6.8, theme.destructive);
            } else {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "Local workflow topology is valid",
                    8.0,
                    theme.primary,
                );
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "Exact provider, backend and resource readiness is not inferred on this page.",
                    6.8,
                    theme.muted_foreground,
                );
            }
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(26),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        padding: UiRect::axes(px(8), px(4)),
                        border: UiRect::all(px(1)),
                        border_radius: studio_card_radius(),
                        ..default()
                    },
                    BackgroundColor(if has_error {
                        theme.destructive.with_alpha(0.07)
                    } else {
                        theme.primary.with_alpha(0.07)
                    }),
                    BorderColor::all(if has_error {
                        theme.destructive.with_alpha(0.28)
                    } else {
                        theme.primary.with_alpha(0.24)
                    }),
                ))
                .with_children(|status| {
                    spawn_text(
                        status,
                        font.clone(),
                        if has_error {
                            "FIX TOPOLOGY BEFORE PREVIEW"
                        } else {
                            "EXACT PLAN PREVIEW REQUIRED"
                        },
                        6.5,
                        if has_error {
                            theme.destructive
                        } else {
                            theme.primary
                        },
                    );
                });
        },
    );

    spawn_panel(
        sidebar,
        font.clone(),
        theme,
        "PLANNED OUTPUTS",
        Some("Terminal products from the current local compile snapshot."),
        |panel| {
            let outputs = session
                .workflow_snapshot
                .as_ref()
                .and_then(|snapshot| {
                    app_core::WorkflowExecutionWireV1::from_snapshot(snapshot).ok()
                })
                .map(|wire| wire.terminal_outputs)
                .unwrap_or_default();
            if outputs.is_empty() {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "No terminal output is available from the current local compile.",
                    6.9,
                    theme.muted_foreground,
                );
            } else {
                for output in outputs {
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                min_width: px(0),
                                flex_direction: FlexDirection::Column,
                                row_gap: px(1),
                                padding: UiRect::new(px(7), px(7), px(5), px(5)),
                                border: UiRect::left(px(2)),
                                ..default()
                            },
                            BorderColor::all(theme.primary.with_alpha(0.32)),
                        ))
                        .with_children(|row| {
                            spawn_wrapped_text(
                                row,
                                font.clone(),
                                terminal_output_label(&output),
                                7.6,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                row,
                                font.clone(),
                                format!("{} · {}", output.node, output.port),
                                6.0,
                                theme.muted_foreground,
                            );
                        });
                }
            }
        },
    );
}

pub(super) fn spawn_workflow_sidebar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    stored: &app_core::StoredWorkflow,
    capabilities: &BTreeMap<app_core::CapabilityId, app_core::NodeCapability>,
    audio_sources: &[(app_core::WorkflowNodeId, String, String)],
) {
    parent
        .spawn((
            Node {
                width: px(WORKFLOW_SIDEBAR_WIDTH),
                min_width: px(252),
                min_height: vh(60.0),
                height: percent(100),
                flex_shrink: 0.0,
                align_self: AlignSelf::Stretch,
                flex_direction: FlexDirection::Column,
                padding: UiRect::left(px(2)),
                row_gap: px(8),
                border: UiRect::left(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.28)),
        ))
        .with_children(|sidebar| {
            let selected = session.selected_workflow_node.as_ref().and_then(|selected_id| {
                stored
                    .definition
                    .nodes
                    .iter()
                    .find(|node| &node.instance_id == selected_id)
            });
            let selected = selected.and_then(|node| {
                capabilities
                    .get(&node.capability_id)
                    .map(|capability| (node, capability))
            });

            if let Some((node, capability)) = selected {
                spawn_panel(
                    sidebar,
                    font.clone(),
                    theme,
                    "MODULE SETTINGS",
                    Some("The stage board remains stable while this module is edited here. Select the same card again to close."),
                    |panel| {
                        node_card::spawn_node_card(
                            panel,
                            font.clone(),
                            theme,
                            node,
                            capability,
                            node_card::NodeCardContext {
                                selected: true,
                                expanded: true,
                                embedded: false,
                                compact: false,
                                allow_drag_reorder: false,
                                definition: &stored.definition,
                                analyzer_binding: stored
                                    .definition
                                    .analyzer_bindings
                                    .iter()
                                    .find(|binding| binding.analyzer_node == node.instance_id),
                                audio_sources,
                            },
                        );
                    },
                );
            } else {
                spawn_overview(sidebar, font, theme, session, stored, capabilities);
            }
        });
}
