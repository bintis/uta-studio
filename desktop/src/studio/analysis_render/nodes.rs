use super::*;
use crate::studio::*;

pub(crate) fn analysis_graph_category_accent(
    category: GraphNodeCategory,
    theme: &StudioTheme,
) -> Color {
    match (theme.dark, category) {
        (true, GraphNodeCategory::Source) => Color::srgb(0.45, 0.62, 0.82),
        (true, GraphNodeCategory::Audio) => Color::srgb(0.38, 0.70, 0.67),
        (true, GraphNodeCategory::Lyrics) => Color::srgb(0.78, 0.64, 0.40),
        (true, GraphNodeCategory::Pitch) => Color::srgb(0.67, 0.54, 0.78),
        (true, GraphNodeCategory::Evidence) => Color::srgb(0.59, 0.57, 0.76),
        (true, GraphNodeCategory::Fusion) => Color::srgb(0.50, 0.69, 0.49),
        (true, GraphNodeCategory::Output) => Color::srgb(0.42, 0.67, 0.77),
        (false, GraphNodeCategory::Source) => Color::srgb(0.22, 0.43, 0.66),
        (false, GraphNodeCategory::Audio) => Color::srgb(0.12, 0.49, 0.47),
        (false, GraphNodeCategory::Lyrics) => Color::srgb(0.63, 0.45, 0.16),
        (false, GraphNodeCategory::Pitch) => Color::srgb(0.48, 0.32, 0.61),
        (false, GraphNodeCategory::Evidence) => Color::srgb(0.39, 0.37, 0.61),
        (false, GraphNodeCategory::Fusion) => Color::srgb(0.28, 0.51, 0.27),
        (false, GraphNodeCategory::Output) => Color::srgb(0.16, 0.47, 0.59),
    }
}

/// Keep controls readable while zooming out, but scale them together with
/// their node while zooming in. Capping at the 100% size made enlarged nodes
/// look empty and left their status/model rows behind at stale coordinates.
pub(crate) fn analysis_graph_scaled(base: f32, minimum: f32, zoom: f32) -> f32 {
    (base * zoom).max(minimum)
}

fn spawn_analysis_graph_status_pill_at(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: String,
    color: Color,
    zoom: f32,
    bottom: f32,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                bottom: px(analysis_graph_scaled(bottom, 4.5, zoom)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(
                        px(analysis_graph_scaled(6.0, 4.0, zoom)),
                        px(analysis_graph_scaled(2.5, 1.5, zoom)),
                    ),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(color.with_alpha(0.16)),
            ))
            .with_children(|pill| {
                spawn_text(
                    pill,
                    font,
                    text,
                    analysis_graph_scaled(7.5, 6.4, zoom),
                    color,
                );
            });
        });
}

fn spawn_analysis_graph_runtime_line(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    route: &str,
    accent: Color,
    zoom: f32,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(analysis_graph_scaled(6.0, 4.0, zoom)),
                right: px(analysis_graph_scaled(6.0, 4.0, zoom)),
                bottom: px(analysis_graph_scaled(5.0, 3.0, zoom)),
                height: px(analysis_graph_scaled(18.0, 14.0, zoom)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(analysis_graph_scaled(3.0, 2.0, zoom))),
                overflow: Overflow::clip(),
                border: UiRect::top(px(analysis_graph_scaled(1.0, 0.7, zoom))),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.10)),
            BorderColor::all(accent.with_alpha(0.22)),
        ))
        .with_children(|tag| {
            tag.spawn((
                Text::new(route),
                ui_text_font(font, analysis_graph_scaled(7.2, 6.2, zoom)),
                TextColor(theme.muted_foreground),
                TextLayout::no_wrap(),
            ));
        });
}

