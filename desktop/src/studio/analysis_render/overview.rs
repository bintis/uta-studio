use super::*;
use crate::studio::*;

fn item_matches_selected_song(file_hash: &str, selected_song: Option<&String>) -> bool {
    selected_song.is_none_or(|selected| selected == file_hash)
}

fn selected_history_for_song<'a>(
    session: &'a StudioSessionView<'_>,
) -> Option<&'a app_core::AnalysisRunHistory> {
    let selected_id = session.selected_analysis_history?;
    session.analysis_history.iter().find(|history| {
        history.id == selected_id
            && item_matches_selected_song(&history.file_hash, session.selected_song.as_ref())
    })
}

fn analysis_engine_error_copy(error: &app_core::EngineErrorHistoryProjection) -> String {
    let fusion_adapter = error.resource.as_deref() == Some("tool:fusion_agent_adapter");
    if !fusion_adapter {
        return format!("{}: {}", error.code, error.message);
    }
    match error.code.as_str() {
        "worker_timeout" => "Fusion Agent Adapter timed out after 600 seconds. No Algorithm fallback was used. Check the provider and retry.".to_string(),
        "cancelled" => "AI judgment was cancelled. No Algorithm fallback was used.".to_string(),
        "worker_protocol_mismatch" => "Fusion Agent Adapter protocol mismatch. Choose a compatible verified adapter in Settings > Models & runtime. No Algorithm fallback was used.".to_string(),
        "worker_unavailable" | "runtime_resolution_failed" => "Fusion Agent Adapter is missing or unusable. Configure it in Settings > Models & runtime. No Algorithm fallback was used.".to_string(),
        "output_validation_failed" => format!("Fusion Agent Adapter returned an invalid candidate selection. No Algorithm fallback was used. {}", error.message),
        "worker_failed" => format!("The Fusion Agent Adapter or its external AI provider failed. No Algorithm fallback was used. {}", error.message),
        _ => format!("AI judgment failed without an Algorithm fallback. {}: {}", error.code, error.message),
    }
}

/// Why a specific capability was never requested, for the handful of
/// capabilities where "not requested" isn't self-explanatory from the
/// toggle the user actually clicked in Processing Studio. `notes.stars` and
/// `technique.analyze` both resolve to the STARS model, which the Engine
/// planner (`analysis-engine/src/planner/plan.rs`) only requests for `zh`/
/// `yue` lyrics regardless of the Step 3 selection -- confirmed against a
/// real song where enabling both cards still left them NotRequested because
/// its lyrics were Japanese. Only `GraphNodeState::NotRequested` gets this
/// treatment; other states already carry a self-explanatory override text.
fn not_requested_reason(state: GraphNodeState, capability_id: &str) -> Option<&'static str> {
    if state != GraphNodeState::NotRequested {
        return None;
    }
    match capability_id {
        "notes.stars" | "technique.analyze" => {
            Some("Not requested · STARS requires Chinese/Cantonese lyrics")
        }
        _ => None,
    }
}

fn select_context_snapshot<'a, T>(
    requires_frozen: bool,
    frozen: Option<&'a T>,
    current: Option<&'a T>,
) -> Option<&'a T> {
    if requires_frozen {
        frozen
    } else {
        frozen.or(current)
    }
}

pub(crate) fn analysis_start_unavailable(_file_hash: &str) -> Option<String> {
    // Start always opens the exact Engine Plan Preview. Global component health
    // and legacy Production warnings are advisory in the testing build; the
    // exact request remains the only place that may block on a genuinely absent,
    // corrupt, or non-executable required resource.
    None
}

pub(crate) fn current_analysis_header(
    session: &StudioSessionView<'_>,
) -> Option<(String, String, usize)> {
    let active_task = session
        .analysis_tasks
        .iter()
        .filter(|task| item_matches_selected_song(&task.file_hash, session.selected_song.as_ref()))
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    item_matches_selected_song(&task.file_hash, session.selected_song.as_ref())
                })
                .find(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Staged | app_core::QueuedStatus::Queued
                    )
                })
        });
    let history = selected_history_for_song(session);
    if let Some(history) = history {
        let progress = if history.status == "completed" {
            100
        } else {
            history.snapshot.overall_progress.clamp(0, 100)
        };
        return Some((history.title.clone(), history.artist.clone(), progress));
    }
    active_task
        .map(|task| {
            let progress = match (&task.status, task.live.as_ref()) {
                (app_core::QueuedStatus::Analyzing(_), Some(live)) if live.engine.is_some() => {
                    live.overall_progress.clamp(0, 100)
                }
                (app_core::QueuedStatus::Analyzing(progress), _) => (*progress).clamp(0, 100),
                _ => 0,
            };
            (task.title.clone(), task.artist.clone(), progress)
        })
        .or_else(|| {
            let hash = session.selected_song.as_ref()?;
            let song = app_core::load_song_by_hash(hash).ok().flatten()?;
            Some((song.title, song.artist, 0))
        })
}

