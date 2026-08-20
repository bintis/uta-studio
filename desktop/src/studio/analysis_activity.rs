use crate::studio::*;

#[allow(dead_code)]
pub(crate) fn spawn_analysis_history_list(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    if session.analysis_history.is_empty() {
        return;
    }
    parent
        .spawn(Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::axes(px(30), px(18)),
            row_gap: px(8),
            border: UiRect::bottom(px(1)),
            ..default()
        })
        .with_children(|history_list| {
            history_list
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    spawn_text(
                        header,
                        font.clone(),
                        format!("ANALYSIS HISTORY · {}", session.analysis_history.len()),
                        9.0,
                        theme.muted_foreground,
                    );
                    header.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    if !session.pending_analysis_history_clear {
                        spawn_text_button(
                            header,
                            font.clone(),
                            theme,
                            "Clear history…",
                            8.0,
                            UiAction::from(AnalysisCommand::RequestClearAnalysisHistory),
                        );
                    }
                });
            if session.pending_analysis_history_clear {
                history_list
                    .spawn((
                        Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            flex_wrap: FlexWrap::Wrap,
                            padding: UiRect::all(px(11)),
                            column_gap: px(9),
                            row_gap: px(7),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(theme.destructive.with_alpha(0.06)),
                        BorderColor::all(theme.destructive.with_alpha(0.46)),
                    ))
                    .with_children(|confirmation| {
                        spawn_wrapped_text(
                            confirmation,
                            font.clone(),
                            "Delete every saved analysis session? Songs, charts, models, generated assets, and the active queue are not affected.",
                            9.0,
                            theme.muted_foreground,
                        );
                        confirmation.spawn(Node {
                            flex_grow: 1.0,
                            ..default()
                        });
                        spawn_text_button(
                            confirmation,
                            font.clone(),
                            theme,
                            "Cancel",
                            8.0,
                            UiAction::from(AnalysisCommand::CancelClearAnalysisHistory),
                        );
                        spawn_text_button(
                            confirmation,
                            font.clone(),
                            theme,
                            "Delete history",
                            8.0,
                            UiAction::from(AnalysisCommand::ConfirmClearAnalysisHistory),
                        );
                    });
            }
            for history in session.analysis_history.iter().take(20) {
                let selected = session.selected_analysis_history == Some(history.id);
                let duration_seconds =
                    ((history.finished_at_ms - history.started_at_ms).max(0) / 1000) as u64;
                history_list
                    .spawn((
                        Button,
                        UiAction::from(AnalysisCommand::SelectAnalysisHistory(Some(history.id))),
                        Node {
                            width: percent(100),
                            min_height: px(48),
                            align_items: AlignItems::Center,
                            padding: UiRect::axes(px(13), px(9)),
                            column_gap: px(12),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme.primary.with_alpha(0.10)
                        } else {
                            theme.background.with_alpha(0.24)
                        }),
                        BorderColor::all(if selected {
                            theme.primary.with_alpha(0.58)
                        } else {
                            theme.border.with_alpha(0.42)
                        }),
                    ))
                    .with_children(|row| {
                        row.spawn(Node {
                            min_width: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(
                                copy,
                                font.clone(),
                                history.title.clone(),
                                10.0,
                                theme.foreground,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                history.artist.clone(),
                                8.0,
                                theme.muted_foreground,
                            );
                        });
                        spawn_text(
                            row,
                            font.clone(),
                            format!("{}:{:02}", duration_seconds / 60, duration_seconds % 60),
                            8.0,
                            theme.muted_foreground,
                        );
                        spawn_text(
                            row,
                            font.clone(),
                            history.status.to_ascii_uppercase(),
                            8.0,
                            if history.status == "completed" {
                                theme.pitch_contour
                            } else {
                                theme.destructive
                            },
                        );
                    });
            }
        });
}

pub(crate) fn handle_analysis_graph_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    keys: Res<ButtonInput<KeyCode>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    shell: Res<ShellState>,
    library: Res<LibraryState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut viewports: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<AnalysisGraphViewport>,
    >,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if shell.route != StudioRoute::Library || library.library_view != LibraryView::Queue {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        return;
    };
    let Ok((computed, transform, mut position)) = viewports.single_mut() else {
        return;
    };
    if !ui_node_contains_pointer(computed, transform, pointer) {
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl {
        return;
    }
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let mut pan_delta = 0.0_f32;
    let mut zoom_delta = 0.0_f32;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 34.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        if event.x.abs() > f32::EPSILON {
            pan_delta -= event.x * scale;
        } else if shift {
            pan_delta -= event.y * scale;
        } else {
            zoom_delta += event.y * ANALYSIS_GRAPH_ZOOM_STEP / 3.0;
        }
    }
    if zoom_delta.abs() > f32::EPSILON {
        let zoomed = clamp_analysis_graph_zoom(analysis.analysis_graph_zoom + zoom_delta);
        if (zoomed - analysis.analysis_graph_zoom).abs() > f32::EPSILON {
            analysis.analysis_graph_zoom = zoomed;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
    }
    if pan_delta.abs() > f32::EPSILON {
        let size = computed.size() * computed.inverse_scale_factor();
        let content = computed.content_size() * computed.inverse_scale_factor();
        position.x = (position.x + pan_delta).clamp(0.0, (content.x - size.x).max(0.0));
        analysis.analysis_graph_scroll_offset = position.x;
    }
}

