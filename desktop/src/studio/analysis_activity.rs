use crate::studio::*;

pub(crate) fn spawn_analysis_history_list(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
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
                            UiAction::RequestClearAnalysisHistory,
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
                            UiAction::CancelClearAnalysisHistory,
                        );
                        spawn_text_button(
                            confirmation,
                            font.clone(),
                            theme,
                            "Delete history",
                            8.0,
                            UiAction::ConfirmClearAnalysisHistory,
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
                        UiAction::SelectAnalysisHistory(Some(history.id)),
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
    mut session: ResMut<StudioSession>,
    mut viewports: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<AnalysisGraphViewport>,
    >,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if session.route != StudioRoute::Library {
        wheel.clear();
        return;
    }
    let Ok(window) = windows.single() else {
        wheel.clear();
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        wheel.clear();
        return;
    };
    let Ok((computed, transform, mut position)) = viewports.single_mut() else {
        wheel.clear();
        return;
    };
    if !ui_node_contains_pointer(computed, transform, pointer) {
        wheel.clear();
        return;
    }
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if !ctrl {
        // The graph canvas isn't the only thing a bare wheel scrolls here --
        // the surrounding page reacts to the same wheel too, so consuming it
        // in this system as well (to pan or zoom) produced two conflicting
        // motions at once for an ordinary scroll gesture. Requiring Ctrl,
        // the same modifier the note editor's own canvas uses for zoom
        // (`handle_editor_wheel`), means this system only ever touches the
        // wheel when the user has clearly opted into graph-local pan/zoom.
        wheel.clear();
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
        let zoomed = clamp_analysis_graph_zoom(session.analysis_graph_zoom + zoom_delta);
        if (zoomed - session.analysis_graph_zoom).abs() > f32::EPSILON {
            session.analysis_graph_zoom = zoomed;
            invalidated.0 = true;
        }
    }
    if pan_delta.abs() > f32::EPSILON {
        let size = computed.size() * computed.inverse_scale_factor();
        let content = computed.content_size() * computed.inverse_scale_factor();
        position.x = (position.x + pan_delta).clamp(0.0, (content.x - size.x).max(0.0));
        session.analysis_graph_scroll_offset = position.x;
    }
}

pub(crate) fn refresh_analysis_activity(
    time: Res<Time>,
    mut timer: ResMut<AnalysisRefreshTimer>,
    mut session: ResMut<StudioSession>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let tasks = app_core::load_analysis_tasks();
    let history = app_core::load_analysis_history(100);
    if tasks == session.analysis_tasks && history == session.analysis_history {
        return;
    }
    session.analysis_tasks = tasks;
    session.analysis_history = history;
    if session.route == StudioRoute::Library && session.library_view == LibraryView::Queue {
        session.refresh_library();
    }
    invalidated.0 = true;
}
