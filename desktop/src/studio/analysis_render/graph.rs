use crate::studio::*;

#[derive(Component)]
pub(crate) struct ActivityPanelScroll;

#[derive(Clone, Copy)]
pub(crate) struct AnalysisGraphBox {
    pub(crate) x: f32,
    pub(crate) y: f32,
    pub(crate) width: f32,
    pub(crate) height: f32,
}

impl AnalysisGraphBox {
    pub(crate) const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// DAG canvas zoom bounds (§7.8/§9.3 "DAG 支持 Pan、Zoom、Fit"). 1.0 is
/// unscaled. The default sits at the previous 160% "readable node" zoom
/// now that the inspect pane no longer steals the canvas; min/max stay
/// wide enough that Fit and +/- still have room around that default.
pub(crate) const ANALYSIS_GRAPH_ZOOM_MIN: f32 = 0.35;
pub(crate) const ANALYSIS_GRAPH_ZOOM_MAX: f32 = 2.4;
pub(crate) const ANALYSIS_GRAPH_ZOOM_STEP: f32 = 0.15;
pub(crate) const ANALYSIS_GRAPH_ZOOM_DEFAULT: f32 = 1.0;
/// Inset so a fitted graph is not flush against the viewport chrome.
pub(crate) const ANALYSIS_GRAPH_FIT_PADDING: f32 = 20.0;
/// Keeps the DAG and its legend inside the real client area even when native
/// Wayland decorations reduce it relative to an internal app screenshot.
pub(crate) const ANALYSIS_GRAPH_VIEWPORT_VH: f32 = 72.0;

/// Relative Metro-grid footprint for one compiled operation. Expensive audio,
/// ASR and alignment work gets a double-height tile; other model-backed work
/// gets extra horizontal room, while routing/finalization remains compact.
///
/// The Advanced Graph canvas no longer packs nodes into this Metro grid --
/// it renders the real layered/routed layout instead so real dependency
/// edges are visible (§3) and no card is ever two rows tall (§5) -- but the
/// function stays, tested, for `metro_tile_layout_with_specs`, which is
/// itself kept for the same reason `RoutedGraph::path` was: real, covered
/// geometry code, just not this page's current caller.
#[allow(dead_code)]
pub(crate) fn analysis_node_tile_span(capability_id: Option<&str>) -> (usize, usize) {
    match capability_id.unwrap_or_default() {
        "audio.separate_vocal_bgm"
        | "audio.extract_vocals"
        | "audio.extract_instrumental"
        | "audio.lead_isolate"
        | "analysis.asr"
        | "analysis.forced_alignment" => (2, 2),
        "audio.denoise"
        | "audio.dereverb"
        | "audio.refine"
        | "analysis.pitch_f0"
        | "analysis.note_boundary"
        | "analysis.technique"
        | "fusion.singing_evidence"
        | "fusion.candidate_graph" => (2, 1),
        _ => (1, 1),
    }
}

/// Real content-derived card width scale (§5). Height never varies -- no
/// node is ever laid out two rows tall -- so a node with more than one
/// configured model, or an unusually long label, gets extra width instead
/// of being squeezed into the same box as a short single-model node.
/// Execution state and selection never participate: this takes only the
/// stable label/model-id content every node already carries, so the DAG
/// never reflows while a run progresses or a card gets selected.
/// Uniform on purpose (§5 "普通节点使用一致的基础尺寸"): the layered layout
/// centers every same-rank sibling inside a shared column width, so even a
/// modest width difference between them reads as a misaligned column.
/// The uniform base card is large enough for wrapped titles and the complete
/// model list; `spawn_workflow_graph_node` does not summarize that list.
pub(crate) fn analysis_node_width_scale(_label: &str, _model_ids: &[String]) -> f32 {
    1.0
}

pub(crate) fn clamp_analysis_graph_zoom(zoom: f32) -> f32 {
    zoom.clamp(ANALYSIS_GRAPH_ZOOM_MIN, ANALYSIS_GRAPH_ZOOM_MAX)
}

/// Zoom that puts the unscaled canvas inside `viewport` with a small inset.
pub(crate) fn analysis_graph_fit_zoom(
    canvas_width: f32,
    canvas_height: f32,
    viewport_width: f32,
    viewport_height: f32,
) -> f32 {
    if canvas_width <= 1.0
        || canvas_height <= 1.0
        || viewport_width <= 1.0
        || viewport_height <= 1.0
    {
        return ANALYSIS_GRAPH_ZOOM_DEFAULT;
    }
    let width = (viewport_width - ANALYSIS_GRAPH_FIT_PADDING).max(1.0);
    let height = (viewport_height - ANALYSIS_GRAPH_FIT_PADDING).max(1.0);
    clamp_analysis_graph_zoom((width / canvas_width).min(height / canvas_height))
}

/// Scales a computed layout rect into screen-space box coordinates for the
/// current zoom level. Zoom is applied here, to the actual layout numbers
/// fed into each node/edge `Node`'s `left/top/width/height`, rather than as
/// a visual-only transform on the canvas wrapper -- that keeps the
/// scrollable content size (and therefore panning range and click
/// hit-testing) consistent with what's drawn at any zoom level, instead of
/// drifting out of sync with it.
pub(crate) fn zoomed_box(rect: LayoutRect, zoom: f32) -> AnalysisGraphBox {
    AnalysisGraphBox::new(
        rect.x * zoom,
        rect.y * zoom,
        rect.width * zoom,
        rect.height * zoom,
    )
}

/// Scroll offset and exact workflow node id for a Focus action.
#[cfg(test)]
pub(crate) fn analysis_graph_focus_target(
    layout: Option<&GraphLayout>,
    id: &app_core::AnalysisNodeId,
    zoom: f32,
) -> Option<(i32, String)> {
    analysis_graph_center_target(layout, id, zoom, 960.0)
}

/// Scroll offset that centers an exact workflow node in the viewport.
#[cfg(test)]
pub(crate) fn analysis_graph_center_target(
    layout: Option<&GraphLayout>,
    id: &app_core::AnalysisNodeId,
    zoom: f32,
    viewport_width: f32,
) -> Option<(i32, String)> {
    let rect = layout?.rect(id)?;
    let node_center = (rect.x + rect.width / 2.0) * zoom;
    let scroll = if viewport_width > 1.0 {
        (node_center - viewport_width / 2.0).max(0.0)
    } else {
        (rect.x * zoom - 60.0).max(0.0)
    };
    Some((scroll.round() as i32, id.to_string()))
}

/// Computes a camera offset from exact Workflow topology when a render-frame
/// layout is not available yet. It never estimates rank from a node id.
pub(crate) fn estimated_analysis_graph_center_target(
    workflow: Option<&app_core::WorkflowExecutionWireV1>,
    node_id: &str,
    zoom: f32,
    viewport_size: Vec2,
) -> Vec2 {
    let Some(workflow) = workflow else {
        return Vec2::ZERO;
    };
    let render = build_workflow_render_graph(workflow, None, None, false);
    let specs = render
        .nodes
        .iter()
        .enumerate()
        .map(|(order, node)| {
            LayoutNodeSpec::from_text(
                node.id.clone(),
                LayoutNodeVisualKind::Compute,
                &node.label,
                &node.detail,
                order,
            )
        })
        .collect::<Vec<_>>();
    let edges = render.edge_pairs();
    let Some(layout) = cached_canvas_routed_layout_with_specs(&specs, &edges) else {
        return Vec2::ZERO;
    };
    let Some(rect) = layout.layout.rect(&app_core::AnalysisNodeId::new(node_id)) else {
        return Vec2::ZERO;
    };
    let width = if viewport_size.x > 1.0 {
        viewport_size.x
    } else {
        960.0
    };
    let height = if viewport_size.y > 1.0 {
        viewport_size.y
    } else {
        720.0
    };
    Vec2::new(
        ((rect.x + rect.width / 2.0) * zoom - width / 2.0).max(0.0),
        ((rect.y + rect.height / 2.0) * zoom - height / 2.0).max(0.0),
    )
}

#[derive(Clone, Copy)]
pub(crate) enum WorkflowNodeVisualState {
    Waiting,
    Running(Option<usize>),
    Complete,
    Cancelled,
    Disabled,
    Failed,
    Deferred,
    ProfileSkipped,
    NotRequested,
}

pub(crate) fn spawn_activity_center(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let task_purpose_cards = session
        .analysis_tasks
        .iter()
        .map(activity_purpose_cards)
        .collect::<Vec<_>>();
    let purpose_count = task_purpose_cards
        .iter()
        .map(|cards| cards.len().max(1))
        .sum::<usize>();
    parent.spawn((
        Button,
        UiAction::from(AppCommand::CloseActivity),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.54)),
        ZIndex(100),
    ));
    parent
        .spawn((
            ActivityPanelScroll,
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(420),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20)),
                row_gap: px(12),
                overflow: Overflow::scroll_y(),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.9)),
            ZIndex(101),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(9),
                    ..default()
                })
                .with_children(|header| {
                    spawn_icon(header, icons, UiIcon::Queue, 17.0, theme.primary);
                    spawn_text(header, font.clone(), "Activity", 18.0, theme.foreground);
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "Close",
                        10.0,
                        UiAction::from(AppCommand::CloseActivity),
                    );
                });
            spawn_wrapped_text(
                panel,
                font.clone(),
                "Live analysis work and the most recent native operation.",
                10.0,
                theme.muted_foreground,
            );
            panel.spawn((
                Node {
                    width: percent(100),
                    height: px(1),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.64)),
            ));
            spawn_text(
                panel,
                font.clone(),
                format!(
                    "RUNS  ·  {}    PURPOSES  ·  {}",
                    session.analysis_tasks.len(),
                    purpose_count
                ),
                9.0,
                theme.muted_foreground,
            );
            if session.analysis_tasks.is_empty() {
                panel
                    .spawn((
                        Node {
                            width: percent(100),
                            padding: UiRect::all(px(18)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.55)),
                    ))
                    .with_children(|empty| {
                        spawn_wrapped_text(
                            empty,
                            font.clone(),
                            "Nothing is running. Requested analyses and failures appear here.",
                            10.0,
                            theme.muted_foreground,
                        );
                    });
            } else {
                for (task, purpose_cards) in session
                    .analysis_tasks
                    .iter()
                    .zip(&task_purpose_cards)
                    .take(10)
                {
                    let (status, progress, failed) = analysis_status_copy(task);
                    panel
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(11)),
                                row_gap: px(4),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.36)),
                            BorderColor::all(if failed {
                                theme.destructive.with_alpha(0.62)
                            } else {
                                theme.border.with_alpha(0.58)
                            }),
                        ))
                        .with_children(|card| {
                            card.spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                column_gap: px(8),
                                ..default()
                            })
                            .with_children(|row| {
                                row.spawn(Node {
                                    min_width: px(0),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.title.clone(),
                                        11.0,
                                        theme.foreground,
                                    );
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        task.artist.clone(),
                                        9.0,
                                        theme.muted_foreground,
                                    );
                                });
                                spawn_text(
                                    row,
                                    font.clone(),
                                    status,
                                    9.0,
                                    if failed {
                                        theme.destructive
                                    } else {
                                        theme.primary
                                    },
                                );
                                // Staged requests wait for an explicit start.
                                // They and worker-queued requests may still be
                                // cancelled before execution begins.
                                if matches!(task.status, app_core::QueuedStatus::Staged) {
                                    spawn_text_button(
                                        row,
                                        font.clone(),
                                        theme,
                                        "Start",
                                        9.0,
                                        UiAction::from(AnalysisCommand::StartQueuedAnalysis(
                                            task.file_hash.clone(),
                                        )),
                                    );
                                }
                                if matches!(
                                    task.status,
                                    app_core::QueuedStatus::Staged | app_core::QueuedStatus::Queued
                                ) {
                                    spawn_text_button(
                                        row,
                                        font.clone(),
                                        theme,
                                        "Cancel",
                                        9.0,
                                        UiAction::from(AnalysisCommand::CancelAnalysisRun(
                                            task.file_hash.clone(),
                                        )),
                                    );
                                }
                            });
                            if let app_core::QueuedStatus::Failed(message) = &task.status {
                                if !message.trim().is_empty() {
                                    spawn_bounded_wrapped_text(
                                        card,
                                        font.clone(),
                                        message,
                                        8.0,
                                        theme.destructive,
                                    );
                                }
                            }
                            if !purpose_cards.is_empty() {
                                for purpose in purpose_cards {
                                    spawn_activity_purpose_card(card, font.clone(), purpose, theme);
                                }
                            } else if !failed && let Some(live) = task.live.as_ref() {
                                let reported = live
                                    .node_id
                                    .as_deref()
                                    .and_then(|node_id| {
                                        find_matching_route(&live.stage_routes, node_id)
                                    })
                                    .and_then(super::nodes::worker_reported_progress);
                                spawn_text(
                                    card,
                                    font.clone(),
                                    reported
                                        .map(|percent| format!("{} · {percent}%", live.operation))
                                        .unwrap_or_else(|| format!("{} · Running", live.operation)),
                                    9.0,
                                    theme.primary,
                                );
                                spawn_wrapped_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}", live.implementation, live.detail),
                                    8.0,
                                    theme.muted_foreground,
                                );
                            }
                            if let Some(progress) = progress {
                                card.spawn((
                                    Node {
                                        position_type: PositionType::Relative,
                                        width: percent(100),
                                        height: px(3),
                                        margin: UiRect::top(px(4)),
                                        overflow: Overflow::clip(),
                                        border_radius: BorderRadius::MAX,
                                        ..default()
                                    },
                                    BackgroundColor(theme.muted),
                                ))
                                .with_children(|rail| {
                                    rail.spawn((
                                        Node {
                                            width: percent(progress.clamp(0, 100) as f32),
                                            height: percent(100),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(theme.primary),
                                    ));
                                });
                            }
                        });
                }
            }
            spawn_activity_history(panel, font.clone(), session, theme);
            panel.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if let Some(notice) = session.notice.as_deref() {
                spawn_wrapped_text(panel, font.clone(), notice, 9.0, theme.muted_foreground);
            }
            spawn_action_button(
                panel,
                font,
                theme,
                "Open analysis queue",
                UiAction::from(LibraryCommand::SetLibraryView(LibraryView::Queue)),
            );
        });
}

