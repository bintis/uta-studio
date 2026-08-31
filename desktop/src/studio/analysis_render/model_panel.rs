use crate::studio::*;

pub(crate) const ANALYSIS_MODEL_PANEL_WIDTH: f32 = 338.0;

#[derive(Component)]
pub(crate) struct AnalysisModelPanelScroll;

fn workflow_node_for_presentation<'a>(
    workflow: &'a app_core::WorkflowExecutionWireV1,
    node_id: &str,
) -> Option<(
    &'a app_core::WorkflowNodeWireV1,
    Option<&'static str>,
    Option<&'static str>,
)> {
    if let Some(node) = workflow
        .nodes
        .iter()
        .find(|node| node.instance_id == node_id)
    {
        return Some((node, None, None));
    }
    for (suffix, capability, output_port) in [
        (".vocal", "audio.extract_vocals", "vocal"),
        (
            ".instrumental",
            "audio.extract_instrumental",
            "instrumental",
        ),
    ] {
        let Some(base) = node_id.strip_suffix(suffix) else {
            continue;
        };
        if let Some(node) = workflow.nodes.iter().find(|node| {
            node.instance_id == base && node.capability_id == "audio.separate_vocal_bgm"
        }) {
            return Some((node, Some(capability), Some(output_port)));
        }
    }
    None
}

fn presentation_model(
    node: &app_core::WorkflowNodeWireV1,
    concrete_capability: Option<&str>,
) -> Option<String> {
    if concrete_capability == Some("audio.extract_instrumental") {
        return node.provider_preferences.instrumental.clone();
    }
    node.provider_preferences.primary.clone()
}

pub(crate) fn spawn_analysis_header_toolbar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    file_hash: &str,
) {
    // §2: label reflects the real scope of `ForceStopAllAnalysis` -- it
    // stops every active analysis, not only the one for this song -- using
    // the same active-task count the sidebar/Activity badge already
    // computes, so the number always matches what actually gets stopped.
    let active_count = active_analysis_task_count(session.analysis_tasks);
    if active_count > 0 {
        spawn_toolbar_button(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            UiIcon::Close,
            if active_count > 1 {
                format!("Stop all analyses ({active_count})")
            } else {
                "Stop analysis".to_string()
            },
            UiAction::from(AnalysisCommand::ForceStopAllAnalysis),
            true,
        );
    }
    spawn_toolbar_button(
        parent,
        font,
        icons,
        theme,
        UiIcon::Settings,
        "Edit workflow",
        UiAction::from(AnalysisCommand::OpenProcessingStudio(file_hash.to_string())),
        false,
    );
}

fn snapshot_node_status(
    snapshot: &app_core::AnalysisProgressSnapshot,
    node_id: &str,
    live_running: bool,
) -> Option<String> {
    let route = snapshot
        .stage_routes
        .iter()
        .rev()
        .find(|route| route.node_id.as_deref() == Some(node_id));
    let route_status = route.and_then(|route| match route.node_event.as_deref() {
        Some("node_failed" | "failed") => Some("FAILED"),
        Some("node_completed" | "artifact_reused" | "completed" | "reused") => Some("COMPLETE"),
        Some("node_cancelled" | "cancelled") => Some("CANCELLED"),
        Some("node_skipped" | "skipped") => Some("SKIPPED"),
        Some("node_started" | "node_progress" | "started" | "progress") if live_running => {
            Some("RUNNING")
        }
        _ if route.finished_at_ms.is_some() => Some("COMPLETE"),
        _ => None,
    });
    route_status
        .map(str::to_string)
        .or_else(|| {
            (live_running && snapshot.node_id.as_deref() == Some(node_id))
                .then(|| "RUNNING".to_string())
        })
        .or_else(|| {
            matches!(
                snapshot.node_event.as_deref(),
                Some("cancelled" | "node_cancelled")
            )
            .then(|| "CANCELLED".to_string())
        })
}

