use crate::studio::*;

pub(crate) fn handle_activity_panel_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    dialogs: Res<DialogState>,
    mut panels: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<ActivityPanelScroll>,
    >,
) {
    if !dialogs.activity_open {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        return;
    };
    let Ok((computed, transform, mut position)) = panels.single_mut() else {
        return;
    };
    if !ui_node_contains_pointer(computed, transform, pointer) {
        return;
    }
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 24.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            -event.y * scale
        })
        .sum::<f32>();
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}

pub(crate) fn active_analysis_task_count(tasks: &[app_core::AnalysisTask]) -> usize {
    tasks
        .iter()
        .filter(|task| {
            matches!(
                task.status,
                app_core::QueuedStatus::Staged
                    | app_core::QueuedStatus::Queued
                    | app_core::QueuedStatus::Analyzing(_)
            )
        })
        .count()
}

pub(crate) fn handle_analysis_model_panel_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    windows: Query<&Window, With<PrimaryWindow>>,
    analysis: Res<AnalysisUiState>,
    mut panels: Query<
        (&ComputedNode, &UiGlobalTransform, &mut ScrollPosition),
        With<AnalysisModelPanelScroll>,
    >,
) {
    if !analysis.analysis_model_panel_open {
        return;
    }
    let Ok(window) = windows.single() else {
        return;
    };
    let Some(pointer) = window.cursor_position() else {
        return;
    };
    let Ok((computed, transform, mut position)) = panels.single_mut() else {
        return;
    };
    if !ui_node_contains_pointer(computed, transform, pointer) {
        return;
    }
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 24.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.y = (position.y + delta).clamp(0.0, (content.y - size.y).max(0.0));
}

#[allow(clippy::too_many_arguments)]
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
            analysis.analysis_graph_needs_fit = false;
            analysis.analysis_graph_fit_active = false;
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
    let sidebar_count_changed =
        active_analysis_task_count(&analysis.analysis_tasks) != active_analysis_task_count(&tasks);
    analysis.analysis_tasks = tasks;
    analysis.analysis_history = history;
    if (shell.route == StudioRoute::Library && library.library_view == LibraryView::Queue)
        || shell.route == StudioRoute::AnalysisInspect
    {
        library.refresh();
    }
    invalidated.invalidate(UiDirtyRegion::Analysis);
    if sidebar_count_changed {
        // The Analysis badge and the Activity indicator live in the persistent
        // chrome, outside the analysis workspace rebuilt above.
        invalidated.invalidate(UiDirtyRegion::Chrome);
    }
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
    if analysis.analysis_graph_needs_fit || !analysis.analysis_graph_follow_enabled {
        return;
    }
    let active_task = analysis.analysis_tasks.iter().find(|task| {
        matches!(task.status, app_core::QueuedStatus::Analyzing(_))
            && library
                .selected_song
                .as_ref()
                .is_none_or(|hash| hash == &task.file_hash)
    });
    let live_id = active_task
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
    let workflow = active_task
        .and_then(|task| task.live.as_ref())
        .and_then(|live| live.engine.as_ref())
        .and_then(exact_workflow_plan_from_engine)
        .map(|(workflow, _)| workflow)
        .or_else(|| {
            analysis.workflow_snapshot.as_ref().and_then(|snapshot| {
                app_core::WorkflowExecutionWireV1::from_snapshot(snapshot).ok()
            })
        });
    analysis.analysis_graph_scroll_offset = estimated_analysis_graph_center_scroll(
        workflow.as_ref(),
        &live_id,
        clamp_analysis_graph_zoom(analysis.analysis_graph_zoom),
        viewport_width,
    );
    analysis.analysis_graph_follow_node = Some(live_id);
    invalidated.invalidate(UiDirtyRegion::Analysis);
}

fn stable_analysis_viewport(value: f32) -> f32 {
    // UI scale, scrollbars and deferred entity replacement can make the same
    // viewport oscillate by fractions of a logical pixel. Quantizing here keeps
    // those layout jitters from rebuilding the entire Analysis workspace.
    (value / 8.0).round() * 8.0
}

