use super::*;
use crate::studio::*;

pub(crate) fn current_analysis_header(
    session: &StudioSessionView<'_>,
) -> Option<(String, String, usize)> {
    let active_task = session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .find(|task| matches!(task.status, app_core::QueuedStatus::Queued))
        });
    let history = session
        .selected_analysis_history
        .and_then(|id| {
            session
                .analysis_history
                .iter()
                .find(|history| history.id == id)
        })
        .or_else(|| {
            active_task
                .is_none()
                .then(|| session.analysis_history.first())
                .flatten()
        });
    if let Some(history) = history {
        let progress = if history.status == "completed" {
            100
        } else {
            0
        };
        return Some((history.title.clone(), history.artist.clone(), progress));
    }
    active_task.map(|task| {
        let progress = match &task.status {
            app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
            _ => 0,
        };
        (task.title.clone(), task.artist.clone(), progress)
    })
}

fn spawn_analysis_empty_canvas(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::all(px(28)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.22)),
        ))
        .with_children(|empty| {
            spawn_wrapped_text(
                empty,
                font,
                "Choose an unanalyzed song to start. The live graph fills this page once a run is queued.",
                13.0,
                theme.muted_foreground,
            );
        });
}

pub(crate) fn spawn_analysis_session_overview(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    spawn_analysis_session_surface(parent, font, session, theme, false);
}

pub(crate) fn spawn_analysis_inspect_surface(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    spawn_analysis_session_surface(parent, font, session, theme, true);
}