fn planned_node_status(
    engine: &app_core::EngineRunHistoryProjection,
    node_id: &str,
    run_completed: bool,
) -> Option<String> {
    let (workflow, plan) = exact_workflow_plan_from_engine(engine)?;
    let (_, concrete_capability, _) = workflow_node_for_presentation(&workflow, node_id)?;
    let analysis_node = node_id
        .strip_suffix(".vocal")
        .or_else(|| node_id.strip_suffix(".instrumental"))
        .unwrap_or(node_id);
    let state = plan?
        .nodes
        .into_iter()
        .find(|node| node.analysis_node == analysis_node)?
        .execution_state;
    let concrete_not_requested = concrete_capability.is_some_and(|capability| {
        exact_engine_capabilities_from_engine(engine)
            .is_some_and(|planned| !planned.contains(capability))
    });
    Some(
        match state {
            app_core::WorkflowNodeExecutionStateWireV1::Ready if concrete_not_requested => {
                "NOT REQUESTED"
            }
            app_core::WorkflowNodeExecutionStateWireV1::Ready if run_completed => "COMPLETE",
            app_core::WorkflowNodeExecutionStateWireV1::Ready => "WAITING",
            app_core::WorkflowNodeExecutionStateWireV1::Deferred => "DEFERRED",
            app_core::WorkflowNodeExecutionStateWireV1::Disabled => "DISABLED",
            app_core::WorkflowNodeExecutionStateWireV1::ProfileSkipped => "PROFILE SKIPPED",
            app_core::WorkflowNodeExecutionStateWireV1::NotRequested => "NOT REQUESTED",
        }
        .to_string(),
    )
}

pub(crate) fn current_analysis_model_panel_context(
    session: &StudioSessionView<'_>,
) -> (String, String) {
    let workflow = selected_workflow_wire(session);
    let node_id = session
        .selected_analysis_node
        .as_ref()
        .filter(|node_id| {
            workflow
                .as_ref()
                .is_some_and(|workflow| workflow_node_for_presentation(workflow, node_id).is_some())
        })
        .cloned()
        .or_else(|| {
            session.selected_analysis_history.and_then(|id| {
                session.analysis_history.iter().find_map(|history| {
                    (history.id == id
                        && session
                            .selected_song
                            .as_ref()
                            .is_none_or(|hash| hash == &history.file_hash))
                    .then(|| history.snapshot.node_id.clone())
                    .flatten()
                })
            })
        })
        .or_else(|| {
            session.analysis_tasks.iter().find_map(|task| {
                if !matches!(task.status, app_core::QueuedStatus::Analyzing(_))
                    || session
                        .selected_song
                        .as_ref()
                        .is_some_and(|hash| hash != &task.file_hash)
                {
                    return None;
                }
                task.live.as_ref()?.node_id.clone()
            })
        })
        .or_else(|| {
            workflow
                .as_ref()
                .and_then(|workflow| workflow.nodes.first())
                .map(|node| node.instance_id.clone())
        })
        .unwrap_or_else(|| "workflow".to_string());
    let selected_history = session.selected_analysis_history.and_then(|id| {
        session.analysis_history.iter().find(|history| {
            history.id == id
                && session
                    .selected_song
                    .as_ref()
                    .is_none_or(|hash| hash == &history.file_hash)
        })
    });
    let active_task = selected_history.is_none().then(|| {
        session
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
            })
    });
    let snapshot = selected_history
        .map(|history| (&history.snapshot, false, history.status == "completed"))
        .or_else(|| {
            active_task.flatten().and_then(|task| {
                task.live.as_ref().map(|snapshot| {
                    (
                        snapshot,
                        matches!(task.status, app_core::QueuedStatus::Analyzing(_)),
                        false,
                    )
                })
            })
        });
    let status = snapshot
        .and_then(|(snapshot, live_running, run_completed)| {
            snapshot_node_status(snapshot, &node_id, live_running).or_else(|| {
                snapshot
                    .engine
                    .as_ref()
                    .and_then(|engine| planned_node_status(engine, &node_id, run_completed))
            })
        })
        .or_else(|| {
            workflow.as_ref().and_then(|workflow| {
                workflow_node_for_presentation(workflow, &node_id)
                    .map(|(node, _, _)| node.execution_policy.to_ascii_uppercase())
            })
        })
        .unwrap_or_else(|| "UNAVAILABLE".to_string());
    (node_id, status)
}