struct ActivityPurposeCard {
    label: String,
    models: Vec<String>,
    state: GraphNodeState,
    progress: Option<usize>,
    runtime: String,
}

fn activity_purpose_cards(task: &app_core::AnalysisTask) -> Vec<ActivityPurposeCard> {
    let Some(engine) = task.live.as_ref().and_then(|live| live.engine.as_ref()) else {
        return Vec::new();
    };
    let Some((workflow, exact_plan)) = exact_workflow_plan_from_engine(engine) else {
        return Vec::new();
    };
    let exact_capabilities = exact_engine_capabilities_from_engine(engine);
    let mut graph = build_workflow_render_graph(
        &workflow,
        exact_plan.as_ref(),
        exact_capabilities.as_ref(),
        false,
    );
    overlay_workflow_runtime(&mut graph, task);
    graph
        .nodes
        .iter()
        .map(|node| {
            let summary =
                analysis_graph_route_summary(task, node, node.state == GraphNodeState::Complete);
            let progress = match node.state {
                GraphNodeState::Complete => Some(100),
                GraphNodeState::Running | GraphNodeState::Waiting | GraphNodeState::Deferred => {
                    analysis_graph_node_progress(task, node)
                }
                _ => None,
            };
            ActivityPurposeCard {
                label: node.label.clone(),
                models: summary
                    .model_ids
                    .iter()
                    .map(|model| app_core::workflow_model_label(model).to_string())
                    .collect(),
                state: node.state,
                progress,
                runtime: summary.runtime,
            }
        })
        .collect()
}

