use super::*;
use crate::studio::*;

pub(crate) fn spawn_analysis_graph_lane_band(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    band: LayoutLaneBand,
    zoom: f32,
) {
    let bounds = zoomed_box(band.rect, zoom);
    let accent = match band.kind {
        LayoutLaneKind::Music => theme.primary,
        LayoutLaneKind::VocalsAndPitch => theme.pitch_contour,
        LayoutLaneKind::LyricsAndTiming => theme.editor_warning,
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                padding: UiRect::axes(px(10.0 * zoom), px(3.0 * zoom)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(9)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.16)),
            BorderColor::all(accent.with_alpha(0.18)),
            ZIndex(-1),
            Pickable::IGNORE,
        ))
        .with_children(|lane| {
            lane.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(0),
                    top: px(9.0 * zoom),
                    bottom: px(9.0 * zoom),
                    width: px(2),
                    border_radius: BorderRadius::MAX,
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.7)),
                Pickable::IGNORE,
            ));
            spawn_text(
                lane,
                font,
                band.kind.label(),
                (6.5 * zoom).clamp(6.0, 8.0),
                accent.with_alpha(0.78),
            );
        });
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    node_id: &str,
    stage_id: &str,
    completed: bool,
) -> (String, bool) {
    let route = task
        .live
        .as_ref()
        .and_then(|live| find_matching_route(&live.stage_routes, node_id, stage_id));
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
    (format!("{implementation} · {model}"), warning)
}

pub(crate) struct AnalysisStageNodeSpec<'a> {
    pub(crate) bounds: AnalysisGraphBox,
    pub(crate) index: usize,
    pub(crate) stage_id: &'a str,
    pub(crate) node_id: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) label: &'a str,
    pub(crate) state: AnalysisGraphStageState,
    pub(crate) selected: bool,
    pub(crate) route: &'a str,
    pub(crate) warning: bool,
    pub(crate) dimmed: bool,
}