fn has_selected_history_for_song(session: &StudioSessionView<'_>) -> bool {
    session.selected_analysis_history.is_some_and(|id| {
        session.analysis_history.iter().any(|history| {
            history.id == id
                && session
                    .selected_song
                    .as_ref()
                    .is_none_or(|hash| hash == &history.file_hash)
        })
    })
}

fn selected_workflow_wire(
    session: &StudioSessionView<'_>,
) -> Option<app_core::WorkflowExecutionWireV1> {
    let selected_history = session.selected_analysis_history.and_then(|id| {
        session.analysis_history.iter().find(|history| {
            history.id == id
                && session
                    .selected_song
                    .as_ref()
                    .is_none_or(|hash| hash == &history.file_hash)
        })
    });
    if let Some(history) = selected_history {
        return history
            .snapshot
            .engine
            .as_ref()
            .and_then(exact_workflow_plan_from_engine)
            .map(|(workflow, _)| workflow);
    }

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
    if let Some(task) = active_task {
        return task
            .live
            .as_ref()
            .and_then(|snapshot| snapshot.engine.as_ref())
            .and_then(exact_workflow_plan_from_engine)
            .map(|(workflow, _)| workflow);
    }

    session
        .workflow_snapshot
        .as_ref()
        .and_then(|snapshot| app_core::WorkflowExecutionWireV1::from_snapshot(snapshot).ok())
}

fn spawn_fact(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    value: impl Into<String>,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                padding: UiRect::all(px(9)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.34)),
            BorderColor::all(theme.border.with_alpha(0.48)),
        ))
        .with_children(|fact| {
            spawn_text(fact, font.clone(), label, 8.0, theme.muted_foreground);
            spawn_bounded_wrapped_text(fact, font, value, 10.0, theme.foreground);
        });
}

