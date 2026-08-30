use super::*;
use crate::studio::*;

pub(crate) fn analysis_graph_category_accent(
    category: GraphNodeCategory,
    theme: &StudioTheme,
) -> Color {
    match (theme.dark, category) {
        (true, GraphNodeCategory::Source) => Color::srgb(0.38, 0.66, 1.0),
        (true, GraphNodeCategory::Audio) => Color::srgb(0.25, 0.84, 0.80),
        (true, GraphNodeCategory::Lyrics) => Color::srgb(0.96, 0.68, 0.18),
        (true, GraphNodeCategory::Pitch) => Color::srgb(0.78, 0.50, 0.93),
        (true, GraphNodeCategory::Evidence) => Color::srgb(0.65, 0.58, 0.94),
        (true, GraphNodeCategory::Fusion) => Color::srgb(0.46, 0.86, 0.40),
        (true, GraphNodeCategory::Output) => Color::srgb(0.35, 0.78, 0.96),
        (false, GraphNodeCategory::Source) => Color::srgb(0.16, 0.42, 0.76),
        (false, GraphNodeCategory::Audio) => Color::srgb(0.05, 0.52, 0.50),
        (false, GraphNodeCategory::Lyrics) => Color::srgb(0.72, 0.42, 0.02),
        (false, GraphNodeCategory::Pitch) => Color::srgb(0.55, 0.25, 0.70),
        (false, GraphNodeCategory::Evidence) => Color::srgb(0.42, 0.34, 0.70),
        (false, GraphNodeCategory::Fusion) => Color::srgb(0.22, 0.58, 0.18),
        (false, GraphNodeCategory::Output) => Color::srgb(0.08, 0.48, 0.67),
    }
}

pub(crate) fn spawn_analysis_graph_status_pill(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    text: String,
    color: Color,
    zoom: f32,
) {
    spawn_analysis_graph_status_pill_at(parent, font, text, color, zoom, 7.0);
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

fn spawn_analysis_graph_model_tag(
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
                height: px(analysis_graph_scaled(21.0, 15.0, zoom)),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(analysis_graph_scaled(6.0, 4.0, zoom))),
                overflow: Overflow::clip(),
                border: UiRect::all(px(analysis_graph_scaled(1.0, 0.7, zoom))),
                border_radius: BorderRadius::all(px(analysis_graph_scaled(4.0, 3.0, zoom))),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.30)),
            BorderColor::all(accent.with_alpha(0.42)),
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

fn spawn_analysis_graph_legend_item(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    detail: &str,
    accent: Color,
    round: bool,
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(7),
            ..default()
        })
        .with_children(|item| {
            item.spawn((
                Node {
                    width: px(16),
                    height: px(16),
                    flex_shrink: 0.0,
                    border: UiRect::all(px(1)),
                    border_radius: if round {
                        BorderRadius::MAX
                    } else {
                        BorderRadius::all(px(4))
                    },
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.14)),
                BorderColor::all(accent.with_alpha(0.82)),
            ));
            item.spawn(Node {
                flex_direction: FlexDirection::Column,
                row_gap: px(1),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 7.0, theme.foreground);
                spawn_text(copy, font, detail, 5.8, theme.muted_foreground);
            });
        });
}

pub(crate) fn spawn_analysis_graph_legend(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(48),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                column_gap: px(22),
                flex_wrap: FlexWrap::Wrap,
                row_gap: px(7),
                padding: UiRect::axes(px(12), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.30)),
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|legend| {
            spawn_text(legend, font.clone(), "STATUS", 6.5, theme.muted_foreground);
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Complete",
                "Finished successfully",
                analysis_graph_category_accent(GraphNodeCategory::Output, theme),
                true,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Waiting",
                "Pending execution",
                theme.muted_foreground,
                true,
            );
            spawn_text(
                legend,
                font.clone(),
                "NODE TYPES",
                6.5,
                theme.muted_foreground,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Processing step",
                "Computational task",
                analysis_graph_category_accent(GraphNodeCategory::Audio, theme),
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Artifact",
                "Intermediate data",
                theme.muted_foreground,
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font.clone(),
                theme,
                "Authoring step",
                "Chart creation",
                analysis_graph_category_accent(GraphNodeCategory::Fusion, theme),
                false,
            );
            spawn_analysis_graph_legend_item(
                legend,
                font,
                theme,
                "Final output",
                "Deliverable",
                analysis_graph_category_accent(GraphNodeCategory::Output, theme),
                false,
            );
        });
}

pub(crate) fn measured_work_unit_progress(
    route: &app_core::AnalysisStageRoute,
) -> Option<(usize, String)> {
    let (completed, total) = route.work_units_completed.zip(route.work_units_total)?;
    let task_id = route.worker_task_id.as_deref()?.trim();
    if total == 0 || completed > total || task_id.is_empty() {
        return None;
    }
    let percent = ((completed.saturating_mul(100) / total).min(100)) as usize;
    Some((
        percent,
        format!("{completed}/{total} work units · task {task_id}"),
    ))
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    node_id: &str,
    completed: bool,
) -> (String, bool) {
    let route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, node_id));
    let Some(route) = route else {
        return (
            if completed {
                "Complete · no runtime trace".to_string()
            } else {
                "Awaiting connected inputs".to_string()
            },
            false,
        );
    };
    let warning = route.fallback_from.is_some() || route.backend_fallback_from.is_some();
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
    let model = if !route.model.trim().is_empty() {
        route.model.as_str()
    } else {
        "default"
    };
    let measured = measured_work_unit_progress(route)
        .map(|(_, units)| format!(" · {units}"))
        .unwrap_or_default();
    (format!("{implementation} · {model}{measured}"), warning)
}