pub(crate) fn refresh_analysis_activity(
    time: Res<Time>,
    mut timer: ResMut<AnalysisRefreshTimer>,
    shell: Res<ShellState>,
    mut library: ResMut<LibraryState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let tasks = app_core::load_analysis_tasks();
    let history = app_core::load_analysis_history(100);
    if tasks == analysis.analysis_tasks && history == analysis.analysis_history {
        return;
    }
    analysis.analysis_tasks = tasks;
    analysis.analysis_history = history;
    if (shell.route == StudioRoute::Library && library.library_view == LibraryView::Queue)
        || shell.route == StudioRoute::AnalysisInspect
    {
        library.refresh();
    }
    invalidated.invalidate(UiDirtyRegion::Analysis);
}

fn analysis_page_is_open(route: StudioRoute, library_view: LibraryView) -> bool {
    (route == StudioRoute::Library && library_view == LibraryView::Queue)
        || route == StudioRoute::AnalysisInspect
}

/// Keeps the live DAG node in the middle of the canvas while a run is
/// walking the graph. Recenters only when the running `node_id` changes so
/// a manual pan is not yanked back on every refresh tick.
pub(crate) fn follow_live_analysis_node(
    shell: Res<ShellState>,
    library: Res<LibraryState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut invalidated: ResMut<UiInvalidated>,
    viewports: Query<&ComputedNode, With<AnalysisGraphViewport>>,
) {
    if !analysis_page_is_open(shell.route, library.library_view) {
        if analysis.analysis_graph_follow_node.take().is_some() {
            // Leaving the page drops the follow so reopening recenters.
        }
        return;
    }
    if analysis.analysis_graph_needs_fit {
        return;
    }
    let live_id = analysis
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .and_then(|task| task.live.as_ref())
        .and_then(|live| live.node_id.clone());
    let Some(live_id) = live_id else {
        analysis.analysis_graph_follow_node = None;
        return;
    };
    if analysis.analysis_graph_follow_node.as_deref() == Some(live_id.as_str()) {
        return;
    }
    let viewport_width = viewports
        .iter()
        .next()
        .map(|computed| computed.size().x * computed.inverse_scale_factor())
        .unwrap_or(0.0);
    analysis.analysis_graph_scroll_offset = estimated_analysis_graph_center_scroll(
        &live_id,
        clamp_analysis_graph_zoom(analysis.analysis_graph_zoom),
        viewport_width,
    );
    analysis.analysis_graph_follow_node = Some(live_id);
    invalidated.invalidate(UiDirtyRegion::Analysis);
}

/// Scales the DAG so the full flow fits the current viewport, then leaves
/// zoom alone until the user clicks Fit or switches MINI/Full. Needs a
/// laid-out `AnalysisGraphViewport` so it waits a frame after spawn.
pub(crate) fn fit_analysis_graph_to_viewport(
    shell: Res<ShellState>,
    library: Res<LibraryState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut invalidated: ResMut<UiInvalidated>,
    viewports: Query<(&ComputedNode, &AnalysisGraphViewport)>,
) {
    if !analysis.analysis_graph_needs_fit
        || !analysis_page_is_open(shell.route, library.library_view)
    {
        return;
    }
    let Ok((computed, canvas)) = viewports.single() else {
        return;
    };
    let viewport = computed.size() * computed.inverse_scale_factor();
    if viewport.x < 16.0
        || viewport.y < 16.0
        || canvas.unscaled_width < 8.0
        || canvas.unscaled_height < 8.0
    {
        return;
    }
    let fitted = analysis_graph_fit_zoom(
        canvas.unscaled_width,
        canvas.unscaled_height,
        viewport.x,
        viewport.y,
    );
    analysis.analysis_graph_needs_fit = false;
    analysis.analysis_graph_scroll_offset = 0.0;
    if (fitted - analysis.analysis_graph_zoom).abs() > 0.01 {
        analysis.analysis_graph_zoom = fitted;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    }
}