pub(crate) fn spawn_analysis_model_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    current_node_id: &str,
    current_status: &str,
) {
    let workflow = selected_workflow_wire(session);
    let selected = workflow
        .as_ref()
        .and_then(|workflow| workflow_node_for_presentation(workflow, current_node_id));

    parent
        .spawn((
            AnalysisModelPanelScroll,
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(ANALYSIS_MODEL_PANEL_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(9),
                padding: UiRect::all(px(12)),
                overflow: Overflow::scroll_y(),
                border: UiRect::left(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border),
            ZIndex(10),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|header| {
                    spawn_text(
                        header,
                        font.clone(),
                        "MODEL & WORKFLOW",
                        9.0,
                        theme.primary,
                    );
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "Close",
                        9.0,
                        UiAction::from(AnalysisCommand::CloseAnalysisModelPanel),
                    );
                });

            spawn_wrapped_text(
                panel,
                font.clone(),
                "1. Select a graph node.  2. Read the model/runtime chosen for that step.  3. Use Processing Studio to change workflow/model intent, then confirm the exact route in Plan Preview.",
                9.0,
                theme.muted_foreground,
            );
            spawn_fact(panel, font.clone(), theme, "1 · SELECTED STEP", current_node_id);
            spawn_fact(
                panel,
                font.clone(),
                theme,
                "CURRENT STATE",
                current_status,
            );

            if let Some((node, concrete_capability, output_port)) = selected {
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "CAPABILITY",
                    concrete_capability
                        .unwrap_or(node.capability_id.as_str())
                        .to_string(),
                );
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "2 · CURRENT MODEL",
                    presentation_model(node, concrete_capability)
                        .unwrap_or_else(|| "Studio / native DSP".to_string()),
                );
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "CURRENT RUNTIME",
                    "Engine-resolved in Plan Preview".to_string(),
                );
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "WHEN THIS STEP RUNS",
                    node.execution_policy.clone(),
                );
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "SCHEDULING PRIORITY",
                    node.priority.to_string(),
                );
                if let Some(workflow) = workflow.as_ref() {
                    for output in workflow
                        .terminal_outputs
                        .iter()
                        .filter(|output| {
                            output.node == node.instance_id
                                && output_port.is_none_or(|port| output.port == port)
                        })
                    {
                        let semantic = output
                            .audio_role
                            .as_deref()
                            .map(|role| format!("{} · {role}", output.semantic_type))
                            .unwrap_or_else(|| output.semantic_type.clone());
                        spawn_fact(
                            panel,
                            font.clone(),
                            theme,
                            &format!("TERMINAL OUTPUT · {}", output.port),
                            semantic,
                        );
                    }
                }
            } else {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "This node is not present in the selected compiled snapshot.",
                    9.0,
                    theme.destructive,
                );
            }

            if let Some(workflow) = workflow.as_ref() {
                spawn_fact(
                    panel,
                    font.clone(),
                    theme,
                    "WORKFLOW SNAPSHOT",
                    format!("Revision {}", workflow.workflow_revision),
                );
            }

            if !has_selected_history_for_song(session)
                && let Some(file_hash) = session.selected_song.as_ref()
            {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "3 · CHANGE SELECTION",
                    8.0,
                    theme.primary,
                );
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "Processing Studio changes which model/capability implementation this workflow asks for. Models & runtime only installs resources and controls backend availability.",
                    9.0,
                    theme.muted_foreground,
                );
                spawn_text_button(
                    panel,
                    font.clone(),
                    theme,
                    "Open Processing Studio",
                    10.0,
                    UiAction::from(AnalysisCommand::OpenProcessingStudio(file_hash.clone())),
                );
                spawn_text_button(
                    panel,
                    font.clone(),
                    theme,
                    "Manage installed models",
                    9.0,
                    UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
                );
            }
        });
}

#[cfg(test)]
mod execution_status_tests {
    use super::*;
    use serde_json::json;

    fn snapshot(route: serde_json::Value) -> app_core::AnalysisProgressSnapshot {
        serde_json::from_value(json!({
            "stage": "shared text",
            "overall_progress": 50,
            "stage_progress": 50,
            "operation": "operation",
            "detail": "",
            "implementation": "native",
            "model": "model",
            "device": "vulkan",
            "requested_device": "vulkan",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "stage_routes": [route]
        }))
        .unwrap()
    }

    #[test]
    fn split_presentation_nodes_report_their_independent_models() {
        let snapshot = app_core::compile_workflow(&app_core::default_workflow("song")).unwrap();
        let workflow = app_core::WorkflowExecutionWireV1::from_snapshot(&snapshot).unwrap();
        let (vocal, vocal_capability, _) =
            workflow_node_for_presentation(&workflow, "vocal_bgm_split.vocal").unwrap();
        let (instrumental, instrumental_capability, _) =
            workflow_node_for_presentation(&workflow, "vocal_bgm_split.instrumental").unwrap();
        assert_eq!(vocal_capability, Some("audio.extract_vocals"));
        assert_eq!(
            presentation_model(vocal, vocal_capability).as_deref(),
            Some("bs_roformer_leap_xe90_vocals")
        );
        assert_eq!(
            presentation_model(instrumental, instrumental_capability).as_deref(),
            Some("bs_polarformer_public_instrumental")
        );
    }

    #[test]
    fn inspector_status_ignores_display_text_without_an_exact_node_id() {
        let snapshot = snapshot(json!({
            "stage": "shared text",
            "node_event": "node_failed",
            "operation": "failed",
            "implementation": "native",
            "model": "model",
            "stage_progress": 100,
            "requested_device": "vulkan",
            "actual_device": "vulkan",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "finished_at_ms": 2
        }));
        assert_eq!(
            snapshot_node_status(&snapshot, "workflow.node", false),
            None
        );
    }
}