pub(crate) fn current_analysis_eyebrow(session: &StudioSessionView<'_>) -> &'static str {
    let selected_history_status =
        selected_history_for_song(session).map(|history| history.status.as_str());
    let has_active_task = session.analysis_tasks.iter().any(|task| {
        item_matches_selected_song(&task.file_hash, session.selected_song.as_ref())
            && matches!(
                task.status,
                app_core::QueuedStatus::Staged
                    | app_core::QueuedStatus::Analyzing(_)
                    | app_core::QueuedStatus::Queued
            )
    });
    analysis_eyebrow_label(
        has_active_task,
        selected_history_status,
        session
            .analysis_history
            .iter()
            .find(|history| {
                item_matches_selected_song(&history.file_hash, session.selected_song.as_ref())
            })
            .map(|history| history.status.as_str()),
    )
}

pub(crate) fn analysis_eyebrow_label(
    has_active_task: bool,
    selected_history_status: Option<&str>,
    newest_history_status: Option<&str>,
) -> &'static str {
    if selected_history_status.is_some() {
        return if selected_history_status == Some("completed") {
            "ANALYSIS COMPLETE"
        } else {
            "ANALYSIS HISTORY"
        };
    }
    if has_active_task {
        return "IN PROGRESS";
    }
    if newest_history_status == Some("completed") {
        "ANALYSIS COMPLETE"
    } else {
        "ANALYSIS HISTORY"
    }
}

pub(crate) fn current_analysis_file_hash(session: &StudioSessionView<'_>) -> Option<String> {
    let active_task = session
        .analysis_tasks
        .iter()
        .filter(|task| {
            session
                .selected_song
                .as_ref()
                .is_none_or(|hash| hash == &task.file_hash)
        })
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    session
                        .selected_song
                        .as_ref()
                        .is_none_or(|hash| hash == &task.file_hash)
                })
                .find(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Staged | app_core::QueuedStatus::Queued
                    )
                })
        });
    selected_history_for_song(session)
        .map(|history| history.file_hash.clone())
        .or_else(|| active_task.map(|task| task.file_hash.clone()))
        .or_else(|| session.selected_song.clone())
}

fn spawn_analysis_empty_canvas(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                width: percent(100),
                min_width: px(0),
                max_width: percent(100),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                row_gap: px(12),
                padding: UiRect::all(px(28)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.22)),
        ))
        .with_children(|empty| {
            if let Some(file_hash) = session.selected_song.as_ref() {
                spawn_wrapped_text(
                    empty,
                    font.clone(),
                    "Start an analysis to generate a workflow run for this song. You can review the exact target and quality before it is queued.",
                    13.0,
                    theme.muted_foreground,
                );
                if let Some(reason) = analysis_start_unavailable(file_hash) {
                    spawn_wrapped_text(
                        empty,
                        font.clone(),
                        reason,
                        9.0,
                        theme.muted_foreground,
                    );
                } else {
                    spawn_action_button(
                        empty,
                        font.clone(),
                        theme,
                        "Start analysis",
                        UiAction::from(AnalysisCommand::StartAnalysis(file_hash.clone())),
                    );
                }
            } else {
                spawn_text(
                    empty,
                    font.clone(),
                    "Choose a song to continue",
                    20.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    empty,
                    font.clone(),
                    "The DAG graph needs a song before it can show or run a processing workflow.",
                    11.0,
                    theme.muted_foreground,
                );
                spawn_action_button(
                    empty,
                    font.clone(),
                    theme,
                    "Choose a song",
                    UiAction::from(LibraryCommand::SetLibraryView(LibraryView::All)),
                );
            }
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

fn analysis_audit_status_color(status: &str, theme: &StudioTheme) -> Color {
    match status {
        "COMPLETE" => theme.pitch_contour,
        "RUNNING" => theme.primary,
        "FAILED" | "CANCELLED" => theme.destructive,
        "BYPASSED" => theme.editor_warning,
        _ => theme.muted_foreground,
    }
}

fn spawn_analysis_audit_section(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    eyebrow: &'static str,
    title: &'static str,
    detail: &'static str,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            studio_card_background(theme),
            studio_card_border(theme),
            studio_card_shadow(theme),
        ))
        .with_children(|card| {
            spawn_text(card, font.clone(), eyebrow, 7.5, theme.primary);
            spawn_text(card, font.clone(), title, 14.0, theme.foreground);
            spawn_wrapped_text(card, font, detail, 8.5, theme.muted_foreground);
            card.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.32)),
            ));
            build(card);
        });
}

fn spawn_analysis_audit_fact(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    value: impl Into<String>,
    accent: Color,
    min_width: f32,
) {
    parent
        .spawn((
            Node {
                min_width: px(min_width),
                flex_basis: px(min_width + 34.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(11)),
                row_gap: px(4),
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.26)),
            BorderColor::all(theme.border.with_alpha(0.38)),
        ))
        .with_children(|fact| {
            spawn_text(fact, font.clone(), label, 7.5, theme.muted_foreground);
            spawn_bounded_wrapped_text(fact, font, value, 10.0, accent);
        });
}