/// The returned label deliberately excludes the raw `worker_task_id` (§8):
/// a full `studio-xxxxxxxxxxxxxxxx` id does not belong on the default
/// card, only in the detailed Inspect view (`WORKER TASK` fact row). The
/// id is still required to gate a genuine, worker-attributed measurement.
pub(crate) fn measured_work_unit_progress(
    route: &app_core::AnalysisStageRoute,
) -> Option<(usize, String)> {
    let (completed, total) = route.work_units_completed.zip(route.work_units_total)?;
    let task_id = route.worker_task_id.as_deref()?.trim();
    if total == 0 || completed > total || task_id.is_empty() {
        return None;
    }
    let percent = ((completed.saturating_mul(100) / total).min(100)) as usize;
    Some((percent, format!("{completed}/{total} work units")))
}

/// Prefer exact completed/total units while still honoring a native
/// worker's validated fractional phase updates. Preprocessing and compile
/// phases can report a real fraction before an iterable work-unit count
/// exists; those frames must not appear as "progress unavailable".
pub(crate) fn worker_reported_progress(route: &app_core::AnalysisStageRoute) -> Option<usize> {
    measured_work_unit_progress(route)
        .map(|(percent, _)| percent)
        .or_else(|| {
            (route.node_event.as_deref() == Some("node_progress")
                && route
                    .worker_task_id
                    .as_deref()
                    .is_some_and(|task_id| !task_id.trim().is_empty()))
            .then_some(route.stage_progress.clamp(0, 100))
        })
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    node: &RenderNode,
    completed: bool,
) -> AnalysisGraphRouteSummary {
    let routes = task
        .live
        .as_ref()
        .map(|live| {
            node.members
                .iter()
                .filter_map(|member| find_matching_route(&live.stage_routes, member.id.as_str()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if routes.is_empty() {
        return AnalysisGraphRouteSummary {
            model_ids: node.model_ids.clone(),
            runtime: if completed {
                "Complete · no runtime trace".to_string()
            } else {
                "Awaiting connected inputs".to_string()
            },
            warning: false,
        };
    }
    let warning = routes
        .iter()
        .any(|route| route.fallback_from.is_some() || route.backend_fallback_from.is_some());
    if node.members.len() > 1 {
        let active = node
            .members
            .iter()
            .filter(|member| {
                !matches!(
                    member.state,
                    GraphNodeState::Disabled
                        | GraphNodeState::ProfileSkipped
                        | GraphNodeState::NotRequested
                )
            })
            .count();
        let completed = node
            .members
            .iter()
            .filter(|member| member.state == GraphNodeState::Complete)
            .count();
        let mut implementations = routes
            .iter()
            .map(|route| route.implementation.clone())
            .filter(|implementation| !implementation.trim().is_empty())
            .collect::<Vec<_>>();
        implementations.sort();
        implementations.dedup();
        let runtime = if implementations.is_empty() {
            format!("{completed}/{active} models complete")
        } else {
            format!(
                "{completed}/{active} models complete · {}",
                implementations.join(" + ")
            )
        };
        return AnalysisGraphRouteSummary {
            model_ids: node.model_ids.clone(),
            runtime,
            warning,
        };
    }
    let route = routes[0];
    let implementation = route
        .backend_fallback_from
        .as_ref()
        .map(|from| {
            format!(
                "{} > {}",
                from.to_ascii_uppercase(),
                route.implementation.to_ascii_uppercase()
            )
        })
        .unwrap_or_else(|| route.implementation.clone());
    let measured = measured_work_unit_progress(route)
        .map(|(_, units)| format!(" · {units}"))
        .unwrap_or_default();
    AnalysisGraphRouteSummary {
        model_ids: {
            let mut models = node.model_ids.clone();
            for model in analysis_graph_model_ids(&route.model) {
                if !models.contains(&model) {
                    models.push(model);
                }
            }
            models
        },
        runtime: format!("{implementation}{measured}"),
        warning,
    }
}

/// Equal-weight progress across the concrete model executions represented by
/// one semantic purpose card. Completed models contribute 100%; configured
/// models that have not started contribute 0%; an active native worker uses
/// its measured work units/fraction. Profile-skipped and unrequested models
/// are excluded because they are not part of this exact run.
pub(crate) fn analysis_graph_node_progress(
    task: &app_core::AnalysisTask,
    node: &RenderNode,
) -> Option<usize> {
    let live = task.live.as_ref()?;
    let active = node
        .members
        .iter()
        .filter(|member| {
            !matches!(
                member.state,
                GraphNodeState::Disabled
                    | GraphNodeState::ProfileSkipped
                    | GraphNodeState::NotRequested
            )
        })
        .collect::<Vec<_>>();
    if active.is_empty() {
        return None;
    }
    let completed = active
        .iter()
        .map(|member| {
            if member.state == GraphNodeState::Complete {
                return 100;
            }
            find_matching_route(&live.stage_routes, member.id.as_str())
                .and_then(worker_reported_progress)
                .unwrap_or(0)
        })
        .sum::<usize>();
    Some((completed / active.len()).min(100))
}

pub(crate) struct AnalysisGraphRouteSummary {
    pub(crate) model_ids: Vec<String>,
    pub(crate) runtime: String,
    pub(crate) warning: bool,
}

pub(crate) fn analysis_graph_model_ids(raw: &str) -> Vec<String> {
    let mut models = Vec::new();
    for model in raw.lines().flat_map(|line| line.split(" + ")) {
        let model = model.trim();
        if model.is_empty() || model.eq_ignore_ascii_case("default") {
            continue;
        }
        if !models.iter().any(|existing| existing == model) {
            models.push(model.to_string());
        }
    }
    models
}

pub(crate) fn analysis_graph_model_labels(model_ids: &[String]) -> impl Iterator<Item = &str> {
    model_ids
        .iter()
        .map(|model_id| app_core::workflow_model_label(model_id))
}

pub(crate) struct WorkflowNodeCardSpec<'a> {
    pub(crate) bounds: AnalysisGraphBox,
    pub(crate) capability_id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) label: &'a str,
    pub(crate) model_ids: &'a [String],
    pub(crate) state: WorkflowNodeVisualState,
    pub(crate) selected: bool,
    pub(crate) route: &'a str,
    pub(crate) warning: bool,
    pub(crate) dimmed: bool,
    pub(crate) zoom: f32,
    pub(crate) input_ports: usize,
    pub(crate) output_ports: usize,
    pub(crate) category: GraphNodeCategory,
}

pub(crate) fn spawn_workflow_graph_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    spec: WorkflowNodeCardSpec,
) {
    let WorkflowNodeCardSpec {
        bounds,
        capability_id,
        node_id,
        file_hash,
        label,
        model_ids,
        state,
        selected,
        route,
        warning,
        dimmed,
        input_ports,
        output_ports,
        zoom,
        category,
    } = spec;
    let accent = analysis_graph_category_accent(category, theme);
    let padding = analysis_graph_scaled(8.0, 5.5, zoom);
    let gap = analysis_graph_scaled(4.0, 2.5, zoom);
    let title_size = analysis_graph_scaled(10.5, 8.0, zoom);
    let meta_size = analysis_graph_scaled(7.5, 6.4, zoom);
    let show_footer = zoom >= 0.60;
    let glyph = match category {
        GraphNodeCategory::Source => "↗",
        GraphNodeCategory::Audio => "~",
        GraphNodeCategory::Lyrics => "A",
        GraphNodeCategory::Pitch => "∿",
        GraphNodeCategory::Evidence => "◇",
        GraphNodeCategory::Fusion => "◎",
        GraphNodeCategory::Output => "+",
    };
    let complete_color = analysis_graph_category_accent(GraphNodeCategory::Output, theme);
    // Status color is fixed per state (§6) -- Running always reads as
    // `theme.primary` regardless of which category's accent this node's
    // top strip/glyph/model text uses, so the same status always looks the
    // same everywhere on the canvas.
    let (status, progress, status_color, status_glyph) = match state {
        WorkflowNodeVisualState::Waiting => ("WAITING", Some(0), theme.muted_foreground, "○"),
        WorkflowNodeVisualState::Running(progress) => ("RUNNING", progress, theme.primary, "●"),
        WorkflowNodeVisualState::Complete => ("COMPLETE", Some(100), complete_color, "✓"),
        WorkflowNodeVisualState::Cancelled => ("CANCELLED", Some(0), theme.destructive, "■"),
        WorkflowNodeVisualState::Disabled => ("DISABLED", Some(0), theme.muted_foreground, "⊘"),
        WorkflowNodeVisualState::Failed => ("FAILED", Some(0), theme.destructive, "✕"),
        WorkflowNodeVisualState::Deferred => ("DEFERRED", Some(0), theme.editor_warning, "…"),
        WorkflowNodeVisualState::ProfileSkipped => {
            ("SKIPPED", Some(0), theme.muted_foreground, "–")
        }
        WorkflowNodeVisualState::NotRequested => (
            "NOT REQUESTED",
            Some(0),
            theme.muted_foreground.with_alpha(0.6),
            "·",
        ),
    };
    let running = matches!(state, WorkflowNodeVisualState::Running(_));
    let complete = matches!(state, WorkflowNodeVisualState::Complete);
    let failed = matches!(
        state,
        WorkflowNodeVisualState::Failed | WorkflowNodeVisualState::Cancelled
    );
    let context_node_id = node_id.to_string();
    let context_capability_id = capability_id.to_string();
    let context_file_hash = file_hash.to_string();
    let context_label = label.to_string();
    // Translucent, not a solid opaque status tile (house style's "restrained
    // translucency" -- direct feedback that a fully opaque card against the
    // dark canvas read as an abrupt cutout rather than a card sitting on
    // the surface). A small status tint mixes into the same base
    // `theme.card` every other card surface in the app uses, then the
    // whole thing is rendered at a normal card alpha instead of 100%.
    let tile_background = match state {
        WorkflowNodeVisualState::Complete => complete_color.mix(&theme.card, 0.82).with_alpha(0.4),
        WorkflowNodeVisualState::Failed | WorkflowNodeVisualState::Cancelled => {
            theme.destructive.mix(&theme.card, 0.82).with_alpha(0.4)
        }
        WorkflowNodeVisualState::Deferred => {
            theme.editor_warning.mix(&theme.card, 0.85).with_alpha(0.36)
        }
        WorkflowNodeVisualState::Running(_) => theme.primary.mix(&theme.card, 0.8).with_alpha(0.42),
        WorkflowNodeVisualState::Waiting => theme.card.with_alpha(STUDIO_CARD_BACKGROUND_ALPHA),
        WorkflowNodeVisualState::Disabled
        | WorkflowNodeVisualState::ProfileSkipped
        | WorkflowNodeVisualState::NotRequested => theme.background.with_alpha(0.22),
    };
    parent
        .spawn((
            Button,
            UiPointerApi(&[
                "ui.pointer.analysis_node.primary",
                "ui.pointer.analysis_node.secondary",
            ]),
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                flex_direction: FlexDirection::Column,
                padding: UiRect {
                    left: px(padding),
                    right: px(padding),
                    top: px(padding),
                    bottom: px(if show_footer {
                        analysis_graph_scaled(52.0, 31.0, zoom)
                    } else {
                        analysis_graph_scaled(8.0, 4.5, zoom)
                    }),
                },
                row_gap: px(gap),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                // Rectangular, not the rounded card style used elsewhere --
                // direct user feedback on a DAG canvas full of small,
                // orthogonally-routed cards: a crisp rectangle reads as one
                // tidy grid, while rounded corners fought the straight
                // routed lines meeting them.
                border_radius: BorderRadius::all(px(2.0)),
                ..default()
            },
            BackgroundColor(if dimmed {
                tile_background.mix(&theme.background, 0.48)
            } else {
                tile_background
            }),
            BorderColor::all(if selected {
                accent.with_alpha(0.82)
            } else if running {
                status_color.with_alpha(if dimmed { 0.20 } else { 0.62 })
            } else if complete {
                complete_color.with_alpha(if dimmed { 0.14 } else { 0.34 })
            } else if failed {
                theme
                    .destructive
                    .with_alpha(if dimmed { 0.18 } else { 0.68 })
            } else {
                theme.border.with_alpha(if dimmed { 0.16 } else { 0.46 })
            }),
            // Running gets no shadow escalation (§6/§9 acceptance: clear,
            // never glowing) -- a fixed status color, a solid border, the
            // status pill, and the progress fill below already say
            // "running" without a soft-edged halo.
            BoxShadow::new(
                theme.foreground.with_alpha(if dimmed {
                    0.0
                } else if selected {
                    0.15
                } else {
                    0.02
                }),
                px(0),
                px(0),
                px(analysis_graph_scaled(8.0, 5.0, zoom)),
                px(0),
            ),
            ZIndex(if selected { 5 } else { 2 }),
        ))
        .with_children(|node| {
            if running && let Some(progress) = progress {
                node.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        bottom: px(0),
                        width: percent(progress.clamp(0, 100) as f32),
                        border_radius: BorderRadius::all(px(analysis_graph_scaled(5.0, 3.0, zoom))),
                        ..default()
                    },
                    BackgroundColor(status_color.with_alpha(if dimmed { 0.05 } else { 0.18 })),
                    Pickable::IGNORE,
                ));
            }
            node.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(analysis_graph_scaled(8.0, 5.0, zoom)),
                    right: px(analysis_graph_scaled(8.0, 5.0, zoom)),
                    top: px(0),
                    height: px(analysis_graph_scaled(2.0, 1.0, zoom)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent.with_alpha(if running { 0.72 } else { 0.26 })),
                Pickable::IGNORE,
            ));
            spawn_analysis_graph_ports(
                node,
                theme,
                complete || running,
                zoom,
                bounds.height,
                input_ports,
                output_ports,
            );
            if selected {
                node.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(analysis_graph_scaled(9.0, 5.0, zoom)),
                        bottom: px(analysis_graph_scaled(9.0, 5.0, zoom)),
                        width: px(analysis_graph_scaled(2.0, 1.0, zoom)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(accent),
                    Pickable::IGNORE,
                ));
            }
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(analysis_graph_scaled(7.0, 3.0, zoom)),
                ..default()
            })
            .with_children(|heading| {
                if zoom >= 0.60 {
                    heading
                        .spawn((
                            Node {
                                width: px(analysis_graph_scaled(20.0, 14.0, zoom)),
                                height: px(analysis_graph_scaled(20.0, 14.0, zoom)),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border_radius: BorderRadius::all(px(analysis_graph_scaled(
                                    6.0, 4.0, zoom,
                                ))),
                                ..default()
                            },
                            BackgroundColor(accent.with_alpha(if running || complete {
                                0.20
                            } else {
                                0.11
                            })),
                        ))
                        .with_children(|badge| {
                            spawn_text(badge, font.clone(), glyph, meta_size, accent);
                        });
                }
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_bounded_wrapped_text(
                            copy,
                            font.clone(),
                            label,
                            title_size,
                            theme.foreground,
                        );
                        // Every configured model remains named on the card.
                        // The enlarged fixed tile and reserved footer keep
                        // this list clear of the status/runtime overlays.
                        for model_label in analysis_graph_model_labels(model_ids) {
                            spawn_bounded_wrapped_text(
                                copy,
                                font.clone(),
                                model_label,
                                meta_size,
                                accent.with_alpha(0.86),
                            );
                        }
                    });
            });
            if show_footer {
                let copy = if running {
                    progress.map_or_else(
                        || format!("{status_glyph} {status} · measured progress unavailable"),
                        |value| format!("{status_glyph} {status} {value}%"),
                    )
                } else {
                    format!("{status_glyph} {status}")
                };
                spawn_analysis_graph_status_pill_at(
                    node,
                    font.clone(),
                    copy,
                    status_color,
                    zoom,
                    31.0,
                );
                spawn_analysis_graph_runtime_line(
                    node,
                    font,
                    theme,
                    route,
                    if warning {
                        theme.editor_warning
                    } else {
                        accent
                    },
                    zoom,
                );
            }
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut analysis: ResMut<AnalysisUiState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>,
                  windows: Query<&Window, With<PrimaryWindow>>| {
                event.propagate(false);
                let viewport_size = windows
                    .single()
                    .map(|window| Vec2::new(window.width(), window.height()))
                    .unwrap_or(Vec2::new(1280.0, 720.0));
                open_analysis_node_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    viewport_size,
                    AnalysisNodeClickTarget {
                        node_id: &context_node_id,
                        label: &context_label,
                        file_hash: &context_file_hash,
                        capability_id: &context_capability_id,
                    },
                    &mut analysis,
                    &mut dialogs,
                    &mut invalidated,
                );
            },
        );
}