fn spawn_activity_purpose_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    purpose: &ActivityPurposeCard,
    theme: &StudioTheme,
) {
    let (status, color) = match purpose.state {
        GraphNodeState::Complete => ("COMPLETE".to_string(), theme.primary),
        GraphNodeState::Running => (
            purpose.progress.map_or_else(
                || "RUNNING".to_string(),
                |progress| format!("RUNNING · {progress}%"),
            ),
            theme.primary,
        ),
        GraphNodeState::Failed => ("FAILED".to_string(), theme.destructive),
        GraphNodeState::Cancelled => ("CANCELLED".to_string(), theme.destructive),
        GraphNodeState::Deferred => ("DEFERRED".to_string(), theme.editor_warning),
        GraphNodeState::ProfileSkipped => ("SKIPPED".to_string(), theme.muted_foreground),
        GraphNodeState::NotRequested => ("NOT REQUESTED".to_string(), theme.muted_foreground),
        GraphNodeState::Disabled => ("DISABLED".to_string(), theme.muted_foreground),
        GraphNodeState::Waiting => ("WAITING".to_string(), theme.muted_foreground),
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Column,
                row_gap: px(3),
                padding: UiRect::all(px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(4)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.32)),
            BorderColor::all(if purpose.state == GraphNodeState::Failed {
                theme.destructive.with_alpha(0.5)
            } else {
                theme.border.with_alpha(0.42)
            }),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: percent(100),
                min_width: px(0),
                align_items: AlignItems::Center,
                column_gap: px(6),
                ..default()
            })
            .with_children(|row| {
                row.spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    ..default()
                })
                .with_children(|label| {
                    spawn_bounded_wrapped_text(
                        label,
                        font.clone(),
                        &purpose.label,
                        8.5,
                        theme.foreground,
                    );
                });
                spawn_text(row, font.clone(), status, 7.5, color);
            });
            if !purpose.models.is_empty() {
                spawn_bounded_wrapped_text(
                    card,
                    font.clone(),
                    purpose.models.join(" · "),
                    7.5,
                    color.with_alpha(0.88),
                );
            }
            spawn_bounded_wrapped_text(
                card,
                font.clone(),
                &purpose.runtime,
                7.0,
                theme.muted_foreground,
            );
            if let Some(progress) = purpose.progress {
                card.spawn((
                    Node {
                        width: percent(100),
                        height: px(2),
                        margin: UiRect::top(px(2)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.muted),
                ))
                .with_children(|rail| {
                    rail.spawn((
                        Node {
                            width: percent(progress.clamp(0, 100) as f32),
                            height: percent(100),
                            ..default()
                        },
                        BackgroundColor(color),
                    ));
                });
            }
        });
}