fn spawn_analysis_audit_callout(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    detail: impl Into<String>,
    accent: Color,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(12), px(10)),
                row_gap: px(3),
                border: UiRect::all(px(1)),
                border_radius: studio_control_radius(),
                ..default()
            },
            BackgroundColor(accent.with_alpha(0.08)),
            BorderColor::all(accent.with_alpha(0.28)),
        ))
        .with_children(|callout| {
            spawn_text(callout, font.clone(), label, 7.5, accent);
            spawn_bounded_wrapped_text(callout, font, detail, 9.0, theme.foreground);
        });
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
        .filter(|task| {
            session
                .selected_song
                .as_ref()
                .is_none_or(|hash| &task.file_hash == hash)
        })
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .filter(|task| {
                    session
                        .selected_song
                        .as_ref()
                        .is_none_or(|hash| &task.file_hash == hash)
                })
                .find(|task| {
                    matches!(
                        task.status,
                        app_core::QueuedStatus::Staged | app_core::QueuedStatus::Queued
                    )
                })
        });
    let history = selected_history_for_song(session);
    let history_task = history.map(|history| app_core::AnalysisTask {
        file_hash: history.file_hash.clone(),
        title: history.title.clone(),
        artist: history.artist.clone(),
        // Historical snapshots are immutable evidence, never a live-running
        // task. Their exact node events are overlaid below without promoting
        // the snapshot's last node to Running.
        status: app_core::QueuedStatus::Queued,
        live: Some(history.snapshot.clone()),
    });
    let current_task = (history_task.is_none() && active_task.is_none())
        .then(|| {
            let file_hash = session.selected_song.as_ref()?;
            let song = app_core::load_song_by_hash(file_hash).ok().flatten();
            Some(app_core::AnalysisTask {
                file_hash: file_hash.clone(),
                title: song
                    .as_ref()
                    .map(|song| song.title.clone())
                    .unwrap_or_else(|| file_hash.clone()),
                artist: song
                    .as_ref()
                    .map(|song| song.artist.clone())
                    .unwrap_or_else(|| "Unknown artist".to_string()),
                status: app_core::QueuedStatus::Queued,
                live: None,
            })
        })
        .flatten();
    let Some(task) = history_task
        .as_ref()
        .or(active_task)
        .or(current_task.as_ref())
    else {
        if !inspect_only {
            spawn_analysis_empty_canvas(parent, font, session, theme);
        }
        return;
    };
    let viewing_history = history_task.is_some();
    let exact_workflow = task
        .live
        .as_ref()
        .and_then(|live| live.engine.as_ref())
        .and_then(exact_workflow_plan_from_engine);
    let exact_engine_capabilities = task
        .live
        .as_ref()
        .and_then(|live| live.engine.as_ref())
        .and_then(exact_engine_capabilities_from_engine);
    let current_workflow = session
        .workflow_snapshot
        .as_ref()
        .and_then(|snapshot| app_core::WorkflowExecutionWireV1::from_snapshot(snapshot).ok());
    let workflow_wire = select_context_snapshot(
        viewing_history || active_task.is_some(),
        exact_workflow.as_ref().map(|(workflow, _)| workflow),
        current_workflow.as_ref(),
    );
    let Some(workflow_wire) = workflow_wire else {
        spawn_wrapped_text(
            parent,
            font,
            "The compiled workflow is unavailable. Reopen Processing Studio and resolve its compile error.",
            10.0,
            theme.destructive,
        );
        return;
    };
    let mut authoritative_render_graph = build_workflow_render_graph(
        workflow_wire,
        exact_workflow.as_ref().and_then(|(_, plan)| plan.as_ref()),
        exact_engine_capabilities.as_ref(),
        viewing_history && history.is_some_and(|history| history.status == "completed"),
    );
    overlay_workflow_runtime(&mut authoritative_render_graph, task);

    let live_node_id = task.live.as_ref().and_then(|live| live.node_id.as_deref());
    let operation = task
        .live
        .as_ref()
        .map(|live| live.operation.as_str())
        .unwrap_or("Waiting for the analysis runtime");
    let selected_node = session
        .selected_analysis_node
        .as_ref()
        .and_then(|id| authoritative_render_graph.node(&app_core::AnalysisNodeId::new(id)))
        .or_else(|| {
            live_node_id
                .and_then(|id| authoritative_render_graph.node(&app_core::AnalysisNodeId::new(id)))
        })
        .or_else(|| authoritative_render_graph.nodes.first());
    let selected_node_id = selected_node
        .map(|node| node.id.as_str())
        .unwrap_or("workflow");
    let selected_step = selected_node
        .map(|node| workflow_graph_step(node.capability_id.as_deref()))
        .unwrap_or(1);
    let selected_route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, selected_node_id));
    let has_structured_runtime = task.live.as_ref().is_some_and(|live| {
        live.node_id.is_some()
            || live
                .stage_routes
                .iter()
                .any(|route| route.node_id.is_some())
    });
    let selected_is_current = has_structured_runtime && live_node_id == Some(selected_node_id);
    let selected_event = selected_route.and_then(|route| route.node_event.as_deref());
    let route_selected_progress = if matches!(
        selected_event,
        Some("completed" | "reused" | "node_completed" | "artifact_reused")
    ) {
        100
    } else if let Some(route) = selected_route {
        if task.live.as_ref().is_some_and(|live| live.engine.is_some()) {
            worker_reported_progress(route).unwrap_or(0)
        } else {
            route.stage_progress.clamp(0, 100)
        }
    } else if has_structured_runtime {
        0
    } else if selected_is_current {
        task.live
            .as_ref()
            .map(|live| live.stage_progress.clamp(0, 100))
            .unwrap_or(0)
    } else {
        0
    };
    let selected_trace_missing =
        !has_structured_runtime && selected_route.is_none() && route_selected_progress >= 100;
    let selected_pending_copy = if selected_trace_missing {
        "Not recorded in this analysis session"
    } else {
        "Pending"
    };
    let selected_label = selected_node
        .map(|node| node.label.as_str())
        .unwrap_or("Workflow");
    let selected_purpose = selected_node
        .map(|node| node.detail.as_str())
        .unwrap_or("Compiled Processing Studio workflow");
    let selected_input = workflow_wire
        .bindings
        .iter()
        .filter(|binding| binding.to_node == selected_node_id)
        .map(|binding| binding.semantic_type.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    let selected_output = workflow_wire
        .bindings
        .iter()
        .filter(|binding| binding.from_node == selected_node_id)
        .map(|binding| binding.semantic_type.as_str())
        .chain(
            workflow_wire
                .terminal_outputs
                .iter()
                .filter(|output| output.node == selected_node_id)
                .map(|output| output.semantic_type.as_str()),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(", ");
    let route_selected_status = match selected_event {
        Some("completed" | "reused" | "node_completed" | "artifact_reused") => "COMPLETE",
        Some("started" | "progress" | "node_started" | "node_progress") => "RUNNING",
        Some("failed" | "node_failed") => "FAILED",
        Some("cancelled" | "node_cancelled") => "CANCELLED",
        Some("skipped" | "node_skipped") => "BYPASSED",
        _ if selected_is_current => "RUNNING",
        _ => "WAITING",
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
    let history_error = history.and_then(|history| {
        history
            .snapshot
            .engine_error
            .as_ref()
            .map(analysis_engine_error_copy)
            .or_else(|| history.error_message.clone())
    });

    let selected_render_state = selected_node.map(|node| node.state);
    let (selected_progress, selected_status) = selected_progress_and_status(
        selected_render_state,
        route_selected_progress,
        route_selected_status,
    );
    let selected_progress_is_reported = selected_status == "COMPLETE"
        || selected_route.and_then(worker_reported_progress).is_some()
        || task.live.as_ref().is_none_or(|live| live.engine.is_none());
    let selected_fallback_text = selected_device_fallback
        .map(|(from, reason)| format!("Device: {from} -> current ({reason})"))
        .or_else(|| {
            selected_backend_fallback
                .map(|(from, reason)| format!("Backend: {from} -> current ({reason})"))
        })
        .unwrap_or_else(|| "None".to_string());
    let selected_duration_text = node_duration_copy(selected_route);
    let selected_worker_task_text = selected_worker_task_text(selected_route);
    let selected_error_text = if viewing_history {
        history_error
            .clone()
            .unwrap_or_else(|| "None recorded".to_string())
    } else {
        "None recorded".to_string()
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: if inspect_only { 0.0 } else { 1.0 },
                flex_direction: FlexDirection::Column,
                padding: if inspect_only {
                    UiRect::axes(px(22), px(14))
                } else if session.analysis_model_panel_open {
                    UiRect::new(px(16), px(ANALYSIS_MODEL_PANEL_WIDTH + 12.0), px(8), px(8))
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
            if !inspect_only
                && let Some(live) = task.live.as_ref()
                && let Some(fallback_from) = live.fallback_from.as_deref()
            {
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

            if !inspect_only {
                // Every node and binding comes from the compiled Processing Studio
                // workflow selected above. Layout receives arbitrary workflow ids
                // and uses only topology plus compiled metadata.
                let render_graph = authoritative_render_graph.clone();
                let locale = effective_ui_locale(session.config);
                let mode = analysis_graph_mode(viewing_history, active_task.is_some());
                let counts = analysis_graph_node_counts(&render_graph.nodes);
                let overall_progress = if let Some(history) = history {
                    Some(if history.status == "completed" {
                        100
                    } else {
                        history.snapshot.overall_progress.clamp(0, 100)
                    })
                } else {
                    task.live
                        .as_ref()
                        .map(|live| live.overall_progress.clamp(0, 100))
                };
                let current_label = (mode == AnalysisGraphMode::Live).then_some(operation);
                let zoom = clamp_analysis_graph_zoom(session.analysis_graph_zoom);
                let follow_available = analysis_graph_follow_available(mode);
                let follow_active = follow_available && session.analysis_graph_follow_enabled;
                spawn_analysis_graph_context_bar(
                    session_card,
                    font.clone(),
                    theme,
                    mode,
                    overall_progress,
                    current_label,
                    counts,
                );

                let mut steps = std::collections::BTreeMap::new();
                let specs = render_graph
                    .nodes
                    .iter()
                    .enumerate()
                    .map(|(order_hint, node)| {
                        let kind = match node.kind {
                            RenderNodeKind::Compute => LayoutNodeVisualKind::Compute,
                        };
                        let label =
                            translate_ui(locale, &node.label).unwrap_or_else(|| node.label.clone());
                        let detail = translate_ui(locale, &node.detail)
                            .unwrap_or_else(|| node.detail.clone());
                        let mut spec = LayoutNodeSpec::from_text(
                            node.id.clone(),
                            kind,
                            &label,
                            &detail,
                            order_hint,
                        );
                        // §5: width may widen for real multi-model/long-label
                        // content; height never varies -- no execution state
                        // or selection enters this at all.
                        spec.width *= analysis_node_width_scale(&node.label, &node.model_ids);
                        steps.insert(
                            node.id.clone(),
                            workflow_graph_step(node.capability_id.as_deref()),
                        );
                        spec
                    })
                    .collect::<Vec<_>>();
                let edge_pairs = render_graph.edge_pairs();
                let routed = cached_canvas_routed_layout_with_specs(&specs, &edge_pairs);
                let canvas_width = routed
                    .as_ref()
                    .map_or(780.0, |routed| routed.layout.canvas_width)
                    .max(780.0);
                let canvas_height = routed
                    .as_ref()
                    .map_or(450.0, |routed| routed.layout.canvas_height)
                    .max(450.0);
                let scaled_canvas_width = canvas_width * zoom;
                let scaled_canvas_height = canvas_height * zoom;
                // Center the fitted graph inside the viewport when it is
                // smaller than the available space (direct feedback that a
                // small/simple workflow sat pinned to the top-left with
                // dead space on the right and bottom). `.max(0.0)` keeps
                // this a no-op the moment content is at least as big as the
                // viewport, so the documented FlexStart/full-reachability
                // guarantee right below is untouched whenever scrolling is
                // actually needed.
                let viewport_width = if session.analysis_graph_viewport_width > 16.0 {
                    session.analysis_graph_viewport_width
                } else {
                    scaled_canvas_width
                };
                let viewport_height = if session.analysis_graph_viewport_height > 16.0 {
                    session.analysis_graph_viewport_height
                } else {
                    scaled_canvas_height
                };
                let horizontal_slack = ((viewport_width - scaled_canvas_width) / 2.0).max(0.0);
                let vertical_slack = ((viewport_height - scaled_canvas_height) / 2.0).max(0.0);
                let selected_lineage = session.selected_analysis_node.as_deref().and_then(|id| {
                    let node_id = app_core::AnalysisNodeId::new(id);
                    render_graph
                        .node(&node_id)
                        .map(|_| compute_analysis_lineage(&edge_pairs, &node_id))
                });
                // Center the actual DAG in a full-page workspace and keep the
                // legend attached. Run history stays in the Activity panel so
                // this surface remains one fitted 1080p composition.

                session_card
                    .spawn(Node {
                        position_type: PositionType::Relative,
                        width: percent(100),
                        min_width: px(0),
                        max_width: percent(100),
                        min_height: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|frame| {
                        frame
                            .spawn((
                                AnalysisGraphViewport {
                                    unscaled_width: canvas_width,
                                    unscaled_height: canvas_height,
                                },
                                UiPointerApi(&["ui.pointer.analysis_viewport_pan"]),
                                ScrollPosition(Vec2::new(
                                    session.analysis_graph_scroll_offset,
                                    session.analysis_graph_vertical_scroll_offset,
                                )),
                                Node {
                                    width: percent(100),
                                    min_width: px(0),
                                    max_width: percent(100),
                                    // §12: flex_grow lets the viewport claim
                                    // the extra height the full-width legend
                                    // row used to occupy below it -- that
                                    // row is now a compact in-viewport
                                    // overlay (§11) instead of consuming its
                                    // own flex slot.
                                    height: vh(ANALYSIS_GRAPH_VIEWPORT_VH),
                                    min_height: px(520),
                                    flex_grow: 1.0,
                                    flex_shrink: 0.0,
                                    // An oversized scroll child must start at the
                                    // viewport origin. Centering it makes its left/top
                                    // half unreachable at Fit's minimum zoom, which is
                                    // why the old canvas opened in the middle of the
                                    // vocal chain instead of at Preflight.
                                    justify_content: JustifyContent::FlexStart,
                                    align_items: AlignItems::FlexStart,
                                    overflow: Overflow::scroll(),
                                    ..default()
                                },
                                // No card frame here (unlike other panels):
                                // the DAG canvas fills the workspace and
                                // should read as part of the app background,
                                // not a boxed panel floating on top of it --
                                // a bordered card looked mismatched against
                                // the app chrome at high window transparency.
                                BackgroundColor(Color::NONE),
                            ))
                            .with_children(|viewport| {
                                viewport
                                    .spawn(Node {
                                        width: px(scaled_canvas_width),
                                        height: px(scaled_canvas_height),
                                        margin: UiRect {
                                            left: px(horizontal_slack),
                                            top: px(vertical_slack),
                                            ..default()
                                        },
                                        flex_shrink: 0.0,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    })
                                    .with_children(|canvas| {
                                        canvas
                                            .spawn(Node {
                                                position_type: PositionType::Relative,
                                                width: percent(100),
                                                min_width: px(scaled_canvas_width),
                                                height: px(scaled_canvas_height),
                                                flex_shrink: 0.0,
                                                ..default()
                                            })
                                            .with_children(|graph| {
                                                // Round-7 feedback: clicking empty
                                                // canvas clears the selected node (and
                                                // its lineage highlight). Spawned
                                                // first (and left unstyled) so real
                                                // nodes/edges/bands paint over it and
                                                // only genuinely blank space is
                                                // reachable.
                                                graph.spawn((
                                                    UiPointerApi(&[
                                                        "ui.pointer.dismiss_analysis_selection",
                                                    ]),
                                                    Node {
                                                        position_type: PositionType::Absolute,
                                                        left: px(0),
                                                        top: px(0),
                                                        right: px(0),
                                                        bottom: px(0),
                                                        ..default()
                                                    },
                                                )).observe(
                                                    |mut event: On<Pointer<Click>>,
                                                     mut analysis: ResMut<AnalysisUiState>,
                                                     mut invalidated: ResMut<UiInvalidated>| {
                                                        if event.button != PointerButton::Primary {
                                                            return;
                                                        }
                                                        event.propagate(false);
                                                        if analysis.selected_analysis_node.is_none()
                                                        {
                                                            return;
                                                        }
                                                        analysis.selected_analysis_node = None;
                                                        invalidated
                                                            .invalidate(UiDirtyRegion::Analysis);
                                                    },
                                                );
                                                let Some(routed) = routed.as_ref() else {
                                                    return;
                                                };
                                                let stage_bands = compute_analysis_stage_bands(
                                                    &routed.layout,
                                                    &steps,
                                                );
                                                spawn_analysis_stage_bands(
                                                    graph,
                                                    font.clone(),
                                                    theme,
                                                    &stage_bands,
                                                    zoom,
                                                );
                                                spawn_analysis_graph_edges(
                                                    graph,
                                                    theme,
                                                    routed,
                                                    &render_graph,
                                                    selected_lineage.as_ref(),
                                                    zoom,
                                                );
                                                // Topological order is retained by the
                                                // layered layout; real semantic
                                                // bindings drive both node rank and
                                                // the edges drawn above.
                                                for node in &render_graph.nodes {
                                                    let Some(rect) = routed.layout.rect(&node.id)
                                                    else {
                                                        continue;
                                                    };
                                                    let bounds = zoomed_box(rect, zoom);
                                                    let lineage_dimmed =
                                                        selected_lineage.as_ref().is_some_and(
                                                            |lineage| !lineage.contains(&node.id),
                                                        );
                                                    match node.kind {
                                                        RenderNodeKind::Compute => {
                                                            let capability_id = node
                                                                .capability_id
                                                                .as_deref()
                                                                .unwrap_or("workflow");
                                                            let (state, override_text) =
                                                                graph_node_visual_state(
                                                                    node.state,
                                                                    analysis_graph_node_progress(
                                                                        task, node,
                                                                    ),
                                                                );
                                                            let AnalysisGraphRouteSummary {
                                                                mut model_ids,
                                                                runtime: mut route,
                                                                mut warning,
                                                            } = analysis_graph_route_summary(
                                                                task,
                                                                node,
                                                                node.state
                                                                    == GraphNodeState::Complete,
                                                            );
                                                            if model_ids.is_empty() {
                                                                model_ids
                                                                    .clone_from(&node.model_ids);
                                                            }
                                                            if let Some(text) = override_text {
                                                                // Explain why a configured card was not
                                                                // selected by this exact execution plan.
                                                                route = not_requested_reason(
                                                                    node.state,
                                                                    capability_id,
                                                                )
                                                                .unwrap_or(text)
                                                                .to_string();
                                                                warning = node.state
                                                                    == GraphNodeState::Failed;
                                                            } else if matches!(
                                                                route.as_str(),
                                                                "Complete · no runtime trace"
                                                                    | "Awaiting connected inputs"
                                                            ) {
                                                                route = node.detail.clone();
                                                            }
                                                            if let Some(output) =
                                                                node.terminal_outputs.first()
                                                            {
                                                                let semantic = output
                                                                    .audio_role
                                                                    .as_deref()
                                                                    .map(|role| {
                                                                        format!(
                                                                            "{} · {role}",
                                                                            output.semantic_type
                                                                        )
                                                                    })
                                                                    .unwrap_or_else(|| {
                                                                        output.semantic_type.clone()
                                                                    });
                                                                route = format!(
                                                                    "{route} · {} → {semantic}",
                                                                    output.port
                                                                );
                                                            }
                                                            spawn_workflow_graph_node(
                                                                graph,
                                                                font.clone(),
                                                                theme,
                                                                WorkflowNodeCardSpec {
                                                                    bounds,
                                                                    capability_id,
                                                                    node_id: node.id.as_str(),
                                                                    file_hash: &task.file_hash,
                                                                    label: &node.label,
                                                                    model_ids: &model_ids,
                                                                    state,
                                                                    selected: session
                                                                        .selected_analysis_node
                                                                        .as_deref()
                                                                        == Some(node.id.as_str()),
                                                                    route: &route,
                                                                    warning,
                                                                    dimmed: lineage_dimmed,
                                                                    zoom,
                                                                    input_ports: 0,
                                                                    output_ports: 0,
                                                                    category: node.category,
                                                                },
                                                            );
                                                        }
                                                    }
                                                }
                                            });
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
                                    let Ok((computed, mut position)) = viewports.single_mut()
                                    else {
                                        return;
                                    };
                                    let size = computed.size() * computed.inverse_scale_factor();
                                    let content =
                                        computed.content_size() * computed.inverse_scale_factor();
                                    let delta = drag.delta / ui_scale.0;
                                    position.x = (position.x - delta.x)
                                        .clamp(0.0, (content.x - size.x).max(0.0));
                                    position.y = (position.y - delta.y)
                                        .clamp(0.0, (content.y - size.y).max(0.0));
                                    analysis.analysis_graph_scroll_offset = position.x;
                                    analysis.analysis_graph_vertical_scroll_offset = position.y;
                                    // §10: a manual pan always pauses Follow; the
                                    // user must click Follow again to resume it.
                                    analysis.analysis_graph_follow_enabled = false;
                                },
                            );
                        // Both overlays stay fixed outside the scrolling
                        // viewport so neither scrolls away with the canvas
                        // content: the pan hint in the bottom-left corner,
                        // the Fit/Zoom/Follow cluster in the bottom-right.
                        spawn_analysis_graph_pan_hint(frame, font.clone(), theme);
                        spawn_analysis_graph_viewport_controls(
                            frame,
                            font.clone(),
                            theme,
                            zoom,
                            follow_active,
                            follow_available,
                        );
                    });
            }

            if inspect_only {
                let status_color = analysis_audit_status_color(selected_status, theme);
                let source_color = if viewing_history {
                    theme.pitch_contour
                } else {
                    theme.primary
                };
                let (source_label, source_copy) = if viewing_history {
                    (
                        "Historical evidence",
                        "This record is frozen from the selected analysis run. Current settings cannot rewrite it.",
                    )
                } else if task.live.is_some() {
                    (
                        "Live execution",
                        "Runtime evidence updates as the selected node advances through this analysis run.",
                    )
                } else {
                    (
                        "Compiled plan",
                        "This is the declared node contract. Runtime evidence will appear after execution starts.",
                    )
                };
                let selected_capability = selected_node
                    .and_then(|node| node.capability_id.as_deref())
                    .unwrap_or("workflow");
                let progress_copy = if selected_progress_is_reported {
                    format!("{selected_progress}%")
                } else {
                    "Not reported".to_string()
                };
                let selected_input_copy = if selected_input.trim().is_empty() {
                    "No declared input artifacts".to_string()
                } else {
                    selected_input.clone()
                };
                let selected_output_copy = if selected_output.trim().is_empty() {
                    "No declared output artifacts".to_string()
                } else {
                    selected_output.clone()
                };
                let has_fallback =
                    selected_device_fallback.is_some() || selected_backend_fallback.is_some();
                let has_error = history_error.is_some();

                session_card
                    .spawn((
                        Node {
                            width: percent(100),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(18)),
                            row_gap: px(11),
                            border: UiRect::all(px(1)),
                            border_radius: studio_card_radius(),
                            ..default()
                        },
                        BackgroundColor(theme.card.with_alpha(0.58)),
                        BorderColor::all(theme.primary.with_alpha(0.28)),
                        studio_card_shadow(theme),
                    ))
                    .with_children(|hero| {
                        hero.spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(7),
                            row_gap: px(6),
                            ..default()
                        })
                        .with_children(|meta| {
                            spawn_text(
                                meta,
                                font.clone(),
                                "NODE AUDIT RECORD",
                                7.5,
                                theme.primary,
                            );
                            meta.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_status_pill(
                                meta,
                                font.clone(),
                                source_label,
                                source_color,
                            );
                            spawn_status_pill(
                                meta,
                                font.clone(),
                                selected_status,
                                status_color,
                            );
                        });
                        spawn_bounded_wrapped_text(
                            hero,
                            font.clone(),
                            format!("Step {selected_step:02} · {selected_label}"),
                            20.0,
                            theme.foreground,
                        );
                        spawn_bounded_wrapped_text(
                            hero,
                            font.clone(),
                            capability_product_label(selected_capability),
                            10.0,
                            theme.muted_foreground,
                        );
                        hero.spawn((
                            Node {
                                width: percent(100),
                                padding: UiRect::axes(px(9), px(7)),
                                overflow: Overflow::clip(),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(7)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.28)),
                            BorderColor::all(theme.border.with_alpha(0.34)),
                            children![(
                                Text::new(selected_node_id.to_string()),
                                ui_text_font(font.clone(), 8.0),
                                TextColor(theme.muted_foreground),
                                TextLayout::no_wrap(),
                            )],
                        ));
                        spawn_wrapped_text(
                            hero,
                            font.clone(),
                            selected_purpose,
                            9.5,
                            theme.muted_foreground,
                        );
                        hero.spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|progress| {
                            spawn_text(
                                progress,
                                font.clone(),
                                "EXECUTION PROGRESS",
                                7.5,
                                theme.muted_foreground,
                            );
                            progress.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                progress,
                                font.clone(),
                                progress_copy,
                                9.0,
                                status_color,
                            );
                        });
                        spawn_progress_bar(
                            hero,
                            theme,
                            if selected_progress_is_reported {
                                selected_progress
                            } else {
                                0
                            },
                            status_color,
                        );
                        spawn_analysis_audit_callout(
                            hero,
                            font.clone(),
                            theme,
                            "CURRENT OPERATION",
                            selected_operation,
                            status_color,
                        );
                        spawn_wrapped_text(
                            hero,
                            font.clone(),
                            source_copy,
                            8.0,
                            theme.muted_foreground.with_alpha(0.78),
                        );
                    });

                spawn_analysis_audit_section(
                    session_card,
                    font.clone(),
                    theme,
                    "01 · EXECUTION ROUTE",
                    "How this node was resolved",
                    "The actual implementation and compute route used for this attempt.",
                    |section| {
                        section
                            .spawn(Node {
                                width: percent(100),
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(9),
                                row_gap: px(9),
                                ..default()
                            })
                            .with_children(|facts| {
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "IMPLEMENTATION",
                                    selected_implementation,
                                    theme.foreground,
                                    190.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "MODEL / ALGORITHM",
                                    selected_model,
                                    theme.primary,
                                    190.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "ACTUAL COMPUTE",
                                    selected_actual_device,
                                    theme.pitch_contour,
                                    170.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "WORKER TASK",
                                    selected_worker_task_text.clone(),
                                    theme.foreground,
                                    210.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "DURATION",
                                    selected_duration_text.clone(),
                                    theme.foreground,
                                    150.0,
                                );
                            });
                    },
                );

                spawn_analysis_audit_section(
                    session_card,
                    font.clone(),
                    theme,
                    "02 · DATA CONTRACT",
                    "Declared inputs and outputs",
                    "Artifacts are shown by semantic type so the node can be audited independently of file paths.",
                    |section| {
                        section
                            .spawn(Node {
                                width: percent(100),
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(9),
                                row_gap: px(9),
                                ..default()
                            })
                            .with_children(|facts| {
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "INPUT ARTIFACTS",
                                    selected_input_copy,
                                    theme.primary,
                                    300.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "OUTPUT ARTIFACTS",
                                    selected_output_copy,
                                    theme.pitch_contour,
                                    300.0,
                                );
                            });
                    },
                );

                spawn_analysis_audit_section(
                    session_card,
                    font.clone(),
                    theme,
                    "03 · AUDIT TRAIL",
                    "Fallbacks and failures",
                    "Only recorded runtime evidence appears here; absence of evidence is never inferred as success.",
                    |section| {
                        section
                            .spawn(Node {
                                width: percent(100),
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(9),
                                row_gap: px(9),
                                ..default()
                            })
                            .with_children(|facts| {
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "FALLBACK",
                                    selected_fallback_text.clone(),
                                    if has_fallback {
                                        theme.editor_warning
                                    } else {
                                        theme.muted_foreground
                                    },
                                    260.0,
                                );
                                spawn_analysis_audit_fact(
                                    facts,
                                    font.clone(),
                                    theme,
                                    "SESSION ERROR",
                                    selected_error_text.clone(),
                                    if has_error {
                                        theme.destructive
                                    } else {
                                        theme.muted_foreground
                                    },
                                    260.0,
                                );
                            });
                        for (label, from, to, reason) in selected_device_fallback
                            .map(|(from, reason)| {
                                ("COMPUTE FALLBACK", from, selected_actual_device, reason)
                            })
                            .into_iter()
                            .chain(selected_backend_fallback.map(|(from, reason)| {
                                ("MODEL FALLBACK", from, selected_implementation, reason)
                            }))
                        {
                            spawn_analysis_audit_callout(
                                section,
                                font.clone(),
                                theme,
                                label,
                                format!(
                                    "{} → {} · {reason}",
                                    from.to_ascii_uppercase(),
                                    to.to_ascii_uppercase()
                                ),
                                theme.editor_warning,
                            );
                        }
                        if let Some(error) = history_error.as_deref() {
                            spawn_analysis_audit_callout(
                                section,
                                font.clone(),
                                theme,
                                "RECORDED SESSION ERROR",
                                error,
                                theme.destructive,
                            );
                        }
                        if !has_fallback && !has_error {
                            spawn_analysis_audit_callout(
                                section,
                                font.clone(),
                                theme,
                                "NO EXCEPTION RECORDED",
                                if matches!(selected_status, "WAITING" | "RUNNING") {
                                    "No fallback or session error has been recorded yet."
                                } else {
                                    "This node has no recorded fallback or session error for the selected run."
                                },
                                theme.pitch_contour,
                            );
                        }
                    },
                );
            }
        });
}