/// The pointer position `open_analysis_node_from_pointer` needs, converted from
/// raw window pixels into `LibrarySongList`'s own local space -- the
/// analysis node context menu is spawned as a direct absolute-positioned
/// child of that same list (`spawn_analysis_node_context_menu`), so that is
/// the coordinate space its `left`/`top` need. Falls back to the raw window
/// position if the list isn't found (defensive only -- every caller of this
/// only runs from inside that list's own subtree).
pub(crate) fn analysis_graph_port_centers(
    node_height: f32,
    port_count: usize,
    zoom: f32,
) -> Vec<f32> {
    if port_count == 0 {
        return Vec::new();
    }
    if port_count == 1 {
        return vec![node_height * 0.5];
    }
    let inset = (node_height * 0.16).clamp(6.0 * zoom, 14.0 * zoom);
    let usable = (node_height - inset * 2.0).max(0.0);
    (0..port_count)
        .map(|index| inset + usable * index as f32 / (port_count as f32 - 1.0))
        .collect()
}

pub(crate) fn spawn_analysis_graph_ports(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    ready: bool,
    zoom: f32,
    node_height: f32,
    input_ports: usize,
    output_ports: usize,
) {
    let size = analysis_graph_scaled(10.0, 7.0, zoom);
    for (left_side, port_count) in [(true, input_ports), (false, output_ports)] {
        for center_y in analysis_graph_port_centers(node_height, port_count, zoom) {
            parent.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: if left_side {
                        px(-size * 0.5)
                    } else {
                        default()
                    },
                    right: if left_side {
                        default()
                    } else {
                        px(-size * 0.5)
                    },
                    top: px(center_y - size * 0.5),
                    width: px(size),
                    height: px(size),
                    border: UiRect::all(px(analysis_graph_scaled(1.0, 0.7, zoom))),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(if ready {
                    theme.pitch_contour
                } else {
                    theme.muted
                }),
                BorderColor::all(theme.background.with_alpha(0.9)),
                Pickable::IGNORE,
            ));
        }
    }
}