fn spawn_activity_history(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            margin: UiRect::top(px(4)),
            ..default()
        },
        BackgroundColor(theme.border.with_alpha(0.64)),
    ));
    parent
        .spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(8),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(5),
            ..default()
        })
        .with_children(|heading| {
            spawn_text(
                heading,
                font.clone(),
                format!("RECENT RUNS  ·  {}", session.analysis_history.len()),
                9.0,
                theme.muted_foreground,
            );
            heading.spawn(Node {
                flex_grow: 1.0,
                ..default()
            });
            if session.selected_analysis_history.is_some() {
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
                    "Delete all",
                    8.0,
                    UiAction::from(AnalysisCommand::ConfirmClearAnalysisHistory),
                );
            } else if !session.analysis_history.is_empty() {
                spawn_text_button(
                    heading,
                    font.clone(),
                    theme,
                    "Clear…",
                    8.0,
                    UiAction::from(AnalysisCommand::RequestClearAnalysisHistory),
                );
            }
        });
    if session.pending_analysis_history_clear {
        spawn_wrapped_text(
            parent,
            font.clone(),
            "Delete all saved analysis runs? This does not delete charts or source media.",
            9.0,
            theme.destructive,
        );
    }
    if session.analysis_history.is_empty() {
        parent
            .spawn((
                Node {
                    width: percent(100),
                    padding: UiRect::all(px(14)),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.34)),
                BorderColor::all(theme.border.with_alpha(0.55)),
            ))
            .with_children(|empty| {
                spawn_wrapped_text(
                    empty,
                    font,
                    "No previous analysis runs.",
                    10.0,
                    theme.muted_foreground,
                );
            });
        return;
    }
    for item in session.analysis_history.iter().take(20) {
        let selected = session.selected_analysis_history == Some(item.id);
        let progress = if item.status == "completed" {
            100
        } else {
            item.snapshot.overall_progress.clamp(0, 100)
        };
        let failed = item.status == "failed";
        parent
            .spawn((
                Button,
                UiAction::from(AnalysisCommand::SelectAnalysisHistory(Some(item.id))),
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(11)),
                    row_gap: px(4),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(5)),
                    ..default()
                },
                BackgroundColor(if selected {
                    theme.primary.with_alpha(0.10)
                } else {
                    theme.background.with_alpha(0.36)
                }),
                BorderColor::all(if failed {
                    theme.destructive.with_alpha(0.62)
                } else if selected {
                    theme.primary.with_alpha(0.62)
                } else {
                    theme.border.with_alpha(0.58)
                }),
            ))
            .with_children(|card| {
                card.spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(8),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(
                            copy,
                            font.clone(),
                            item.title.clone(),
                            11.0,
                            theme.foreground,
                        );
                        spawn_text(
                            copy,
                            font.clone(),
                            item.artist.clone(),
                            9.0,
                            theme.muted_foreground,
                        );
                    });
                    spawn_text(
                        row,
                        font.clone(),
                        format!("{} · {progress}%", item.status.to_ascii_uppercase()),
                        8.0,
                        if failed {
                            theme.destructive
                        } else if selected {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        },
                    );
                });
            });
    }
}