pub(crate) fn spawn_analysis_stage_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    spec: AnalysisStageNodeSpec,
) {
    let AnalysisStageNodeSpec {
        bounds,
        index,
        stage_id,
        node_id,
        file_hash,
        label,
        state,
        selected,
        route,
        warning,
        dimmed,
    } = spec;
    let (status, progress, status_color) = match state {
        AnalysisGraphStageState::Waiting => ("WAITING", 0, theme.muted_foreground),
        AnalysisGraphStageState::Running(progress) => ("RUNNING", progress, theme.primary),
        AnalysisGraphStageState::Complete => ("COMPLETE", 100, theme.pitch_contour),
    };
    let running = matches!(state, AnalysisGraphStageState::Running(_));
    let complete = matches!(state, AnalysisGraphStageState::Complete);
    let context_node_id = node_id.to_string();
    let context_stage_id = stage_id.to_string();
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
                padding: UiRect::all(px(8)),
                row_gap: px(4),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(if running {
                theme.primary.with_alpha(if dimmed { 0.06 } else { 0.16 })
            } else if selected {
                theme.card.with_alpha(if dimmed { 0.35 } else { 0.9 })
            } else {
                theme.card.with_alpha(if dimmed { 0.22 } else { 0.68 })
            }),
            BorderColor::all(if selected {
                theme.primary.with_alpha(0.92)
            } else if running {
                theme.primary.with_alpha(if dimmed { 0.22 } else { 0.62 })
            } else if complete {
                theme
                    .pitch_contour
                    .with_alpha(if dimmed { 0.16 } else { 0.42 })
            } else {
                theme.border.with_alpha(if dimmed { 0.28 } else { 0.68 })
            }),
            ZIndex(2),
        ))
        .with_children(|node| {
            spawn_analysis_graph_ports(node, theme, complete || running);
            if selected {
                node.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(9),
                        bottom: px(9),
                        width: px(2),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.primary),
                    Pickable::IGNORE,
                ));
            }
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|heading| {
                heading
                    .spawn((
                        Node {
                            width: px(22),
                            height: px(22),
                            flex_shrink: 0.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(if running {
                            theme.primary
                        } else if complete {
                            theme.pitch_contour
                        } else {
                            theme.muted
                        }),
                    ))
                    .with_children(|badge| {
                        spawn_text(
                            badge,
                            font.clone(),
                            format!("{:02}", index + 1),
                            7.0,
                            if running || complete {
                                theme.background
                            } else {
                                theme.muted_foreground
                            },
                        );
                    });
                heading
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        ..default()
                    })
                    .with_children(|copy| {
                        spawn_text(copy, font.clone(), label, 9.0, theme.foreground);
                        spawn_text(copy, font.clone(), status, 7.0, status_color);
                    });
            });
            node.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|meter| {
                meter
                    .spawn((
                        Node {
                            min_width: px(0),
                            height: px(3),
                            flex_grow: 1.0,
                            overflow: Overflow::clip(),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(theme.muted.with_alpha(0.72)),
                    ))
                    .with_children(|rail| {
                        rail.spawn((
                            Node {
                                width: percent(progress as f32),
                                height: percent(100),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(if complete {
                                theme.pitch_contour
                            } else {
                                theme.primary
                            }),
                        ));
                    });
                spawn_text(
                    meter,
                    font.clone(),
                    format!("{progress}%"),
                    7.0,
                    status_color,
                );
            });
            spawn_bounded_wrapped_text(
                node,
                font,
                route,
                7.0,
                if warning {
                    theme.editor_warning
                } else {
                    theme.muted_foreground
                },
            );
        })
        .observe(
            move |mut event: On<Pointer<Press>>,
                  mut analysis: ResMut<AnalysisUiState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_analysis_node_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    AnalysisNodeClickTarget {
                        node_id: &context_node_id,
                        label: &context_label,
                        file_hash: &context_file_hash,
                        stage_id: &context_stage_id,
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
pub(crate) fn analysis_context_menu_position(
    window_position: Vec2,
    scroll_offset: f32,
    lists: &Query<(&ComputedNode, &UiGlobalTransform), With<LibrarySongList>>,
) -> Vec2 {
    let Ok((computed, transform)) = lists.single() else {
        return window_position;
    };
    // `UiGlobalTransform` maps into space centered on the node (matching
    // `ui_node_contains_pointer`'s own use of it), not space anchored at its
    // top-left corner -- which is what `Node::left`/`Node::top` on the
    // absolute-positioned menu actually need. Add back half the list's own
    // size to shift the origin from its center to its corner.
    let center_relative = transform
        .affine()
        .inverse()
        .transform_point2(window_position);
    let half_size = computed.size() * computed.inverse_scale_factor() / 2.0;
    let mut local = center_relative + half_size;
    // Bevy's UI layout subtracts a node's *parent's* scroll position from
    // its rendered spot regardless of position type -- an absolute child of
    // a scrolling node still moves with that scroll (unlike CSS, where
    // `position: absolute` normally opts out of that). The menu is a direct
    // child of `LibrarySongList`, so without adding the list's current
    // scroll back in here, the menu would render offset from the node by
    // however far the list had been scrolled at click time. The list only
    // scrolls vertically (`ScrollPosition(Vec2::new(0.0, ...))`), so only Y
    // needs it.
    local.y += scroll_offset;
    local
}

/// `click_target`, when set, is `(node_id, label, file_hash, stage_id)` of
/// the *real* compute node this virtual box's output belongs to (an
/// Artifact/Export box is never itself a real `AnalysisGraphSpec` node --
/// see `build_render_graph`'s doc comment) -- clicking it opens the same
/// node context menu (right-click) / inspector selection (left-click) as
/// that real node, via `open_analysis_node_from_pointer`, so "Run this node
/// only" etc. on an output box runs the compute step that actually produces
/// it.
#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
pub(crate) fn spawn_analysis_artifact_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    eyebrow: &str,
    title: &str,
    detail: &str,
    ready: bool,
    output: bool,
    click_target: Option<(String, String, String, String)>,
) {
    let accent = if output {
        theme.primary
    } else {
        theme.pitch_contour
    };
    let mut entity = parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(bounds.x),
            top: px(bounds.y),
            width: px(bounds.width),
            height: px(bounds.height),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            padding: UiRect::axes(px(12), px(8)),
            row_gap: px(2),
            overflow: Overflow::clip(),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(if output { 8 } else { 18 })),
            ..default()
        },
        BackgroundColor(if ready {
            accent.with_alpha(0.1)
        } else {
            theme.background.with_alpha(0.72)
        }),
        BorderColor::all(if ready {
            accent.with_alpha(0.62)
        } else {
            theme.border.with_alpha(0.62)
        }),
        ZIndex(2),
    ));
    if click_target.is_some() {
        entity.insert((
            Button,
            UiPointerApi(&[
                "ui.pointer.analysis_artifact.primary",
                "ui.pointer.analysis_artifact.secondary",
            ]),
        ));
    }
    entity.with_children(|node| {
        spawn_analysis_graph_ports(node, theme, ready);
        spawn_text(
            node,
            font.clone(),
            format!(
                "{eyebrow} · {}",
                if ready {
                    if output { "AVAILABLE" } else { "READY" }
                } else {
                    "PENDING"
                }
            ),
            6.5,
            if ready {
                accent
            } else {
                theme.muted_foreground
            },
        );
        spawn_text(node, font.clone(), title, 9.0, theme.foreground);
        spawn_bounded_wrapped_text(node, font, detail, 7.0, theme.muted_foreground);
    });
    if let Some((node_id, label, file_hash, stage_id)) = click_target {
        entity.observe(
            move |mut event: On<Pointer<Press>>,
                  mut analysis: ResMut<AnalysisUiState>,
                  mut dialogs: ResMut<DialogState>,
                  mut invalidated: ResMut<UiInvalidated>| {
                event.propagate(false);
                open_analysis_node_from_pointer(
                    event.button,
                    event.pointer_location.position,
                    AnalysisNodeClickTarget {
                        node_id: &node_id,
                        label: &label,
                        file_hash: &file_hash,
                        stage_id: &stage_id,
                    },
                    &mut analysis,
                    &mut dialogs,
                    &mut invalidated,
                );
            },
        );
    }
}