#[cfg(test)]
mod port_tests {
    use super::{
        analysis_graph_model_ids, analysis_graph_model_labels, analysis_graph_node_progress,
        analysis_graph_port_centers, measured_work_unit_progress, worker_reported_progress,
    };
    use crate::studio::{
        GraphNodeCategory, GraphNodeState, RenderNode, RenderNodeKind, RenderNodeMember,
    };

    fn route(units: Option<(u64, u64)>) -> app_core::AnalysisStageRoute {
        serde_json::from_value(serde_json::json!({
            "stage": "worker",
            "node_id": "pitch",
            "node_event": "node_progress",
            "operation": "window inference",
            "implementation": "openvino_gpu",
            "model": "rmvpe",
            "stage_progress": 99,
            "work_units_completed": units.map(|(completed, _)| completed),
            "work_units_total": units.map(|(_, total)| total),
            "worker_task_id": "rmvpe-task-7",
            "requested_device": "gpu",
            "actual_device": "gpu",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "finished_at_ms": null
        }))
        .unwrap()
    }

    #[test]
    fn dynamic_port_centers_match_the_layout_router_distribution() {
        assert!(analysis_graph_port_centers(112.0, 0, 1.0).is_empty());
        assert_eq!(analysis_graph_port_centers(112.0, 1, 1.0), vec![56.0]);
        assert_eq!(
            analysis_graph_port_centers(112.0, 3, 1.0),
            vec![14.0, 56.0, 98.0]
        );
    }

