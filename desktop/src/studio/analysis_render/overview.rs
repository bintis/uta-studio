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

fn spawn_analysis_history_below_graph(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
    live_available: bool,
) {
    let history_items = session
        .analysis_history
        .iter()
        .filter(|history| {
            session
                .selected_song
                .as_ref()
                .is_none_or(|hash| hash == &history.file_hash)
        })
        .take(8)
        .collect::<Vec<_>>();
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                margin: UiRect::top(px(22)),
                padding: UiRect::axes(px(12), px(10)),
                row_gap: px(8),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            studio_card_background(theme),
            studio_card_border(theme),
        ))
        .with_children(|history| {
            history
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(5),
                    ..default()
                })
                .with_children(|heading| {
                    spawn_text(heading, font.clone(), "RECENT RUNS", 8.0, theme.primary);
                    heading.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    if live_available && selected_history_for_song(session).is_some() {
                        spawn_text_button(
                            heading,
                            font.clone(),
                            theme,
                            "Back to live",
                            8.0,
                            UiAction::from(AnalysisCommand::SelectAnalysisHistory(None)),
                        );
                    }
                    if session.pending_analysis_history_clear {
                        spawn_text(
                            heading,
                            font.clone(),
                            "Delete all saved runs?",
                            8.0,
                            theme.destructive,
                        );
                        spawn_text_button(
                            heading,
                            font.clone(),
                            theme,
                            "Cancel",
                            8.0,
                            UiAction::from(AnalysisCommand::CancelClearAnalysisHistory),
                        );
                        spawn_text_button(
                            heading,
                            font.clone(),
                            theme,
                            "Delete runs",
                            8.0,
                            UiAction::from(AnalysisCommand::ConfirmClearAnalysisHistory),
                        );
                    } else if !history_items.is_empty() {
                        spawn_text_button(
                            heading,
                            font.clone(),
                            theme,
                            "Clear runs…",
                            8.0,
                            UiAction::from(AnalysisCommand::RequestClearAnalysisHistory),
                        );
                    }
                });
            if history_items.is_empty() {
                spawn_text(
                    history,
                    font,
                    "No previous analysis runs for this song.",
                    9.0,
                    theme.muted_foreground,
                );
                return;
            }
            history
                .spawn(Node {
                    width: percent(100),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(6),
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|items| {
                    for item in &history_items {
                        let selected = session.selected_analysis_history == Some(item.id);
                        let progress = if item.status == "completed" {
                            100
                        } else {
                            item.snapshot.overall_progress.clamp(0, 100)
                        };
                        spawn_text_button(
                            items,
                            font.clone(),
                            theme,
                            format!(
                                "{}{} · {} · {}%",
                                if selected { "• " } else { "" },
                                item.title,
                                item.status.to_ascii_uppercase(),
                                progress
                            ),
                            8.0,
                            UiAction::from(AnalysisCommand::SelectAnalysisHistory(Some(item.id))),
                        );
                    }
                });
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
            measured_work_unit_progress(route)
                .map(|(percent, _)| percent)
                .unwrap_or(0)
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
    let selected_progress_is_measured = selected_status == "COMPLETE"
        || selected_route
            .and_then(measured_work_unit_progress)
            .is_some()
        || task.live.as_ref().is_none_or(|live| live.engine.is_none());
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

            let active_node_progress = task.live.as_ref().and_then(|live| {
                let node_id = live.node_id.as_deref()?;
                live.stage_routes.iter().rev().find_map(|route| {
                    (route.node_id.as_deref() == Some(node_id)
                        && route.finished_at_ms.is_none()
                        && route.node_event.as_deref() == Some("node_progress"))
                    .then(|| measured_work_unit_progress(route).map(|(percent, _)| percent))
                    .flatten()
                })
            });
            if !inspect_only {
                // Every node and binding comes from the compiled Processing Studio
                // workflow selected above. Layout receives arbitrary workflow ids
                // and uses only topology plus compiled metadata.
                let render_graph = authoritative_render_graph.clone();
                let locale = effective_ui_locale(session.config);
                let layout_nodes = render_graph
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
                        let (column_span, row_span) =
                            analysis_node_tile_span(node.capability_id.as_deref());
                        spec.width *= column_span as f32;
                        spec.height *= row_span as f32;
                        (spec, workflow_graph_step(node.capability_id.as_deref()))
                    })
                    .collect::<Vec<_>>();
                let edge_pairs = render_graph.edge_pairs();
                let zoom = clamp_analysis_graph_zoom(session.analysis_graph_zoom);
                // The first frame uses a close reference fallback. Once Bevy
                // reports the real viewport, the Metro packer consumes that
                // exact space and only grows vertically when the tiles need it.
                let graph_viewport_height = if session.analysis_graph_viewport_height > 16.0 {
                    session.analysis_graph_viewport_height
                } else {
                    780.0
                };
                let target_canvas_width = if session.analysis_graph_viewport_width > 16.0 {
                    (session.analysis_graph_viewport_width - ANALYSIS_GRAPH_FIT_PADDING).max(780.0)
                } else {
                    1180.0
                };
                let target_canvas_height =
                    (graph_viewport_height - ANALYSIS_GRAPH_FIT_PADDING).max(500.0);
                let layout = metro_tile_layout_with_specs(
                    &layout_nodes,
                    &edge_pairs,
                    target_canvas_width,
                    target_canvas_height,
                );
                let canvas_width = layout
                    .as_ref()
                    .map_or(780.0, |layout| layout.canvas_width)
                    .max(780.0);
                let canvas_height = layout
                    .as_ref()
                    .map_or(450.0, |layout| layout.canvas_height)
                    .max(450.0);
                let scaled_canvas_width = canvas_width * zoom;
                let scaled_canvas_height = canvas_height * zoom;
                // Match the reference composition: center the actual DAG in a
                // full-page workspace, keep the legend attached to that page,
                // and let history begin below the first fold.

                session_card
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
                            height: vh(ANALYSIS_GRAPH_VIEWPORT_VH),
                            min_height: px(520),
                            flex_shrink: 0.0,
                            // An oversized scroll child must start at the
                            // viewport origin. Centering it makes its left/top
                            // half unreachable at Fit's minimum zoom, which is
                            // why the old canvas opened in the middle of the
                            // vocal chain instead of at Preflight.
                            justify_content: JustifyContent::FlexStart,
                            align_items: AlignItems::FlexStart,
                            overflow: Overflow::scroll(),
                            border: UiRect::all(px(1)),
                            border_radius: studio_card_radius(),
                            ..default()
                        },
                        studio_card_background(theme),
                        studio_card_border(theme),
                    ))
                    .with_children(|viewport| {
                        viewport
                            .spawn(Node {
                                width: percent(100),
                                min_width: px(scaled_canvas_width),
                                height: px(graph_viewport_height),
                                min_height: px(scaled_canvas_height),
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
                                        height: px(graph_viewport_height),
                                        flex_shrink: 0.0,
                                        ..default()
                                    })
                                    .with_children(|graph| {
                                        let Some(layout) = layout.as_ref() else {
                                            return;
                                        };
                                        // Topological order is retained by the
                                        // tile packer; the dense color surface
                                        // replaces connector rails and lane
                                        // chrome with direct operation cards.
                                        for node in &render_graph.nodes {
                                            let Some(rect) = layout.rect(&node.id) else {
                                                continue;
                                            };
                                            let bounds = zoomed_box(rect, zoom);
                                            let lineage_dimmed = false;
                                            match node.kind {
                                                RenderNodeKind::Compute => {
                                                    let capability_id = node
                                                        .capability_id
                                                        .as_deref()
                                                        .unwrap_or("workflow");
                                                    let (state, override_text) =
                                                        graph_node_visual_state(
                                                            node.state,
                                                            active_node_progress,
                                                        );
                                                    let (mut route, mut warning) =
                                                        analysis_graph_route_summary(
                                                            task,
                                                            node.id.as_str(),
                                                            node.state == GraphNodeState::Complete,
                                                        );
                                                    if let Some(text) = override_text {
                                                        // Explain why a configured card was not
                                                        // selected by this exact execution plan.
                                                        route = not_requested_reason(
                                                            node.state,
                                                            capability_id,
                                                        )
                                                        .unwrap_or(text)
                                                        .to_string();
                                                        warning =
                                                            node.state == GraphNodeState::Failed;
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
                                                            selected_run_id: session
                                                                .selected_analysis_history,
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
                            let Ok((computed, mut position)) = viewports.single_mut() else {
                                return;
                            };
                            let size = computed.size() * computed.inverse_scale_factor();
                            let content = computed.content_size() * computed.inverse_scale_factor();
                            let delta = drag.delta / ui_scale.0;
                            position.x =
                                (position.x - delta.x).clamp(0.0, (content.x - size.x).max(0.0));
                            position.y =
                                (position.y - delta.y).clamp(0.0, (content.y - size.y).max(0.0));
                            analysis.analysis_graph_scroll_offset = position.x;
                            analysis.analysis_graph_vertical_scroll_offset = position.y;
                        },
                    );
                spawn_analysis_graph_legend(session_card, font.clone(), theme);
                spawn_analysis_history_below_graph(
                    session_card,
                    font.clone(),
                    session,
                    theme,
                    active_task.is_some(),
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
                                        selected_step,
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
                                    if selected_progress_is_measured {
                                        format!("{selected_status} · {selected_progress}%")
                                    } else {
                                        format!("{selected_status} · measured progress unavailable")
                                    },
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
                        spawn_text(
                            inspector,
                            font.clone(),
                            "RUNTIME DETAILS",
                            8.0,
                            theme.primary,
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
                                // (native worker progress frames, Phase 7's
                                // "Duration 检查器字段" gap closed).
                                let fact_rows: Vec<(&str, String)> = vec![
                                    ("IMPLEMENTATION", selected_implementation.to_string()),
                                    ("MODEL", selected_model.to_string()),
                                    ("DEVICE", selected_actual_device.to_string()),
                                    ("INPUT", selected_input.to_string()),
                                    ("OUTPUT", selected_output.to_string()),
                                    ("DURATION", selected_duration_text.clone()),
                                    ("FALLBACK", selected_fallback_text.clone()),
                                    ("ERROR", selected_error_text.clone()),
                                ];
                                for (label, value) in fact_rows {
                                    let value_color =
                                        if label == "ERROR" && value != "None recorded" {
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
                        if let Some(error) = history_error.as_deref() {
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