fn spawn_analysis_session_surface(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
    inspect_only: bool,
) {
    let active_task = session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .find(|task| matches!(task.status, app_core::QueuedStatus::Queued))
        });
    let history = session
        .selected_analysis_history
        .and_then(|id| {
            session
                .analysis_history
                .iter()
                .find(|history| history.id == id)
        })
        .or_else(|| {
            active_task
                .is_none()
                .then(|| session.analysis_history.first())
                .flatten()
        });
    let history_task = history.map(|history| app_core::AnalysisTask {
        file_hash: history.file_hash.clone(),
        title: history.title.clone(),
        artist: history.artist.clone(),
        status: app_core::QueuedStatus::Analyzing(if history.status == "completed" {
            100
        } else {
            0
        }),
        live: Some(history.snapshot.clone()),
    });
    let Some(task) = history_task.as_ref().or(active_task) else {
        if !inspect_only {
            spawn_analysis_empty_canvas(parent, font, theme);
        }
        return;
    };
    let viewing_history = history_task.is_some();

    let progress = match &task.status {
        app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
        _ => 0,
    };
    let stage = task
        .live
        .as_ref()
        .map(|live| live.stage.as_str())
        .unwrap_or("preparing");
    let live_node_id = task.live.as_ref().and_then(|live| live.node_id.as_deref());
    let stage_index = resolve_live_stage_index(stage, live_node_id);
    let operation = task
        .live
        .as_ref()
        .map(|live| live.operation.as_str())
        .unwrap_or("Waiting for the analysis runtime");
    let detail = task
        .live
        .as_ref()
        .map(|live| live.detail.as_str())
        .unwrap_or("The task is queued and will start when the current analysis completes.");
    let selected_stage = session.selected_analysis_stage.as_deref().unwrap_or(stage);
    let selected_stage_index = analysis_stage_index(selected_stage);
    // Real node id for the selected bucket (Phase 3/7 wire-protocol fix):
    // computed here, ahead of the plan/artifact block below that used to be
    // the only place deriving it, because `selected_route` now needs it
    // too. Every top-level compute node maps 1:1 with a bucket today (no
    // compound child is individually clickable yet -- see
    // `expanded_compound_nodes` further down), so this is already the
    // precise node id for anything actually selectable right now.
    let (selected_node_id, _) = stage_primary_node_and_artifact(selected_stage_index);
    let selected_route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, selected_node_id, selected_stage));
    let selected_is_current = analysis_stage_matches(stage, selected_stage);
    let selected_progress = selected_route
        .map(|route| route.stage_progress.clamp(0, 100))
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.stage_progress.clamp(0, 100))
                    .unwrap_or(0)
            })
        })
        .unwrap_or({
            if selected_stage_index < stage_index {
                100
            } else {
                0
            }
        });
    let selected_trace_missing = selected_route.is_none() && selected_progress >= 100;
    let selected_pending_copy = if selected_trace_missing {
        "Not recorded in this analysis session"
    } else {
        "Pending"
    };
    let (selected_label, selected_purpose, selected_input, selected_output) =
        analysis_stage_details(selected_stage);
    let selected_status = if selected_progress >= 100 {
        "COMPLETE"
    } else if selected_is_current {
        "RUNNING"
    } else if selected_stage_index < stage_index {
        "COMPLETE"
    } else {
        "WAITING"
    };
    let selected_operation = selected_route
        .map(|route| route.operation.as_str())
        .or_else(|| selected_is_current.then_some(operation))
        .unwrap_or("This step has not started yet.");
    let selected_implementation = selected_route
        .map(|route| route.implementation.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.implementation.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_model = selected_route
        .map(|route| route.model.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.model.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_requested_device = selected_route
        .map(|route| route.requested_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.requested_device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_actual_device = selected_route
        .map(|route| route.actual_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.device.as_str())
                    .unwrap_or("Pending")
            })
        })
        .unwrap_or(selected_pending_copy);
    let selected_device_fallback = selected_route.and_then(|route| {
        route
            .fallback_from
            .as_deref()
            .zip(route.fallback_reason.as_deref())
    });
    let selected_backend_fallback = selected_route.and_then(|route| {
        route
            .backend_fallback_from
            .as_deref()
            .zip(route.backend_fallback_reason.as_deref())
    });
    let history_error = history.and_then(|history| history.error_message.as_deref());

    // Real Phase 1/2 domain-model data for the selected stage, grounding
    // the inspector panel in the actual DAG plan and on-disk artifact
    // state instead of only the static per-stage copy above
    // (docs/analysis-dag-redesign.md Phase 7 "node inspector" item).
    let plan_node_id = selected_node_id;
    let (_, plan_artifact_kind) = stage_primary_node_and_artifact(selected_stage_index);
    let plan_preview = app_core::preview_full_analysis_plan(&task.file_hash)
        .ok()
        .map(|plan| {
            let attempts = history
                .map(|history| app_core::load_analysis_node_attempts(history.id))
                .unwrap_or_default();
            let plan = overlay_failed_node_attempts(plan, &attempts);
            let candidate_status = app_core::candidate_chart_status(&task.file_hash);
            overlay_stale_candidate_chart(plan, &candidate_status)
        });
    // Single real read of on-disk artifact presence for this render, reused
    // by both the inspector panel below and the DAG canvas's node/edge
    // readiness -- was previously two separate calls to the same function.
    let artifact_presence = app_core::cached_artifact_presence_for_song(&task.file_hash);
    let planned_node = plan_preview
        .as_ref()
        .and_then(|plan| plan.node(&app_core::AnalysisNodeId::new(plan_node_id)));
    let plan_state_copy = planned_node
        .map(|node| node_state_copy(node.state))
        .unwrap_or("Not planned in this run");
    let plan_will_run_copy = planned_node.map_or("Unknown", |node| {
        if node.will_run {
            "Will run this pass"
        } else {
            "Reused or skipped"
        }
    });
    let plan_reason_copy = planned_node.and_then(|node| node.reason.as_deref());
    let plan_artifact_copy = plan_artifact_kind.map(|kind| {
        if app_core::artifact_present(&artifact_presence, kind) {
            "Present on disk"
        } else {
            "Not yet generated"
        }
    });
    // Cheap SQL read, not a file scan -- safe to call every render. The
    // table itself only fills in once `SyncArtifactRevisions` (or a future
    // live-run writer) has recorded something for this song/kind.
    let artifact_revisions = plan_artifact_kind
        .map(|kind| app_core::load_artifact_revisions(&task.file_hash, kind))
        .unwrap_or_default();

    // Remaining Phase 7 §7.4 inspector facts (Cache Signature, Algorithm
    // Version, Last Attempt, Fallback, Error, Parameters, Parameter source)
    // -- all backed by real data that was already being loaded/computed
    // above for other purposes, just not surfaced as facts yet.
    let active_revision = artifact_revisions.iter().find(|revision| revision.active);
    let selected_cache_signature = active_revision
        .map(|revision| revision.config_hash.chars().take(12).collect::<String>())
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_algorithm_version = active_revision
        .map(|revision| revision.algorithm_version.clone())
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_last_attempt = active_revision
        .map(|revision| format_epoch_ms(revision.created_at_ms))
        .unwrap_or_else(|| selected_pending_copy.to_string());
    let selected_fallback_text = selected_device_fallback
        .map(|(from, reason)| format!("Device: {from} -> current ({reason})"))
        .or_else(|| {
            selected_backend_fallback
                .map(|(from, reason)| format!("Backend: {from} -> current ({reason})"))
        })
        .unwrap_or_else(|| "None".to_string());
    let selected_duration_text = node_duration_copy(selected_route);
    let selected_error_text = if viewing_history {
        history_error
            .map(str::to_string)
            .unwrap_or_else(|| "None recorded".to_string())
    } else {
        "None recorded".to_string()
    };
    let selected_parameter = plan_preview
        .as_ref()
        .and_then(|plan| selected_stage_parameter(plan_node_id, &plan.profile_snapshot));
    // Phase 8 §8.4: a real three-tier resolution (Global Defaults -> Song
    // Profile -> Run Override), replacing the old binary "song profile
    // exists at all? y/n" check -- backed by the identical
    // `resolve_profile_field` real execution uses (`process_song`), so this
    // fact row and what actually runs can never disagree.
    let selected_parameter_source = node_parameter_source_copy(
        node_config_profile_field(plan_node_id),
        &app_core::AnalysisProfileSnapshot::from_app_config(
            &app_core::AppConfig::load(),
            &task.file_hash,
        ),
        app_core::get_song_analysis_profile(&task.file_hash).as_ref(),
        app_core::pending_run_override_for(&task.file_hash, plan_node_id).as_deref(),
    );

    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: if inspect_only { 0.0 } else { 1.0 },
                flex_direction: FlexDirection::Column,
                padding: if inspect_only {
                    UiRect::axes(px(22), px(14))
                } else {
                    UiRect::axes(px(16), px(8))
                },
                row_gap: if inspect_only { px(10) } else { px(6) },
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.4)),
        ))
        .with_children(|session_card| {
            if !inspect_only {
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|current| {
                    spawn_text(
                        current,
                        font.clone(),
                        if viewing_history {
                            "HISTORY"
                        } else {
                            "LIVE"
                        },
                        8.0,
                        theme.primary,
                    );
                    spawn_text(current, font.clone(), operation, 12.0, theme.foreground);
                    spawn_text(current, font.clone(), detail, 9.0, theme.muted_foreground);
                    if viewing_history && active_task.is_some() {
                        spawn_text_button(
                            current,
                            font.clone(),
                            theme,
                            "View live",
                            9.0,
                            UiAction::from(AnalysisCommand::SelectAnalysisHistory(None)),
                        );
                    }
                });
            if let Some(live) = task.live.as_ref()
                && let Some(fallback_from) = live.fallback_from.as_deref() {
                    session_card
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            ..default()
                        })
                        .with_children(|route| {
                            spawn_text(
                                route,
                                font.clone(),
                                "EXECUTION FALLBACK",
                                8.0,
                                theme.editor_warning,
                            );
                            spawn_text(
                                route,
                                font.clone(),
                                format!(
                                    "{} > {}",
                                    fallback_from.to_ascii_uppercase(),
                                    live.device.to_ascii_uppercase()
                                ),
                                9.0,
                                theme.editor_warning,
                            );
                            if let Some(reason) = live.fallback_reason.as_deref() {
                                spawn_wrapped_text(
                                    route,
                                    font.clone(),
                                    reason,
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                        });
                }
            }

            let active_stage_progress = task
                .live
                .as_ref()
                .map(|live| live.stage_progress.clamp(0, 100))
                .unwrap_or(0);
            if !inspect_only {

            // GraphViewModel + auto-layout (docs/analysis-dag-redesign.md
            // Phase 7 §7.1/§7.2): every node's state now comes from the
            // real Phase 1 plan blended with the existing bucket-based
            // run-time completion signal, and every position is computed,
            // not hand-placed. Compound-node expand/collapse
            // (`session.expanded_compound_nodes`) is toggled from the Node
            // Context Menu's "Expand sub-checks"/"Collapse sub-checks"
            // action -- music.analysis renders collapsed with a "N
            // sub-checks not shown" note by default, and as separate boxes
            // once expanded.
            let stage_complete = |index: usize| {
                index < stage_index
                    || (index == stage_index && active_stage_progress >= 100)
                    || progress >= 100
            };
            let graph_spec = app_core::baseline_graph_spec();
            // MINI view (`session.analysis_mini_view`, toggled by the "VIEW"
            // row's MINI/Full button) shows only the top-level, model-backed
            // nodes: build the graph as if nothing were expanded, regardless
            // of what the user individually expanded in the full view. That
            // per-node state (`expanded_compound_nodes`) is left untouched
            // either way, so switching back to Full restores it exactly.
            let no_expanded = std::collections::BTreeSet::new();
            let mut full_expanded = session.expanded_compound_nodes.clone();
            // Stem children are the real selected pipeline; always show them
            // in Full view instead of the stems.separate shell.
            full_expanded.insert(app_core::AnalysisNodeId::new("stems.separate"));
            let expanded = if session.analysis_mini_view {
                &no_expanded
            } else {
                &full_expanded
            };
            let graph_view = build_graph_view_model(
                &graph_spec,
                plan_preview.as_ref(),
                live_node_id,
                stage_index,
                expanded,
                &analysis_node_stage_index,
                &stage_complete,
            );
            let render_graph = build_render_graph(&graph_spec, &graph_view, &|kind| {
                app_core::artifact_present(&artifact_presence, kind)
            });
            // MINI view drops the synthetic Artifact/Export boxes too --
            // "只显示以模型为基础的大节点" means the real compute/model
            // stages only, not the data-file decoration `build_render_graph`
            // adds around them. Filtering here (rather than skipping the
            // `RenderNodeKind::Artifact`/`::Export` spawn arms below) means
            // the layout algorithm never lays those boxes out in the first
            // place, and the corner mini-map -- which reads the same
            // `render_graph.nodes` -- gets the same filtering for free.
            let render_graph = if session.analysis_mini_view {
                filter_render_graph_for_mini_view(render_graph)
            } else {
                render_graph
            };
            let lineage_highlight = session
                .artifact_lineage
                .as_ref()
                .filter(|_| session.analysis_lineage_mode || session.artifact_lineage.is_some())
                .map(|panel| {
                    graph_lineage_highlight(
                        &render_graph,
                        &panel.lineage,
                        panel.scope,
                        &panel.selected,
                    )
                })
                .or_else(|| {
                    session.analysis_lineage_mode.then(GraphLineageHighlight::default)
                });
            let lineage_active = lineage_highlight
                .as_ref()
                .is_some_and(GraphLineageHighlight::is_active);
            let render_ids: Vec<app_core::AnalysisNodeId> =
                render_graph.nodes.iter().map(|n| n.id.clone()).collect();
            let routed = layered_layout_from_edges(
                &render_ids,
                &render_graph.edge_pairs(),
                LayoutSpacing::canvas(),
            )
            .map(|layout| {
                route_layered_edges(&layout, &render_graph.edge_pairs(), LayoutSpacing::canvas())
            });
            let layout = routed.as_ref().map(|routed| &routed.layout);
            let canvas_width = layout.map_or(780.0, |l| l.canvas_width).max(780.0);
            let canvas_height = layout.map_or(280.0, |l| l.canvas_height).max(220.0);
            let zoom = clamp_analysis_graph_zoom(session.analysis_graph_zoom);
            let scaled_canvas_width = canvas_width * zoom;
            let scaled_canvas_height = canvas_height * zoom;

            // Focus targets for §7.8/§9.3's "Focus Current/Failed/Stale" --
            // real per-node `NodeState::Failed`/`::Stale` from the Phase 1
            // planner (`plan_preview`), not `GraphNodeState` (the render
            // state the canvas boxes use below), which doesn't carry those
            // two variants yet. A button is only spawned when a matching
            // node genuinely exists this pass, per the phase plan's own
            // "菜单项必须按状态和节点能力启用或禁用".
            let current_focus = live_node_id
                .map(app_core::AnalysisNodeId::new)
                .and_then(|id| analysis_graph_focus_target(layout, &id, zoom));
            let failed_focus = plan_preview
                .as_ref()
                .and_then(|plan| {
                    plan.nodes
                        .iter()
                        .find(|node| node.state == app_core::NodeState::Failed)
                })
                .and_then(|node| analysis_graph_focus_target(layout, &node.id, zoom));
            let stale_focus = plan_preview
                .as_ref()
                .and_then(|plan| {
                    plan.nodes
                        .iter()
                        .find(|node| node.state == app_core::NodeState::Stale)
                })
                .and_then(|node| analysis_graph_focus_target(layout, &node.id, zoom));

            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|controls| {
                    spawn_text(controls, font.clone(), "VIEW", 7.0, theme.muted_foreground);
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "−",
                        11.0,
                        UiAction::from(AnalysisCommand::AdjustAnalysisGraphZoom(
                            -((ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32),
                        )),
                    );
                    spawn_text(
                        controls,
                        font.clone(),
                        format!("{:.0}%", zoom * 100.0),
                        9.0,
                        theme.foreground,
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "+",
                        11.0,
                        UiAction::from(AnalysisCommand::AdjustAnalysisGraphZoom(
                            (ANALYSIS_GRAPH_ZOOM_STEP * 100.0).round() as i32,
                        )),
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "Fit",
                        9.0,
                        UiAction::from(AnalysisCommand::FitAnalysisGraph(canvas_width.round() as i32)),
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        if session.analysis_mini_view {
                            "Full view"
                        } else {
                            "MINI view"
                        },
                        9.0,
                        UiAction::from(AnalysisCommand::ToggleAnalysisMiniView),
                    );
                    if let Some((scroll, stage_id)) = current_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus current",
                            9.0,
                            UiAction::from(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)),
                        );
                    }
                    if let Some((scroll, stage_id)) = failed_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus failed",
                            9.0,
                            UiAction::from(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)),
                        );
                    }
                    if let Some((scroll, stage_id)) = stale_focus {
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Focus stale",
                            9.0,
                            UiAction::from(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)),
                        );
                    }
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        "Plan Preview",
                        9.0,
                        UiAction::from(AnalysisCommand::OpenPlanPreview(task.file_hash.clone())),
                    );
                    spawn_text_button(
                        controls,
                        font.clone(),
                        theme,
                        if session.analysis_lineage_mode || session.artifact_lineage.is_some() {
                            "Hide source"
                        } else {
                            "Source"
                        },
                        9.0,
                        UiAction::from(AnalysisCommand::ToggleAnalysisLineageMode),
                    );
                    if session.analysis_lineage_mode || session.artifact_lineage.is_some() {
                        for (label, scope) in [
                            ("Upstream only", LineageScope::Upstream),
                            ("Downstream only", LineageScope::Downstream),
                            ("Full lineage", LineageScope::Full),
                        ] {
                            spawn_text_button(
                                controls,
                                font.clone(),
                                theme,
                                label,
                                9.0,
                                UiAction::from(AnalysisCommand::SetArtifactLineageScope(scope)),
                            );
                        }
                        spawn_text_button(
                            controls,
                            font.clone(),
                            theme,
                            "Return to run view",
                            9.0,
                            UiAction::from(AnalysisCommand::CloseArtifactLineage),
                        );
                    }
                });

            session_card
                .spawn((
                    AnalysisGraphViewport {
                        unscaled_width: canvas_width,
                        unscaled_height: canvas_height,
                    },
                    UiPointerApi(&["ui.pointer.analysis_viewport_pan"]),
                    ScrollPosition(Vec2::new(session.analysis_graph_scroll_offset, 0.0)),
                    Node {
                        width: percent(100),
                        min_height: px(0),
                        flex_grow: 1.0,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        overflow: Overflow::scroll(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.28)),
                    BorderColor::all(theme.border.with_alpha(0.42)),
                ))
                .with_children(|viewport| {
                    viewport
                        .spawn(Node {
                            position_type: PositionType::Relative,
                            width: px(scaled_canvas_width),
                            height: px(scaled_canvas_height),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|graph| {
                            let Some(routed) = routed.as_ref() else {
                                return;
                            };
                            let layout = &routed.layout;
                            for band in layout.lane_bands() {
                                spawn_analysis_graph_lane_band(
                                    graph,
                                    font.clone(),
                                    theme,
                                    band,
                                    zoom,
                                );
                            }
                            for edge in &render_graph.edges {
                                let Some(path) = routed.path(&edge.from, &edge.to) else {
                                    continue;
                                };
                                let binding = edge.artifact_kind.map(|kind| {
                                    app_core::resolve_graph_edge_binding(
                                        &task.file_hash,
                                        history.map(|history| history.id),
                                        edge.producer_node.as_str(),
                                        kind,
                                    )
                                });
                                let selected_edge = session.selected_graph_edge.as_ref().is_some_and(|selected| {
                                    selected.from == edge.from.as_str() && selected.to == edge.to.as_str()
                                });
                                let dimmed = lineage_active
                                    && lineage_highlight.as_ref().is_some_and(|highlight| {
                                        !highlight.emphasizes_edge(&edge.from, &edge.to)
                                    });
                                let points: Vec<Vec2> = path
                                    .iter()
                                    .map(|point| Vec2::new(point.x * zoom, point.y * zoom))
                                    .collect();
                                let show_label = selected_edge
                                    || session.analysis_lineage_mode
                                    || session.artifact_lineage.is_some();
                                spawn_analysis_graph_binding_path(
                                    graph,
                                    font.clone(),
                                    theme,
                                    &points,
                                    edge,
                                    binding.as_ref(),
                                    selected_edge,
                                    dimmed,
                                    show_label,
                                );
                            }
                            for node in &render_graph.nodes {
                                let Some(rect) = layout.rect(&node.id) else {
                                    continue;
                                };
                                let bounds = zoomed_box(rect, zoom);
                                let lineage_dimmed = lineage_active
                                    && lineage_highlight.as_ref().is_some_and(|highlight| {
                                        !highlight.emphasizes_node(&node.id)
                                    });
                                let edge_endpoint = session.selected_graph_edge.as_ref().is_some_and(
                                    |selected| {
                                        selected.from == node.id.as_str()
                                            || selected.to == node.id.as_str()
                                    },
                                );
                                match node.kind {
                                    RenderNodeKind::Compute => {
                                        let bucket = analysis_node_stage_index(node.id.as_str())
                                            .unwrap_or(0);
                                        let stage_id = bucket_stage_id(bucket);
                                        let (state, override_text) =
                                            graph_node_state_to_stage_state(
                                                node.state,
                                                active_stage_progress,
                                            );
                                        let (mut route, mut warning) =
                                            analysis_graph_route_summary(
                                                task,
                                                node.id.as_str(),
                                                stage_id,
                                                stage_complete(bucket),
                                            );
                                        if let Some(text) = override_text {
                                            route = text.to_string();
                                            warning = matches!(
                                                node.state,
                                                GraphNodeState::Blocked
                                                    | GraphNodeState::Failed
                                                    | GraphNodeState::Stale
                                            );
                                        } else if !node.detail.is_empty() {
                                            route = node.detail.clone();
                                        }
                                        if node.collapsed_child_count > 0 {
                                            route = format!(
                                                "{route} · {} sub-check{} not shown",
                                                node.collapsed_child_count,
                                                if node.collapsed_child_count == 1 {
                                                    ""
                                                } else {
                                                    "s"
                                                }
                                            );
                                        }
                                        spawn_analysis_stage_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            AnalysisStageNodeSpec {
                                                bounds,
                                                index: bucket,
                                                stage_id,
                                                node_id: node.id.as_str(),
                                                file_hash: &task.file_hash,
                                                label: &node.label,
                                                state,
                                                selected: selected_stage == stage_id || edge_endpoint,
                                                route: &route,
                                                warning,
                                                dimmed: lineage_dimmed,
                                            },
                                        );
                                    }
                                    RenderNodeKind::Artifact => {
                                        spawn_workbench_artifact_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            bounds,
                                            &node.label,
                                            &node.detail,
                                            node.state == GraphNodeState::Complete,
                                            node.id.as_str(),
                                            &task.file_hash,
                                            history.map(|history| history.id),
                                            lineage_dimmed,
                                            edge_endpoint,
                                        );
                                    }
                                    RenderNodeKind::Export => {
                                        spawn_workbench_export_node(
                                            graph,
                                            font.clone(),
                                            theme,
                                            bounds,
                                            &node.label,
                                            &task.file_hash,
                                            node.id.as_str(),
                                            node.state == GraphNodeState::Complete,
                                            lineage_dimmed,
                                            edge_endpoint,
                                        );
                                    }
                                }
                            }
                            if let Some(highlight) = lineage_highlight.as_ref() {
                                for missing in &highlight.missing_gaps {
                                    spawn_text(
                                        graph,
                                        font.clone(),
                                        format!("GAP · missing legacy revision {missing}"),
                                        7.0,
                                        theme.destructive,
                                    );
                                }
                            }
                        });
                })
                .observe(
                    |mut drag: On<Pointer<Drag>>,
                     ui_scale: Res<UiScale>,
                     mut analysis: ResMut<AnalysisUiState>,
                     mut viewports: Query<
                        (&ComputedNode, &mut ScrollPosition),
                        With<AnalysisGraphViewport>,
                    >| {
                        if drag.button != PointerButton::Primary {
                            return;
                        }
                        drag.propagate(false);
                        let Ok((computed, mut position)) = viewports.single_mut() else {
                            return;
                        };
                        let size = computed.size() * computed.inverse_scale_factor();
                        let content = computed.content_size() * computed.inverse_scale_factor();
                        let delta = drag.delta / ui_scale.0;
                        position.x = (position.x - delta.x)
                            .clamp(0.0, (content.x - size.x).max(0.0));
                        analysis.analysis_graph_scroll_offset = position.x;
                    },
                );
            }

            if inspect_only {
            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(16)),
                        row_gap: px(12),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.34)),
                    BorderColor::all(theme.primary.with_alpha(0.38)),
                ))
                .with_children(|inspector| {
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(10),
                            flex_wrap: FlexWrap::Wrap,
                            row_gap: px(5),
                            ..default()
                        })
                        .with_children(|header| {
                            spawn_text(
                                header,
                                font.clone(),
                                format!(
                                    "STEP {:02} · {}",
                                    selected_stage_index + 1,
                                    selected_label.to_ascii_uppercase()
                                ),
                                9.0,
                                theme.primary,
                            );
                            header.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                header,
                                font.clone(),
                                format!("{selected_status} · {selected_progress}%"),
                                9.0,
                                if selected_status == "WAITING" {
                                    theme.muted_foreground
                                } else {
                                    theme.pitch_contour
                                },
                            );
                        });
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_purpose,
                        10.0,
                        theme.muted_foreground,
                    );
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        selected_operation,
                        13.0,
                        theme.foreground,
                    );
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(9),
                            row_gap: px(9),
                            ..default()
                        })
                        .with_children(|facts| {
                            // §7.4 lists 14 facts; ALGORITHM VERSION / CACHE
                            // SIGNATURE / LAST ATTEMPT come from the active
                            // artifact revision, FALLBACK from the same
                            // route data the "current operation" banner
                            // above already uses, ERROR from the selected
                            // history run, DURATION from the same route's
                            // real `started_at_ms`/`finished_at_ms`
                            // (`server.py::_progress_payload`, Phase 7's
                            // "Duration 检查器字段" gap closed), and the
                            // node's one profile parameter (+ its source)
                            // only when the selected node actually has one
                            // (`selected_stage_parameter`) -- PARAMETER
                            // SOURCE-without-a-parameter is intentionally
                            // omitted rather than faked: a source with no
                            // parameter to source would be meaningless.
                            let mut fact_rows: Vec<(&str, String)> = vec![
                                ("IMPLEMENTATION", selected_implementation.to_string()),
                                ("MODEL / ALGORITHM", selected_model.to_string()),
                                ("REQUESTED DEVICE", selected_requested_device.to_string()),
                                ("ACTUAL DEVICE", selected_actual_device.to_string()),
                                ("INPUT", selected_input.to_string()),
                                ("OUTPUT", selected_output.to_string()),
                                ("ALGORITHM VERSION", selected_algorithm_version.clone()),
                                ("CACHE SIGNATURE", selected_cache_signature.clone()),
                                ("LAST ATTEMPT", selected_last_attempt.clone()),
                                ("DURATION", selected_duration_text.clone()),
                                ("FALLBACK", selected_fallback_text.clone()),
                                ("ERROR", selected_error_text.clone()),
                            ];
                            if let Some((label, value)) = selected_parameter.clone() {
                                fact_rows.push((label, value));
                                fact_rows
                                    .push(("PARAMETER SOURCE", selected_parameter_source.to_string()));
                            }
                            for (label, value) in fact_rows {
                                let value_color = if label == "ERROR" && value != "None recorded" {
                                    theme.destructive
                                } else if label == "FALLBACK" && value != "None" {
                                    theme.editor_warning
                                } else {
                                    theme.foreground
                                };
                                facts
                                    .spawn((
                                        Node {
                                            min_width: px(205),
                                            flex_basis: px(240),
                                            flex_grow: 1.0,
                                            flex_direction: FlexDirection::Column,
                                            padding: UiRect::all(px(10)),
                                            row_gap: px(3),
                                            overflow: Overflow::clip(),
                                            border: UiRect::all(px(1)),
                                            border_radius: BorderRadius::all(px(4)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.34)),
                                        BorderColor::all(theme.border.with_alpha(0.4)),
                                    ))
                                    .with_children(|fact| {
                                        spawn_text(
                                            fact,
                                            font.clone(),
                                            label,
                                            7.0,
                                            theme.muted_foreground,
                                        );
                                        spawn_bounded_wrapped_text(
                                            fact,
                                            font.clone(),
                                            value,
                                            9.0,
                                            value_color,
                                        );
                                    });
                            }
                        });
                    inspector
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(10)),
                                row_gap: px(4),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(4)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.34)),
                            BorderColor::all(theme.border.with_alpha(0.4)),
                        ))
                        .with_children(|plan_box| {
                            plan_box
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    ..default()
                                })
                                .with_children(|plan_header| {
                                    spawn_text(
                                        plan_header,
                                        font.clone(),
                                        "PLAN & ARTIFACTS",
                                        7.0,
                                        theme.muted_foreground,
                                    );
                                    plan_header.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    spawn_text_button(
                                        plan_header,
                                        font.clone(),
                                        theme,
                                        "Sync from disk",
                                        8.0,
                                        UiAction::from(AnalysisCommand::SyncArtifactRevisions(task.file_hash.clone())),
                                    );
                                });
                            spawn_text(
                                plan_box,
                                font.clone(),
                                format!("{plan_state_copy} · {plan_will_run_copy}"),
                                9.0,
                                theme.foreground,
                            );
                            if let Some(artifact_copy) = plan_artifact_copy {
                                spawn_text(
                                    plan_box,
                                    font.clone(),
                                    artifact_copy,
                                    9.0,
                                    theme.muted_foreground,
                                );
                            }
                            if let Some(reason) = plan_reason_copy {
                                spawn_wrapped_text(
                                    plan_box,
                                    font.clone(),
                                    reason,
                                    9.0,
                                    theme.editor_warning,
                                );
                            }
                            for revision in &artifact_revisions {
                                let file_name = revision
                                    .path
                                    .file_name()
                                    .map(|name| name.to_string_lossy().to_string())
                                    .unwrap_or_else(|| revision.id.clone());
                                plan_box
                                    .spawn(Node {
                                        width: percent(100),
                                        align_items: AlignItems::Center,
                                        column_gap: px(8),
                                        flex_wrap: FlexWrap::Wrap,
                                        row_gap: px(4),
                                        ..default()
                                    })
                                    .with_children(|row| {
                                        spawn_text(
                                            row,
                                            font.clone(),
                                            match (revision.active, revision.invalidated) {
                                                (_, true) => format!("✕ {file_name} · invalidated"),
                                                (true, false) => format!("● {file_name}"),
                                                (false, false) => format!("○ {file_name}"),
                                            },
                                            9.0,
                                            if revision.invalidated {
                                                theme.destructive
                                            } else if revision.active {
                                                theme.pitch_contour
                                            } else {
                                                theme.muted_foreground
                                            },
                                        );
                                        row.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        if artifact_kind_is_playable(revision.kind) {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Play",
                                                8.0,
                                                UiAction::from(LibraryCommand::PlayArtifactRevision(
                                                    revision.path.clone(),
                                                )),
                                            );
                                        } else {
                                            // §7.6 "Preview": the JSON/text
                                            // counterpart to "Play" above --
                                            // the two are mutually exclusive
                                            // by artifact kind, never both
                                            // shown for the same revision.
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Preview",
                                                8.0,
                                                UiAction::from(AnalysisCommand::PreviewArtifactRevision(
                                                    revision.path.clone(),
                                                )),
                                            );
                                        }
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Open",
                                            8.0,
                                            UiAction::from(AnalysisCommand::OpenArtifactRevision(revision.path.clone())),
                                        );
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Reveal",
                                            8.0,
                                            UiAction::from(AnalysisCommand::RevealArtifactRevision(
                                                revision.path.clone(),
                                            )),
                                        );
                                        if !revision.active && !revision.invalidated {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Set active",
                                                8.0,
                                                UiAction::from(AnalysisCommand::SetActiveArtifactRevision(
                                                    revision.clone(),
                                                )),
                                            );
                                        }
                                        // Phase 6 `invalidate_artifact_revision` /
                                        // §7.6 "Invalidate": omitted once a
                                        // revision is already invalidated --
                                        // there's nothing further to invalidate,
                                        // and no "restore" action exists yet
                                        // (a fresh rerun or Sync from disk is
                                        // the intended way back).
                                        if !revision.invalidated {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Invalidate",
                                                8.0,
                                                UiAction::from(AnalysisCommand::RequestInvalidateArtifactRevision(
                                                    revision.clone(),
                                                )),
                                            );
                                        }
                                        spawn_text_button(
                                            row,
                                            font.clone(),
                                            theme,
                                            "Inspect provenance",
                                            8.0,
                                            UiAction::from(AnalysisCommand::InspectArtifactProvenance(revision.clone())),
                                        );
                                        let workbench_ref =
                                            artifact_ref_from_revision(revision);
                                        if let Ok(inspection) =
                                            app_core::inspect_artifact(&workbench_ref)
                                        {
                                            if inspection.capabilities.iter().any(|capability| {
                                                matches!(
                                                    capability,
                                                    app_core::ArtifactCapability::OpenLyricsEditor
                                                        | app_core::ArtifactCapability::OpenChartEditor
                                                )
                                            }) {
                                                spawn_text_button(
                                                    row,
                                                    font.clone(),
                                                    theme,
                                                    "Edit",
                                                    8.0,
                                                    UiAction::from(AnalysisCommand::OpenArtifactCompatibleEditor(
                                                        workbench_ref.clone(),
                                                    )),
                                                );
                                            }
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                if inspection.pinned { "Unpin" } else { "Pin" },
                                                8.0,
                                                UiAction::from(AnalysisCommand::ToggleArtifactPinned(workbench_ref.clone())),
                                            );
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Source",
                                                8.0,
                                                UiAction::from(AnalysisCommand::ShowArtifactLineage(workbench_ref.clone())),
                                            );
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Impact",
                                                8.0,
                                                UiAction::from(AnalysisCommand::ShowArtifactImpact(workbench_ref.clone())),
                                            );
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Help",
                                                8.0,
                                                UiAction::from(AppCommand::OpenDocumentation(Some(format!(
                                                    "artifact:{:?}",
                                                    revision.kind
                                                )))),
                                            );
                                        }
                                        // §7.6 "Compare revisions": against
                                        // whichever revision is Active for
                                        // this kind -- omitted for the
                                        // Active revision itself (nothing to
                                        // compare it to) and when this song's
                                        // kind has no Active revision at all.
                                        if !revision.active
                                            && let Some(active) = active_revision
                                        {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Compare revisions",
                                                8.0,
                                                UiAction::from(AnalysisCommand::CompareArtifactRevisions(
                                                    revision.clone(),
                                                    artifact_ref_from_revision(active),
                                                )),
                                            );
                                        }
                                        let revision_is_pinned = app_core::inspect_artifact(
                                            &artifact_ref_from_revision(revision),
                                        )
                                        .is_ok_and(|inspection| inspection.pinned);
                                        if !revision_is_pinned {
                                            spawn_text_button(
                                                row,
                                                font.clone(),
                                                theme,
                                                "Delete",
                                                8.0,
                                                UiAction::from(AnalysisCommand::RequestDeleteArtifactRevision(
                                                    revision.clone(),
                                                )),
                                            );
                                        }
                                    });
                            }
                        });
                    spawn_node_io_workbench(
                        inspector,
                        font.clone(),
                        theme,
                        &task.file_hash,
                        plan_node_id,
                        history.map(|history| history.id),
                        session.selected_artifact_inspector_tab,
                    );
                    for (label, from, to, reason) in selected_device_fallback
                        .map(|(from, reason)| {
                            ("COMPUTE FALLBACK", from, selected_actual_device, reason)
                        })
                        .into_iter()
                        .chain(selected_backend_fallback.map(|(from, reason)| {
                            ("MODEL FALLBACK", from, selected_implementation, reason)
                        }))
                    {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!(
                                "{label} · {} > {} · {reason}",
                                from.to_ascii_uppercase(),
                                to.to_ascii_uppercase()
                            ),
                            9.0,
                            theme.editor_warning,
                        );
                    }
                    if let Some(error) = history_error {
                        spawn_wrapped_text(
                            inspector,
                            font.clone(),
                            format!("SESSION ERROR · {error}"),
                            9.0,
                            theme.destructive,
                        );
                    }
                });

            if let Some(live) = task.live.as_ref() {
                session_card
                    .spawn(Node {
                        width: percent(100),
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: px(10),
                        row_gap: px(10),
                        ..default()
                    })
                    .with_children(|details| {
                        let device_route = live
                            .fallback_from
                            .as_ref()
                            .map(|from| {
                                format!(
                                    "{} > {}",
                                    from.to_ascii_uppercase(),
                                    live.device.to_ascii_uppercase()
                                )
                            })
                            .unwrap_or_else(|| live.device.to_ascii_uppercase());
                        for (label, value) in [
                            ("IMPLEMENTATION", live.implementation.clone()),
                            ("MODEL / ALGORITHM", live.model.clone()),
                            ("ACTUAL COMPUTE ROUTE", device_route),
                        ] {
                            details
                                .spawn((
                                    Node {
                                        min_width: px(230),
                                        flex_grow: 1.0,
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(px(12)),
                                        row_gap: px(3),
                                        overflow: Overflow::clip(),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(4)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.26)),
                                    BorderColor::all(theme.border.with_alpha(0.45)),
                                ))
                                .with_children(|item| {
                                    spawn_text(
                                        item,
                                        font.clone(),
                                        label,
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                    spawn_bounded_wrapped_text(
                                        item,
                                        font.clone(),
                                        value,
                                        10.0,
                                        theme.foreground,
                                    );
                                });
                        }
                    });
            }
            }

        });

    if let Some(context) = session.analysis_artifact_context.as_ref() {
        spawn_analysis_artifact_context_menu(parent, font.clone(), theme, context);
    }
    if let Some(context) = session.analysis_export_context.as_ref() {
        spawn_analysis_export_context_menu(parent, font.clone(), theme, context);
    }
    if let Some(dialog) = session.node_config_dialog.as_ref() {
        spawn_node_config_dialog(
            parent,
            font.clone(),
            theme,
            dialog,
            session.config.compute_backend.as_deref() == Some("intel"),
            session.notice.as_deref(),
        );
    }
    if let Some(draft) = session.plan_preview_draft.as_ref() {
        spawn_plan_preview_dialog(
            parent,
            font.clone(),
            theme,
            draft,
            session.notice.as_deref(),
        );
    }
    if let Some(state) = session.app_log_viewer.as_ref() {
        spawn_app_log_viewer(
            parent,
            font.clone(),
            theme,
            state,
            session.selected_analysis_history,
        );
    }
}