pub(crate) fn analysis_status_copy(task: &app_core::AnalysisTask) -> (String, Option<usize>, bool) {
    match &task.status {
        app_core::QueuedStatus::Staged => ("Waiting to start".to_string(), None, false),
        app_core::QueuedStatus::Queued => ("Queued".to_string(), None, false),
        app_core::QueuedStatus::Analyzing(_)
            if task.live.as_ref().is_some_and(|live| live.engine.is_some()) =>
        {
            let progress = task
                .live
                .as_ref()
                .map_or(0, |live| live.overall_progress.clamp(0, 100));
            (format!("Analyzing · {progress}%"), Some(progress), false)
        }
        app_core::QueuedStatus::Analyzing(progress) => {
            (format!("Analyzing · {progress}%"), Some(*progress), false)
        }
        app_core::QueuedStatus::Failed(_) => ("Failed".to_string(), None, true),
    }
}

/// Minimal, dependency-free ms-since-epoch -> `"YYYY-MM-DD HH:MM"` (UTC)
/// formatter for artifact/history timestamps in the inspector -- good
/// enough for display without pulling in a full date/time crate for one
/// field. Proleptic Gregorian civil-date conversion via Howard Hinnant's
/// well-known days-from-epoch algorithm.
pub(crate) fn format_epoch_ms(ms: i64) -> String {
    let total_seconds = ms.div_euclid(1000);
    let days = total_seconds.div_euclid(86_400);
    let secs_of_day = total_seconds.rem_euclid(86_400);
    let hour = secs_of_day / 3600;
    let minute = (secs_of_day % 3600) / 60;

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02} {hour:02}:{minute:02} UTC")
}