pub(crate) struct WorkflowNodeCardSpec<'a> {
    pub(crate) bounds: AnalysisGraphBox,
    pub(crate) capability_id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) label: &'a str,
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
    let (status, progress, status_color, status_glyph) = match state {
        WorkflowNodeVisualState::Waiting => ("WAITING", Some(0), theme.muted_foreground, "○"),
        WorkflowNodeVisualState::Running(progress) => ("RUNNING", progress, accent, "●"),
        WorkflowNodeVisualState::Complete => ("COMPLETE", Some(100), complete_color, "✓"),
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
    let failed = matches!(state, WorkflowNodeVisualState::Failed);
    let context_node_id = node_id.to_string();
    let context_capability_id = capability_id.to_string();
    let context_file_hash = file_hash.to_string();
    let context_label = label.to_string();
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
                    bottom: px(analysis_graph_scaled(28.0, 19.0, zoom)),
                },
                row_gap: px(gap),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(analysis_graph_scaled(8.0, 5.0, zoom))),
                ..default()
            },
            BackgroundColor(if running {
                accent.with_alpha(if dimmed { 0.06 } else { 0.15 })
            } else if selected {
                theme.card.with_alpha(if dimmed { 0.35 } else { 0.96 })
            } else {
                theme.card.with_alpha(if dimmed { 0.22 } else { 0.90 })
            }),
            BorderColor::all(if selected {
                accent.with_alpha(0.96)
            } else if running {
                accent.with_alpha(if dimmed { 0.22 } else { 0.72 })
            } else if complete {
                accent.with_alpha(if dimmed { 0.16 } else { 0.70 })
            } else if failed {
                theme
                    .destructive
                    .with_alpha(if dimmed { 0.18 } else { 0.60 })
            } else {
                accent.with_alpha(if dimmed { 0.18 } else { 0.46 })
            }),
            BoxShadow::new(
                accent.with_alpha(if dimmed {
                    0.0
                } else if running {
                    0.52
                } else if selected {
                    0.20
                } else {
                    0.045
                }),
                px(0),
                px(0),
                px(if running {
                    analysis_graph_scaled(20.0, 12.0, zoom)
                } else {
                    analysis_graph_scaled(8.0, 5.0, zoom)
                }),
                px(if running { (2.0 * zoom).max(1.0) } else { 0.0 }),
            ),
            ZIndex(2),
        ))
        .with_children(|node| {
            spawn_analysis_graph_ports(
                node,
                theme,
                complete || running,
                zoom,
                bounds.height,
                input_ports,
                output_ports,
            );
            node.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(analysis_graph_scaled(7.0, 4.0, zoom)),
                    right: px(analysis_graph_scaled(7.0, 4.0, zoom)),
                    top: px(0),
                    height: px((1.0 * zoom).max(0.7)),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent.with_alpha(if running {
                    0.92
                } else if complete {
                    0.46
                } else {
                    0.24
                })),
                Pickable::IGNORE,
            ));
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
                                border: UiRect::all(px((1.0 * zoom).max(0.7))),
                                border_radius: BorderRadius::all(px(analysis_graph_scaled(
                                    5.0, 3.0, zoom,
                                ))),
                                ..default()
                            },
                            BackgroundColor(accent.with_alpha(if running || complete {
                                0.20
                            } else {
                                0.11
                            })),
                            BorderColor::all(accent.with_alpha(if running || complete {
                                0.42
                            } else {
                                0.24
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
                    });
            });
            let copy = if zoom < 0.60 {
                if running {
                    progress.map_or_else(|| "…".to_string(), |value| format!("{value}%"))
                } else {
                    status_glyph.to_string()
                }
            } else if running {
                progress.map_or_else(
                    || format!("{status_glyph} {status} · measured progress unavailable"),
                    |value| format!("{status_glyph} {status} {value}%"),
                )
            } else {
                format!("{status_glyph} {status}")
            };
            if zoom >= 0.60 {
                spawn_analysis_graph_status_pill_at(
                    node,
                    font.clone(),
                    copy,
                    status_color,
                    zoom,
                    31.0,
                );
                spawn_analysis_graph_model_tag(
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
            } else {
                spawn_analysis_graph_status_pill(node, font, copy, status_color, zoom);
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
    use super::{analysis_graph_port_centers, measured_work_unit_progress};

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
            "worker_task_id": units.map(|_| "rmvpe-task-7"),
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
            Some((50, "2/4 work units · task rmvpe-task-7".to_string()))
        );
        assert_eq!(measured_work_unit_progress(&route(None)), None);
        assert_eq!(measured_work_unit_progress(&route(Some((5, 4)))), None);
        assert_eq!(measured_work_unit_progress(&route(Some((0, 0)))), None);
    }
}