    #[test]
    fn node_progress_is_determinate_only_from_valid_worker_units() {
        assert_eq!(
            measured_work_unit_progress(&route(Some((2, 4)))),
            Some((50, "2/4 work units".to_string()))
        );
        // §8: the compact card's runtime line must never carry the raw
        // worker task id -- that belongs only in the detailed Inspect view.
        assert!(
            !measured_work_unit_progress(&route(Some((2, 4))))
                .unwrap()
                .1
                .contains("rmvpe-task-7")
        );
        assert_eq!(measured_work_unit_progress(&route(None)), None);
        assert_eq!(measured_work_unit_progress(&route(Some((5, 4)))), None);
        assert_eq!(measured_work_unit_progress(&route(Some((0, 0)))), None);
    }

    #[test]
    fn native_fraction_is_visible_before_work_units_exist() {
        assert_eq!(worker_reported_progress(&route(None)), Some(99));
        assert_eq!(worker_reported_progress(&route(Some((2, 4)))), Some(50));
    }

    #[test]
    fn multiple_models_are_presented_as_independent_lines() {
        assert_eq!(
            analysis_graph_model_ids("rmvpe + fcpe\nrmvpe"),
            ["rmvpe", "fcpe"]
        );
        assert!(analysis_graph_model_ids("default").is_empty());
    }