#[cfg(test)]
mod source_selection_tests {
    use super::*;

    #[test]
    fn selected_song_rejects_other_song_runtime_evidence() {
        let selected = "song-a".to_string();
        assert!(item_matches_selected_song("song-a", Some(&selected)));
        assert!(!item_matches_selected_song("song-b", Some(&selected)));
    }

    #[test]
    fn frozen_execution_context_never_falls_back_to_the_current_editable_snapshot() {
        let current = "current draft";
        assert_eq!(select_context_snapshot(true, None, Some(&current)), None);
    }

    #[test]
    fn not_requested_reason_explains_only_the_stars_capabilities() {
        assert_eq!(
            not_requested_reason(GraphNodeState::NotRequested, "notes.stars"),
            Some("Not requested · STARS requires Chinese/Cantonese lyrics")
        );
        assert_eq!(
            not_requested_reason(GraphNodeState::NotRequested, "technique.analyze"),
            Some("Not requested · STARS requires Chinese/Cantonese lyrics")
        );
        assert_eq!(
            not_requested_reason(GraphNodeState::NotRequested, "notes.game"),
            None
        );
        // A capability id that happens to be STARS-backed but isn't in the
        // NotRequested state must not be relabeled -- other states already
        // carry their own self-explanatory override text.
        assert_eq!(
            not_requested_reason(GraphNodeState::Deferred, "notes.stars"),
            None
        );
    }