pub(crate) fn format_node_attempt_comparison(
    comparison: &app_core::NodeAttemptComparison,
) -> String {
    let (current, previous) = match (&comparison.attempt_a, &comparison.attempt_b) {
        (Some(current), Some(previous)) => (current, previous),
        (Some(_), None) => {
            return format!(
                "{} has no recorded attempt in the previous run.",
                comparison.node_id
            );
        }
        (None, Some(_)) => {
            return format!(
                "{} has no recorded attempt in the current run.",
                comparison.node_id
            );
        }
        (None, None) => {
            return format!(
                "{} has no recorded attempt in either run.",
                comparison.node_id
            );
        }
    };
    if comparison.changed_fields.is_empty() {
        return format!(
            "{} is unchanged from the previous attempt ({}).",
            comparison.node_id, current.implementation
        );
    }
    let field_value = |field: &str, attempt: &app_core::NodeAttempt| -> String {
        match field {
            "status" => attempt.status.clone(),
            "implementation" => attempt.implementation.clone(),
            "model" => attempt.model.clone(),
            "requested_device" => attempt.requested_device.clone(),
            "actual_device" => attempt.actual_device.clone(),
            "fallback_from" => attempt.fallback_from.clone().unwrap_or_default(),
            "backend_fallback_from" => attempt.backend_fallback_from.clone().unwrap_or_default(),
            _ => String::new(),
        }
    };
    let changes: Vec<String> = comparison
        .changed_fields
        .iter()
        .map(|field| {
            format!(
                "{field}: {} → {}",
                field_value(field, previous),
                field_value(field, current)
            )
        })
        .collect();
    format!(
        "{} changed since the previous attempt — {}",
        comparison.node_id,
        changes.join(", ")
    )
}