/// Scales the DAG so the full flow fits the current viewport, then leaves
/// zoom alone until the user clicks Fit. Needs a
/// laid-out `AnalysisGraphViewport` so it waits a frame after spawn.
pub(crate) fn fit_analysis_graph_to_viewport(
    shell: Res<ShellState>,
    library: Res<LibraryState>,
    mut analysis: ResMut<AnalysisUiState>,
    mut invalidated: ResMut<UiInvalidated>,
    viewports: Query<(&ComputedNode, &AnalysisGraphViewport)>,
) {
    if !analysis_page_is_open(shell.route, library.library_view) {
        return;
    }
    let Ok((computed, canvas)) = viewports.single() else {
        return;
    };
    let measured = computed.size() * computed.inverse_scale_factor();
    let viewport = Vec2::new(
        stable_analysis_viewport(measured.x),
        stable_analysis_viewport(measured.y),
    );
    if viewport.x < 16.0
        || viewport.y < 16.0
        || canvas.unscaled_width < 8.0
        || canvas.unscaled_height < 8.0
    {
        return;
    }
    let viewport_changed = (analysis.analysis_graph_viewport_width - viewport.x).abs() >= 8.0
        || (analysis.analysis_graph_viewport_height - viewport.y).abs() >= 8.0;
    if !analysis.analysis_graph_needs_fit
        && !(analysis.analysis_graph_fit_active && viewport_changed)
    {
        // Geometry metadata is useful to later focus commands, but it does not
        // justify replacing every analysis entity. The next explicit Fit,
        // meaningful resize will rebuild once.
        if viewport_changed {
            analysis.analysis_graph_viewport_width = viewport.x;
            analysis.analysis_graph_viewport_height = viewport.y;
        }
        return;
    }
    // Metro geometry is already packed against the measured viewport. Never
    // shrink it below 100% just to expose empty canvas on both axes; an
    // unusually deep workflow scrolls vertically like a tile surface.
    let fitted = analysis_graph_fit_zoom(
        canvas.unscaled_width,
        canvas.unscaled_height,
        viewport.x,
        viewport.y,
    )
    .max(ANALYSIS_GRAPH_ZOOM_DEFAULT);
    // `ComputedNode::size` is in the scaled UI coordinate space. The graph
    // geometry and zoom helpers use logical pixels, so persist the same
    // inverse-scaled width used by Fit above. Mixing the two spaces expands
    // the next canvas by the display/UI scale and clips the output phase.
    let layout_width = viewport.x;
    analysis.analysis_graph_viewport_width = layout_width;
    analysis.analysis_graph_viewport_height = viewport.y;
    analysis.analysis_graph_needs_fit = false;
    analysis.analysis_graph_scroll_offset = 0.0;
    analysis.analysis_graph_vertical_scroll_offset = 0.0;
    if (fitted - analysis.analysis_graph_zoom).abs() > 0.01 || viewport_changed {
        analysis.analysis_graph_zoom = fitted;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    }
}

#[cfg(test)]
mod viewport_stability_tests {
    use super::stable_analysis_viewport;

    #[test]
    fn viewport_jitter_quantizes_to_one_layout_size() {
        assert_eq!(stable_analysis_viewport(799.4), 800.0);
        assert_eq!(stable_analysis_viewport(800.6), 800.0);
        assert_eq!(stable_analysis_viewport(804.1), 808.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(status: app_core::QueuedStatus) -> app_core::AnalysisTask {
        app_core::AnalysisTask {
            file_hash: "song".to_string(),
            title: "Song".to_string(),
            artist: "Artist".to_string(),
            status,
            live: None,
        }
    }

    #[test]
    fn active_task_count_tracks_only_sidebar_badge_jobs() {
        let tasks = [
            task(app_core::QueuedStatus::Staged),
            task(app_core::QueuedStatus::Queued),
            task(app_core::QueuedStatus::Analyzing(42)),
            task(app_core::QueuedStatus::Failed("failed".to_string())),
        ];

        assert_eq!(active_analysis_task_count(&tasks), 3);
        assert_eq!(
            active_analysis_task_count(&[task(app_core::QueuedStatus::Failed(
                "failed".to_string()
            ))]),
            0
        );
    }
}