    #[test]
    fn fusion_adapter_errors_are_presented_as_typed_actionable_failures() {
        let copy = |code: &str| {
            analysis_engine_error_copy(&app_core::EngineErrorHistoryProjection {
                code: code.to_string(),
                message: "technical detail".to_string(),
                retryable: false,
                request_id: None,
                capability: Some("fusion.candidate_graph".to_string()),
                resource: Some("tool:fusion_agent_adapter".to_string()),
            })
        };
        assert!(copy("worker_timeout").contains("600 seconds"));
        assert!(copy("cancelled").contains("cancelled"));
        assert!(copy("worker_protocol_mismatch").contains("protocol mismatch"));
        assert!(copy("worker_unavailable").contains("Settings > Models & runtime"));
        assert!(copy("worker_failed").contains("external AI provider"));
        assert!(copy("output_validation_failed").contains("invalid candidate selection"));
        for code in [
            "worker_timeout",
            "cancelled",
            "worker_protocol_mismatch",
            "worker_unavailable",
            "worker_failed",
            "output_validation_failed",
        ] {
            assert!(copy(code).contains("No Algorithm fallback"));
        }
    }

    #[test]
    fn frozen_execution_snapshot_wins_outside_history_too() {
        let frozen = "engine request";
        let current = "current draft";
        assert_eq!(
            select_context_snapshot(false, Some(&frozen), Some(&current)),
            Some(&frozen)
        );
    }
}