    #[test]
    fn every_configured_model_keeps_an_explicit_card_label() {
        let model_ids = vec![
            "rmvpe".to_string(),
            "fcpe".to_string(),
            "game".to_string(),
            "basic_pitch".to_string(),
            "rosvot".to_string(),
        ];
        assert_eq!(
            analysis_graph_model_labels(&model_ids).collect::<Vec<_>>(),
            ["RMVPE", "FCPE", "GAME", "Basic Pitch", "ROSVOT"]
        );
    }

    #[test]
    fn purpose_card_progress_combines_all_participating_models() {
        let mut running = route(Some((1, 2)));
        running.node_id = Some("running-model".to_string());
        let live = serde_json::from_value(serde_json::json!({
            "stage": "notes",
            "overall_progress": 50,
            "stage_progress": 50,
            "operation": "running",
            "detail": "",
            "implementation": "native",
            "model": "running-model",
            "device": "gpu",
            "requested_device": "gpu",
            "fallback_from": null,
            "fallback_reason": null,
            "backend_fallback_from": null,
            "backend_fallback_reason": null,
            "stage_routes": [running],
            "node_id": "running-model"
        }))
        .unwrap();
        let task = app_core::AnalysisTask {
            file_hash: "song".to_string(),
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            status: app_core::QueuedStatus::Analyzing(50),
            live: Some(live),
        };
        let members = vec![
            RenderNodeMember {
                id: app_core::AnalysisNodeId::new("complete-model"),
                model_ids: vec!["complete-model".to_string()],
                state: GraphNodeState::Complete,
            },
            RenderNodeMember {
                id: app_core::AnalysisNodeId::new("running-model"),
                model_ids: vec!["running-model".to_string()],
                state: GraphNodeState::Running,
            },
            RenderNodeMember {
                id: app_core::AnalysisNodeId::new("waiting-model"),
                model_ids: vec!["waiting-model".to_string()],
                state: GraphNodeState::Waiting,
            },
            RenderNodeMember {
                id: app_core::AnalysisNodeId::new("skipped-model"),
                model_ids: vec!["skipped-model".to_string()],
                state: GraphNodeState::ProfileSkipped,
            },
        ];
        let node = RenderNode {
            id: app_core::AnalysisNodeId::new("note-boundary"),
            kind: RenderNodeKind::Compute,
            label: "Note boundary".to_string(),
            model_ids: members
                .iter()
                .flat_map(|member| member.model_ids.iter().cloned())
                .collect(),
            detail: String::new(),
            state: GraphNodeState::Running,
            category: GraphNodeCategory::Evidence,
            capability_id: Some("analysis.note_boundary".to_string()),
            terminal_outputs: Vec::new(),
            members,
        };

        // (100 complete + 50 running + 0 waiting) / 3 participating models.
        assert_eq!(analysis_graph_node_progress(&task, &node), Some(50));
    }
}