/// §7.4 "DURATION" inspector fact -- Phase 7's "Duration 检查器字段" gap
/// closed by real per-node `started_at_ms`/`finished_at_ms`
/// (native worker progress frames), not something inferred from transport
/// receive time. `None`/incomplete data (still running, predates this
/// field, or a corrupt `finished < started`) reads as "Not yet available"
/// rather than a wrong or negative duration.
pub(crate) fn node_duration_copy(route: Option<&app_core::AnalysisStageRoute>) -> String {
    match route.and_then(|r| r.started_at_ms.zip(r.finished_at_ms)) {
        Some((started, finished)) if finished >= started => {
            format_duration((finished - started) as f64 / 1000.0)
        }
        _ => "Not yet available".to_string(),
    }
}

/// §8 detailed Inspect view "WORKER TASK" fact -- the one place the full
/// `worker_task_id` is shown (never on the default compact card; see
/// `measured_work_unit_progress`, which deliberately excludes it).
pub(crate) fn selected_worker_task_text(route: Option<&app_core::AnalysisStageRoute>) -> String {
    route
        .and_then(|route| route.worker_task_id.as_deref())
        .filter(|task_id| !task_id.trim().is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "Not available".to_string())
}

/// Maps exact compiled-workflow state onto the compact node widget.
pub(crate) fn graph_node_visual_state(
    state: GraphNodeState,
    running_progress: Option<usize>,
) -> (WorkflowNodeVisualState, Option<&'static str>) {
    match state {
        GraphNodeState::Running => (WorkflowNodeVisualState::Running(running_progress), None),
        GraphNodeState::Complete => (WorkflowNodeVisualState::Complete, None),
        GraphNodeState::Cancelled => (WorkflowNodeVisualState::Cancelled, Some("Stopped by user")),
        GraphNodeState::Waiting => (WorkflowNodeVisualState::Waiting, None),
        GraphNodeState::Disabled => (
            WorkflowNodeVisualState::Disabled,
            Some("Disabled in Processing Studio"),
        ),
        GraphNodeState::Failed => (
            WorkflowNodeVisualState::Failed,
            Some("Failed · inspect details"),
        ),
        GraphNodeState::Deferred => (
            WorkflowNodeVisualState::Deferred,
            Some("Deferred · conditional expert"),
        ),
        GraphNodeState::ProfileSkipped => (
            WorkflowNodeVisualState::ProfileSkipped,
            Some("Skipped by quality profile"),
        ),
        GraphNodeState::NotRequested => (
            WorkflowNodeVisualState::NotRequested,
            Some("Not requested by this exact execution plan"),
        ),
    }
}

pub(crate) fn graph_node_panel_status(
    state: Option<GraphNodeState>,
    fallback: &'static str,
) -> &'static str {
    match state {
        Some(GraphNodeState::Complete) => "COMPLETE",
        Some(GraphNodeState::Cancelled) => "CANCELLED",
        Some(GraphNodeState::Running) => "RUNNING",
        Some(GraphNodeState::Failed) => "FAILED",
        Some(GraphNodeState::Disabled) => "DISABLED",
        Some(GraphNodeState::ProfileSkipped) => "OFF",
        Some(GraphNodeState::NotRequested) => "NOT REQUESTED",
        Some(GraphNodeState::Deferred) => "DEFERRED",
        Some(GraphNodeState::Waiting) => "WAITING",
        None => fallback,
    }
}

/// Returns the exact runtime event for one compiled workflow node.
pub(crate) fn find_matching_route<'a>(
    routes: &'a [app_core::AnalysisStageRoute],
    node_id: &str,
) -> Option<&'a app_core::AnalysisStageRoute> {
    routes
        .iter()
        .rev()
        .find(|route| route.node_id.as_deref() == Some(node_id))
}

/// Keeps the canvas, inspector, and quick panel on one status source. The
/// route still supplies granular progress for Running/Failed, while every
/// terminal, blocked, disabled, frozen, bypassed, or waiting label comes
/// from the canvas's plan-plus-event `GraphNodeState`.
pub(crate) fn selected_progress_and_status(
    render_state: Option<GraphNodeState>,
    route_progress: usize,
    route_status: &'static str,
) -> (usize, &'static str) {
    match render_state {
        Some(GraphNodeState::Complete) => {
            (100, graph_node_panel_status(render_state, route_status))
        }
        Some(GraphNodeState::Running | GraphNodeState::Failed | GraphNodeState::Cancelled) => (
            route_progress,
            graph_node_panel_status(render_state, route_status),
        ),
        Some(_) => (0, graph_node_panel_status(render_state, route_status)),
        None => (route_progress, route_status),
    }
}