pub(crate) fn spawn_analysis_graph_ports(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    ready: bool,
) {
    for (left, right) in [(Some(px(-5)), None), (None, Some(px(-5)))] {
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: left.unwrap_or_default(),
                right: right.unwrap_or_default(),
                top: percent(50),
                width: px(10),
                height: px(10),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            UiTransform::from_xy(px(0), px(-5)),
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

#[allow(dead_code)]
pub(crate) fn spawn_analysis_graph_path(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    points: &[Vec2],
    ready: bool,
) {
    let color = if ready {
        theme.pitch_contour.with_alpha(0.68)
    } else {
        theme.border.with_alpha(0.64)
    };
    spawn_analysis_graph_segments(parent, points, color, 2.0, false);
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_graph_binding_path(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    points: &[Vec2],
    edge: &RenderEdge,
    binding: Option<&app_core::ArtifactBinding>,
    selected: bool,
    dimmed: bool,
    show_label: bool,
) {
    let state = binding
        .map(|binding| binding.state)
        .unwrap_or(app_core::ArtifactBindingState::NotApplicable);
    let mut color = match state {
        app_core::ArtifactBindingState::Resolved => theme.pitch_contour,
        app_core::ArtifactBindingState::FrozenReuse => theme.primary,
        app_core::ArtifactBindingState::Bypassed => theme.editor_warning,
        app_core::ArtifactBindingState::Missing | app_core::ArtifactBindingState::Invalidated => {
            theme.destructive
        }
        app_core::ArtifactBindingState::Ephemeral
        | app_core::ArtifactBindingState::LegacyUntracked => theme.editor_warning,
        app_core::ArtifactBindingState::Source => theme.muted_foreground,
        app_core::ArtifactBindingState::NotApplicable => theme.border,
    };
    let alpha = if dimmed {
        0.18
    } else if selected {
        0.95
    } else {
        0.72
    };
    color = color.with_alpha(alpha);
    let thickness = if selected {
        3.5
    } else if matches!(edge.role, RenderEdgeRole::ExportTarget) {
        2.0
    } else {
        2.25
    };
    spawn_analysis_graph_segments(parent, points, color, thickness, true);

    let selected_edge = selected_graph_edge_from_binding(
        &edge.from,
        &edge.to,
        &edge.producer_node,
        edge.artifact_kind,
        binding,
    );
    let click_edge = selected_edge.clone();
    if let (Some(first), Some(last)) = (points.first(), points.last()) {
        let hit_left = first.x.min(last.x) - 6.0;
        let hit_top = first.y.min(last.y) - 8.0;
        let hit_width = (first.x - last.x).abs().max(16.0) + 12.0;
        let hit_height = (first.y - last.y).abs().max(16.0) + 16.0;
        parent
            .spawn((
                Button,
                UiPointerApi(&["ui.pointer.analysis_edge.primary"]),
                Node {
                    position_type: PositionType::Absolute,
                    left: px(hit_left),
                    top: px(hit_top),
                    width: px(hit_width),
                    height: px(hit_height),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                ZIndex(1),
            ))
            .observe(
                move |mut event: On<Pointer<Click>>,
                      mut shell: ResMut<ShellState>,
                      library: Res<LibraryState>,
                      mut analysis: ResMut<AnalysisUiState>,
                      mut dialogs: ResMut<DialogState>,
                      mut invalidated: ResMut<UiInvalidated>| {
                    event.propagate(false);
                    if event.button != PointerButton::Primary {
                        return;
                    }
                    let short = click_edge
                        .revision_id
                        .as_deref()
                        .map(|id| {
                            id.chars()
                                .rev()
                                .take(10)
                                .collect::<String>()
                                .chars()
                                .rev()
                                .collect::<String>()
                        })
                        .unwrap_or_else(|| "no revision".to_string());
                    let kind = click_edge
                        .kind
                        .map(|kind| format!("{kind:?}"))
                        .unwrap_or_else(|| "compute".to_string());
                    shell.notice = Some(format!(
                        "{kind} · {} · {short}",
                        edge_binding_style_copy(click_edge.state)
                    ));
                    analysis.selected_graph_edge = Some(click_edge.clone());
                    if let Some(revision_id) = click_edge.revision_id.as_ref()
                        && let Some(kind) = click_edge.kind
                    {
                        let reference = app_core::ArtifactRef {
                            file_hash: analysis
                                .selected_analysis_history
                                .and_then(|id| {
                                    analysis
                                        .analysis_history
                                        .iter()
                                        .find(|history| history.id == id)
                                        .map(|history| history.file_hash.clone())
                                })
                                .or_else(|| library.selected_song.clone())
                                .unwrap_or_default(),
                            kind,
                            revision_id: revision_id.clone(),
                        };
                        if analysis.analysis_lineage_mode
                            && let Ok(lineage) = app_core::artifact_lineage(&reference)
                        {
                            dialogs.artifact_lineage = Some(ArtifactLineagePanel {
                                lineage,
                                scope: analysis.analysis_lineage_scope,
                                selected: reference,
                            });
                        }
                    }
                    invalidated.invalidate(UiDirtyRegion::Analysis);
                },
            );
    }

    if (show_label || selected)
        && let Some(mid) = points.get(points.len() / 2)
    {
        let kind = edge
            .artifact_kind
            .map(|kind| format!("{kind:?}"))
            .unwrap_or_else(|| "edge".to_string());
        let short = binding
            .and_then(|item| item.artifact_ref.as_ref())
            .map(|reference| {
                reference
                    .revision_id
                    .chars()
                    .rev()
                    .take(8)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect::<String>()
            })
            .unwrap_or_default();
        let label = if short.is_empty() {
            format!("{kind} · {}", edge_binding_style_copy(state))
        } else {
            format!("{kind} · {short}")
        };
        parent
            .spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(mid.x - 46.0),
                    top: px(mid.y - 10.0),
                    max_width: px(120.0),
                    padding: UiRect::axes(px(4), px(2)),
                    border_radius: BorderRadius::all(px(3)),
                    ..default()
                },
                BackgroundColor(theme.card.with_alpha(0.92)),
                ZIndex(3),
                Pickable::IGNORE,
            ))
            .with_children(|chip| {
                spawn_text(chip, font, label, 6.5, theme.muted_foreground);
            });
    }
}

fn spawn_analysis_graph_segments(
    parent: &mut ChildSpawnerCommands,
    points: &[Vec2],
    color: Color,
    thickness: f32,
    pickable_segments: bool,
) {
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let horizontal = (from.y - to.y).abs() <= 0.5;
        let left = from.x.min(to.x);
        let top = from.y.min(to.y);
        let mut entity = parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(if horizontal {
                    (to.x - from.x).abs().max(thickness)
                } else {
                    thickness
                }),
                height: px(if horizontal {
                    thickness
                } else {
                    (to.y - from.y).abs().max(thickness)
                }),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            ZIndex(0),
        ));
        if !pickable_segments {
            entity.insert(Pickable::IGNORE);
        }
    }
}
