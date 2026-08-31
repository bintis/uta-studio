use crate::studio::*;

#[derive(Resource)]
pub(crate) struct AnalysisRefreshTimer(pub(crate) Timer);

#[derive(Resource)]
pub(crate) struct AnalysisLogRefreshTimer(pub(crate) Timer);

#[derive(Component, Clone, Copy)]
pub(crate) struct AnalysisGraphViewport {
    pub(crate) unscaled_width: f32,
    pub(crate) unscaled_height: f32,
}

#[derive(Component)]
pub(crate) struct AnalysisLogViewerScroll;

#[derive(Component)]
pub(crate) struct AnalysisLogViewerOutput;

#[derive(Component)]
pub(crate) struct PlanPreviewScroll;

#[derive(Clone)]
pub(crate) struct AnalysisNodeMenuAction {
    pub(crate) label: &'static str,
    pub(crate) action: UiAction,
}

#[derive(Clone)]
pub(crate) struct AnalysisNodeContextMenu {
    pub(crate) node_id: String,
    /// Compiled capability identity shown by the inspector.
    pub(crate) capability_id: String,
    pub(crate) label: String,
    pub(crate) run_action: Option<AnalysisNodeMenuAction>,
    /// `None` when no history run is currently selected -- "Compare with
    /// previous attempt" needs a `current_run_id` to diff against, which
    /// only exists once a run is selected in the Activity/Queue view.
    pub(crate) compare_node_action: Option<UiAction>,
    /// §7.5's last item, "View logs": always offered because a node filter is
    /// meaningful for every node. Opens the selected run's dedicated JSONL
    /// log; legacy runs without `log_path` show an explicit empty state.
    pub(crate) view_logs_action: Option<UiAction>,
    pub(crate) position: Vec2,
}

/// Temporary Run Analysis state. The compiled `EngineRunPreview` is the exact
/// request/plan snapshot shown to the user; changing a run control invalidates
/// and replaces it without mutating Global or Song settings.
pub(crate) struct PlanPreviewDraft {
    pub(crate) file_hash: String,
    pub(crate) outputs: app_core::AnalysisOutputSelection,
    pub(crate) outputs_overridden: bool,
    pub(crate) run_override: app_core::AnalysisExperienceOverride,
    /// Studio-owned inheritance projection. Backend presentation remains
    /// limited to the frozen `EngineRunPreview` view fields.
    pub(crate) effective_settings: Option<app_core::EffectiveAnalysisExperience>,
    pub(crate) engine_preview: Result<app_core::EngineRunPreview, String>,
}

impl PlanPreviewDraft {
    pub(crate) fn invalidate(&mut self) {
        if let Ok(preview) = self.engine_preview.as_mut() {
            preview.invalidate();
        }
    }
}

pub(crate) fn rebuild_engine_plan_preview(draft: &mut PlanPreviewDraft, config: &AppConfig) {
    draft.invalidate();
    let song_profile = app_core::get_song_analysis_profile(&draft.file_hash);
    let effective = app_core::resolve_analysis_experience(
        &config.analysis_experience,
        song_profile
            .as_ref()
            .map(|profile| &profile.analysis_experience),
        Some(&draft.run_override),
    );
    if !draft.outputs_overridden {
        draft.outputs =
            app_core::AnalysisOutputSelection::from_target(effective.default_target.value);
    }
    draft.effective_settings = Some(effective);

    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    draft.engine_preview = app_core::preview_engine_run(
        app_core::EngineRunDraft {
            file_hash: draft.file_hash.clone(),
            request_id: format!("studio-{nonce}"),
            lyrics: Default::default(),
            target_override: None,
            requested_outputs: Some(draft.outputs),
            compute_backend: config.compute_backend.clone(),
            model_backend_overrides: config.model_backend_overrides.clone(),
            default_device_class: config.default_device_class.clone(),
            model_device_overrides: config.model_device_overrides.clone(),
            run_override: draft.run_override.clone(),
        },
        &config.analysis_experience,
    );
    match &draft.engine_preview {
        Ok(preview) if preview.ready => {
            bevy::log::info!(
                target: "uta_studio::analysis_preview",
                file_hash = %draft.file_hash,
                request_id = %preview.request_id,
                "Exact Engine plan preview is ready"
            );
        }
        Ok(preview) => {
            bevy::log::warn!(
                target: "uta_studio::analysis_preview",
                file_hash = %draft.file_hash,
                request_id = %preview.request_id,
                blockers = %preview.blockers.join(" | "),
                "Exact Engine plan preview is blocked"
            );
        }
        Err(error) => {
            bevy::log::error!(
                target: "uta_studio::analysis_preview",
                file_hash = %draft.file_hash,
                error = %error,
                "Exact Engine plan preview could not be built"
            );
        }
    }
}

/// §7.5's "View logs" dialog state -- which run-scoped analysis log and
/// node filter should be shown.
pub(crate) struct AnalysisLogViewerState {
    pub(crate) file_hash: String,
    pub(crate) node_id: String,
    pub(crate) follow_tail: bool,
    pub(crate) scroll_offset: f32,
    pub(crate) observed_log_revision: Option<(u64, u128)>,
    pub(crate) log_poll_initialized: bool,
}

pub(crate) fn spawn_analysis_log_viewer(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    state: &AnalysisLogViewerState,
    selected_run_id: Option<i64>,
) {
    let lines =
        app_core::analysis_log_lines(selected_run_id, &state.file_hash, Some(&state.node_id), 240);
    let log_path = app_core::analysis_log_path_for(selected_run_id, &state.file_hash);
    let header = if log_path.is_some() {
        if selected_run_id.is_none() {
            format!("LIVE · following node {}", state.node_id)
        } else {
            format!("RUN LOG · node {}", state.node_id)
        }
    } else if selected_run_id.is_none() {
        format!("WAITING FOR LOG · node {}", state.node_id)
    } else {
        "NO LOG · this legacy run did not record a dedicated analysis log".to_string()
    };
    let console_text = if lines.is_empty() && selected_run_id.is_some() {
        "No log output was recorded for this node.".to_string()
    } else if lines.is_empty() {
        "Waiting for log output…".to_string()
    } else {
        lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Keep a close action both on the backdrop and in the fixed footer. The
    // log itself stays inside Studio; no desktop file-manager window is opened.
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::CloseAnalysisLogViewer),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(92),
    ));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            ZIndex(93),
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(760),
                        max_width: percent(92),
                        height: percent(78),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(20)),
                        row_gap: px(8),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "LOG CONSOLE", 9.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        format!("{} -- {}", state.node_id, state.file_hash),
                        13.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(dialog, font.clone(), header, 10.0, theme.muted_foreground);
                    spawn_text(
                        dialog,
                        font.clone(),
                        if state.follow_tail {
                            "Live tail enabled · scroll up to inspect earlier output"
                        } else {
                            "Live tail paused · scroll to the bottom to resume"
                        },
                        8.0,
                        theme.muted_foreground,
                    );
                    dialog.spawn(Node {
                        height: px(4),
                        ..default()
                    });
                    // Console output is one text stream so appending log records does not
                    // create hundreds of independently laid-out UI nodes.
                    dialog
                        .spawn((
                            AnalysisLogViewerScroll,
                            Node {
                                min_height: px(0),
                                flex_grow: 1.0,
                                padding: UiRect::all(px(12)),
                                border: UiRect::all(px(1)),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            BackgroundColor(Color::srgb(0.025, 0.03, 0.04)),
                            BorderColor::all(theme.border),
                            ScrollPosition(Vec2::new(0.0, state.scroll_offset)),
                        ))
                        .with_children(|scroll| {
                            scroll.spawn((
                                AnalysisLogViewerOutput,
                                Node {
                                    width: percent(100),
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                Text::new(console_text),
                                ui_text_font(font.clone(), 9.0),
                                TextColor(Color::srgb(0.72, 0.86, 0.76)),
                                TextLayout {
                                    linebreak: bevy::text::LineBreak::WordOrCharacter,
                                    ..default()
                                },
                            ));
                        });
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            flex_shrink: 0.0,
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            margin: UiRect::top(px(10)),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Close",
                                UiAction::from(AnalysisCommand::CloseAnalysisLogViewer),
                            );
                        });
                });
        });
}

/// Escape closes the Plan Preview dialog, same idea as
/// `handle_library_search_keyboard`'s Escape-closes-search handling.
pub(crate) fn handle_plan_preview_keyboard(
    keys: Res<ButtonInput<KeyCode>>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !keys.just_pressed(KeyCode::Escape) {
        return;
    }
    if dialogs.plan_preview_draft.is_some() {
        dialogs.plan_preview_draft = None;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    } else if dialogs.analysis_log_viewer.is_some() {
        dialogs.analysis_log_viewer = None;
        invalidated.invalidate(UiDirtyRegion::Analysis);
    }
}

/// The preview contains both stage choices and the resolved model plan. Keep
/// it inside short windows and make the complete contents reachable with an
/// ordinary wheel gesture while the modal is open.
pub(crate) fn handle_plan_preview_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    dialogs: Res<DialogState>,
    mut panels: Query<(&ComputedNode, &mut ScrollPosition), With<PlanPreviewScroll>>,
) {
    if dialogs.plan_preview_draft.is_none() {
        return;
    }
    let Ok((computed, mut position)) = panels.single_mut() else {
        return;
    };
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

/// The modal owns ordinary wheel input, preventing scroll-through to the DAG.
/// Scrolling up pauses tail-follow; reaching the bottom resumes it.
pub(crate) fn handle_analysis_log_viewer_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    mut dialogs: ResMut<DialogState>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<AnalysisLogViewerScroll>>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    let Some(state) = dialogs.analysis_log_viewer.as_mut() else {
        return;
    };
    let Ok((computed, mut position)) = lists.single_mut() else {
        wheel.clear();
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
    state.scroll_offset = position.y;
    let was_following = state.follow_tail;
    state.follow_tail = position.y >= max - 1.0;
    if state.follow_tail != was_following {
        invalidated.invalidate(UiDirtyRegion::Dialog);
    }
}

pub(crate) fn analysis_log_viewer_closed(dialogs: Res<DialogState>) -> bool {
    dialogs.analysis_log_viewer.is_none()
}

pub(crate) fn follow_analysis_log_viewer_tail(
    mut dialogs: ResMut<DialogState>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<AnalysisLogViewerScroll>>,
) {
    let Some(state) = dialogs.analysis_log_viewer.as_mut() else {
        return;
    };
    if !state.follow_tail {
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        return;
    };
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = max;
    state.scroll_offset = max;
}

pub(crate) fn refresh_analysis_log_viewer(
    time: Res<Time>,
    mut timer: ResMut<AnalysisLogRefreshTimer>,
    analysis: Res<AnalysisUiState>,
    mut dialogs: ResMut<DialogState>,
    mut invalidated: ResMut<UiInvalidated>,
) {
    if !timer.0.tick(time.delta()).just_finished() {
        return;
    }
    let Some(state) = dialogs.analysis_log_viewer.as_mut() else {
        return;
    };
    // Historical runs are immutable; only an active run can append output.
    if analysis.selected_analysis_history.is_some() {
        return;
    }
    let revision =
        app_core::analysis_log_path_for(analysis.selected_analysis_history, &state.file_hash)
            .and_then(|path| std::fs::metadata(path).ok())
            .map(|metadata| {
                let modified = metadata
                    .modified()
                    .ok()
                    .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                    .map_or(0, |duration| duration.as_nanos());
                (metadata.len(), modified)
            });
    if state.log_poll_initialized && state.observed_log_revision == revision {
        return;
    }
    state.log_poll_initialized = true;
    state.observed_log_revision = revision;
    invalidated.invalidate(UiDirtyRegion::Dialog);
}

pub(crate) fn exact_preview_allows_queue(
    ready: bool,
    blockers: &[String],
    invalidated: bool,
) -> bool {
    ready && blockers.is_empty() && !invalidated
}

fn plan_preview_status(draft: &PlanPreviewDraft, theme: &StudioTheme) -> (&'static str, Color) {
    match &draft.engine_preview {
        Ok(preview)
            if exact_preview_allows_queue(
                preview.ready,
                &preview.blockers,
                preview.invalidated,
            ) =>
        {
            ("Ready to run", theme.primary)
        }
        Ok(_) => ("Blocked", theme.editor_warning),
        Err(_) => ("Preview unavailable", theme.destructive),
    }
}

fn spawn_plan_preview_card(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    min_width: f32,
    build: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn((
            Node {
                min_width: px(min_width),
                flex_basis: px(min_width),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(8),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.28)),
            BorderColor::all(theme.border.with_alpha(0.46)),
        ))
        .with_children(build);
}

fn spawn_plan_preview_divider(parent: &mut ChildSpawnerCommands, theme: &StudioTheme) {
    parent.spawn((
        Node {
            width: percent(100),
            height: px(1),
            margin: UiRect::axes(px(0), px(4)),
            ..default()
        },
        BackgroundColor(theme.border.with_alpha(0.34)),
    ));
}

fn spawn_plan_preview_readiness(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
) {
    let (label, color) = plan_preview_status(draft, theme);
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(62),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(14), px(11)),
                row_gap: px(5),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                ..default()
            },
            BackgroundColor(color.with_alpha(0.065)),
            BorderColor::all(color.with_alpha(0.28)),
        ))
        .with_children(|status| {
            spawn_text(status, font.clone(), label, 10.0, color);
            match &draft.engine_preview {
                Ok(preview)
                    if exact_preview_allows_queue(
                        preview.ready,
                        &preview.blockers,
                        preview.invalidated,
                    ) => {
                        spawn_wrapped_text(
                            status,
                            font.clone(),
                            "All capabilities required by this exact request are ready under Production policy.",
                            8.5,
                            theme.muted_foreground,
                        );
                    }
                Ok(preview) => {
                    if preview.blockers.is_empty() {
                        spawn_wrapped_text(
                            status,
                            font.clone(),
                            "This exact request is not ready to queue.",
                            8.5,
                            theme.muted_foreground,
                        );
                    } else {
                        for blocker in &preview.blockers {
                            spawn_wrapped_text(
                                status,
                                font.clone(),
                                format!("• {blocker}"),
                                8.5,
                                theme.editor_warning,
                            );
                        }
                    }
                }
                Err(error) => {
                    spawn_wrapped_text(
                        status,
                        font.clone(),
                        error,
                        8.5,
                        theme.destructive,
                    );
                }
            }
        });
}

pub(crate) fn spawn_plan_preview_dialog(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
    notice: Option<&str>,
) {
    let (status_label, status_color) = plan_preview_status(draft, theme);
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::ClosePlanPreview),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(92),
    ));

    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::ZERO,
                ..default()
            },
            ZIndex(93),
            Pickable::IGNORE,
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        position_type: PositionType::Relative,
                        width: Val::Vw(96.0),
                        max_width: px(1520),
                        height: Val::Vh(94.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::clip(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.985)),
                    BorderColor::all(theme.border.with_alpha(0.82)),
                    BoxShadow::new(
                        Color::srgba(0.0, 0.0, 0.0, 0.34),
                        px(0),
                        px(18),
                        px(42),
                        px(-12),
                    ),
                ))
                .with_children(|dialog| {
                    dialog.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            top: px(0),
                            bottom: px(0),
                            width: px(3),
                            ..default()
                        },
                        BackgroundColor(status_color.with_alpha(0.64)),
                        ZIndex(2),
                        Pickable::IGNORE,
                    ));
                    dialog
                        .spawn((
                            Node {
                                width: percent(100),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                flex_wrap: FlexWrap::Wrap,
                                padding: UiRect::axes(px(20), px(14)),
                                column_gap: px(14),
                                row_gap: px(8),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.72)),
                            BorderColor::all(theme.border.with_alpha(0.5)),
                        ))
                        .with_children(|header| {
                            header
                                .spawn(Node {
                                    min_width: px(280),
                                    flex_basis: px(620),
                                    flex_grow: 1.0,
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(2),
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        "RUN ANALYSIS",
                                        7.5,
                                        theme.primary,
                                    );
                                    spawn_text(
                                        copy,
                                        font.clone(),
                                        "Exact Engine plan preview",
                                        17.0,
                                        theme.foreground,
                                    );
                                    spawn_wrapped_text(
                                        copy,
                                        font.clone(),
                                        "Temporary choices below affect this run only. They do not change Global defaults, the Song Profile, or installed resources.",
                                        8.5,
                                        theme.muted_foreground,
                                    );
                                });
                            spawn_settings_badge(
                                header,
                                font.clone(),
                                status_label,
                                status_color,
                            );
                            spawn_text_button(
                                header,
                                font.clone(),
                                theme,
                                "Close",
                                9.0,
                                UiAction::from(AnalysisCommand::ClosePlanPreview),
                            );
                        });

                    dialog
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(0),
                                flex_grow: 1.0,
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(16)),
                                row_gap: px(14),
                                overflow: Overflow::scroll_y(),
                                ..default()
                            },
                            PlanPreviewScroll,
                            ScrollPosition::default(),
                            BackgroundColor(theme.background.with_alpha(0.2)),
                        ))
                        .with_children(|body| {
                            body.spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Stretch,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(12),
                                row_gap: px(12),
                                ..default()
                            })
                            .with_children(|setup| {
                                spawn_plan_preview_card(setup, theme, 340.0, |outputs| {
                                    spawn_text(
                                        outputs,
                                        font.clone(),
                                        format!("OUTPUTS · {}", preview_target_source(draft)),
                                        8.0,
                                        theme.primary,
                                    );
                                    spawn_run_output_sheet(outputs, font.clone(), theme, draft);
                                });
                                spawn_plan_preview_card(setup, theme, 280.0, |quality| {
                                    spawn_text(
                                        quality,
                                        font.clone(),
                                        format!(
                                            "QUALITY · Source: {}",
                                            preview_quality_source(draft)
                                        ),
                                        8.0,
                                        theme.primary,
                                    );
                                    spawn_run_quality_row(quality, font.clone(), theme, draft);
                                });
                            });

                            if let Ok(preview) = &draft.engine_preview {
                                body.spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::FlexStart,
                                    flex_wrap: FlexWrap::Wrap,
                                    column_gap: px(12),
                                    row_gap: px(12),
                                    ..default()
                                })
                                .with_children(|details| {
                                    spawn_plan_preview_card(details, theme, 280.0, |request| {
                                        spawn_preview_request_summary(
                                            request,
                                            font.clone(),
                                            theme,
                                            draft,
                                            preview,
                                        );
                                        spawn_plan_preview_divider(request, theme);
                                        spawn_preview_lyrics_context(
                                            request,
                                            font.clone(),
                                            theme,
                                            preview,
                                        );
                                    });
                                    spawn_plan_preview_card(details, theme, 340.0, |plan| {
                                        spawn_preview_execution_plan(
                                            plan,
                                            font.clone(),
                                            theme,
                                            preview,
                                        );
                                    });
                                    spawn_plan_preview_card(details, theme, 340.0, |resources| {
                                        spawn_preview_resources(
                                            resources,
                                            font.clone(),
                                            theme,
                                            preview,
                                        );
                                        spawn_plan_preview_divider(resources, theme);
                                        spawn_preview_outputs(
                                            resources,
                                            font.clone(),
                                            theme,
                                            preview,
                                        );
                                    });
                                });
                            }

                            spawn_plan_preview_readiness(
                                body,
                                font.clone(),
                                theme,
                                draft,
                            );
                            if let Some(notice) = notice {
                                body.spawn((
                                    Node {
                                        width: percent(100),
                                        padding: UiRect::axes(px(14), px(10)),
                                        border: UiRect::all(px(1)),
                                        border_radius: studio_card_radius(),
                                        ..default()
                                    },
                                    BackgroundColor(theme.destructive.with_alpha(0.055)),
                                    BorderColor::all(theme.destructive.with_alpha(0.28)),
                                ))
                                .with_children(|message| {
                                    spawn_wrapped_text(
                                        message,
                                        font.clone(),
                                        notice,
                                        8.5,
                                        theme.destructive,
                                    );
                                });
                            }
                        });

                    dialog
                        .spawn((
                            Node {
                                width: percent(100),
                                min_height: px(62),
                                flex_shrink: 0.0,
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::SpaceBetween,
                                flex_wrap: FlexWrap::Wrap,
                                padding: UiRect::axes(px(18), px(11)),
                                column_gap: px(14),
                                row_gap: px(8),
                                border: UiRect::top(px(1)),
                                ..default()
                            },
                            BackgroundColor(theme.card.with_alpha(0.82)),
                            BorderColor::all(theme.border.with_alpha(0.5)),
                        ))
                        .with_children(|footer| {
                            footer
                                .spawn(Node {
                                    min_width: px(240),
                                    flex_basis: px(520),
                                    flex_grow: 1.0,
                                    ..default()
                                })
                                .with_children(|copy| {
                                    spawn_wrapped_text(
                                        copy,
                                        font.clone(),
                                        "Read-only preview. No analysis, cache, Active selection, or export is changed here.",
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                });
                            footer
                                .spawn(Node {
                                    align_items: AlignItems::Center,
                                    flex_wrap: FlexWrap::Wrap,
                                    column_gap: px(8),
                                    row_gap: px(8),
                                    ..default()
                                })
                                .with_children(|actions| {
                                    spawn_compact_action_button(
                                        actions,
                                        font.clone(),
                                        theme,
                                        "Models & runtime",
                                        UiAction::from(SettingsCommand::SettingsTab(
                                            SettingsTab::Models,
                                        )),
                                    );
                                    let request_ready = draft.engine_preview.as_ref().is_ok_and(
                                        |preview| {
                                            exact_preview_allows_queue(
                                                preview.ready,
                                                &preview.blockers,
                                                preview.invalidated,
                                            )
                                        },
                                    );
                                    if request_ready {
                                        spawn_compact_primary_action_button(
                                            actions,
                                            font.clone(),
                                            theme,
                                            "Start now",
                                            UiAction::from(AnalysisCommand::QueueExactPreview),
                                        );
                                    } else {
                                        actions
                                            .spawn((
                                                Node {
                                                    min_width: px(116),
                                                    min_height: px(STUDIO_CONTROL_HEIGHT),
                                                    align_items: AlignItems::Center,
                                                    justify_content: JustifyContent::Center,
                                                    padding: UiRect::axes(px(11), px(7)),
                                                    border: UiRect::all(px(1)),
                                                    border_radius: studio_card_radius(),
                                                    ..default()
                                                },
                                                BackgroundColor(
                                                    theme.background.with_alpha(0.28),
                                                ),
                                                BorderColor::all(
                                                    theme.border.with_alpha(0.38),
                                                ),
                                                Pickable::IGNORE,
                                            ))
                                            .with_children(|disabled| {
                                                spawn_text(
                                                    disabled,
                                                    font.clone(),
                                                    status_label,
                                                    9.0,
                                                    theme.muted_foreground,
                                                );
                                            });
                                    }
                                });
                        });
                });
        });
}

pub(crate) fn preview_target_source(draft: &PlanPreviewDraft) -> &'static str {
    if draft.outputs_overridden {
        return "RUN";
    }
    match draft
        .effective_settings
        .as_ref()
        .map(|effective| effective.default_target.source)
    {
        Some(app_core::AnalysisSettingSource::Song) => "SONG",
        Some(app_core::AnalysisSettingSource::Run) => "RUN",
        Some(app_core::AnalysisSettingSource::Global) => "GLOBAL",
        None => "UNAVAILABLE",
    }
}

fn spawn_run_segment(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    active: bool,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_height: px(34),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::axes(px(11), px(7)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if active {
                theme.primary.with_alpha(0.14)
            } else {
                theme.background.with_alpha(0.38)
            }),
            BorderColor::all(if active {
                theme.primary.with_alpha(0.58)
            } else {
                theme.border.with_alpha(0.48)
            }),
            TabIndex(0),
        ))
        .with_children(|button| {
            spawn_text(
                button,
                font,
                label,
                9.0,
                if active {
                    theme.primary
                } else {
                    theme.foreground
                },
            );
        });
}

fn spawn_run_output_sheet(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
) {
    parent
        .spawn(Node {
            width: percent(100),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(6),
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            spawn_run_segment(
                row,
                font.clone(),
                theme,
                "Use profile outputs",
                !draft.outputs_overridden,
                UiAction::from(AnalysisCommand::ResetPlanPreviewOutputs),
            );
            for (output, label) in [
                (
                    app_core::AnalysisOutputKind::CandidateChart,
                    "Candidate chart",
                ),
                (
                    app_core::AnalysisOutputKind::PitchEvidence,
                    "Pitch evidence",
                ),
                (app_core::AnalysisOutputKind::Transcript, "Transcript"),
                (app_core::AnalysisOutputKind::Alignment, "Alignment"),
                (app_core::AnalysisOutputKind::Instrumental, "Instrumental"),
            ] {
                let selected = draft.outputs.contains(output);
                let display = if selected {
                    format!("☑ {label}")
                } else {
                    format!("☐ {label}")
                };
                spawn_run_segment(
                    row,
                    font.clone(),
                    theme,
                    &display,
                    selected,
                    UiAction::from(AnalysisCommand::TogglePlanPreviewOutput(output)),
                );
            }
        });
}

pub(crate) fn preview_quality_source(draft: &PlanPreviewDraft) -> &'static str {
    if draft.run_override.quality_profile.is_some() {
        return "RUN";
    }
    match draft
        .effective_settings
        .as_ref()
        .map(|effective| effective.quality_profile.source)
    {
        Some(app_core::AnalysisSettingSource::Song) => "SONG",
        Some(app_core::AnalysisSettingSource::Run) => "RUN",
        Some(app_core::AnalysisSettingSource::Global) => "GLOBAL",
        None => "UNAVAILABLE",
    }
}

fn spawn_run_quality_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    draft: &PlanPreviewDraft,
) {
    let selected = draft
        .run_override
        .quality_profile
        .or_else(|| {
            draft
                .effective_settings
                .as_ref()
                .map(|effective| effective.quality_profile.value)
        })
        .unwrap_or(app_core::AnalysisQualityProfile::Balanced);
    parent
        .spawn(Node {
            width: percent(100),
            flex_wrap: FlexWrap::Wrap,
            row_gap: px(6),
            column_gap: px(6),
            ..default()
        })
        .with_children(|row| {
            spawn_run_segment(
                row,
                font.clone(),
                theme,
                "Use profile",
                draft.run_override.quality_profile.is_none(),
                UiAction::from(AnalysisCommand::ResetPlanPreviewQuality),
            );
            for (quality, label) in [
                (app_core::AnalysisQualityProfile::Fast, "Fast"),
                (app_core::AnalysisQualityProfile::Balanced, "Balanced"),
                (app_core::AnalysisQualityProfile::Maximum, "Maximum"),
            ] {
                spawn_run_segment(
                    row,
                    font.clone(),
                    theme,
                    label,
                    draft.run_override.quality_profile.is_some() && selected == quality,
                    UiAction::from(AnalysisCommand::SetPlanPreviewQuality(quality)),
                );
            }
        });
}

fn spawn_preview_lyrics_context(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    preview: &app_core::EngineRunPreview,
) {
    let context = &preview.lyrics_context;
    let context_label = match context.mode {
        app_core::StudioLyricsMode::None if context.transcript_requested => {
            "Generated transcript context"
        }
        app_core::StudioLyricsMode::None => "No supplied lyrics context",
        app_core::StudioLyricsMode::Reference => "Reference lyrics context",
        app_core::StudioLyricsMode::Canonical => "Known canonical lyrics context",
    };
    let language = context.language_hint.as_deref().unwrap_or("Automatic");
    spawn_text(parent, font.clone(), "LYRICS CONTEXT", 8.0, theme.primary);
    for line in [
        format!("Mode · {:?} · {context_label}", context.mode),
        format!(
            "Supplied text · {} · Tokens · {}",
            if context.text_supplied { "Yes" } else { "No" },
            if context.tokens_supplied { "Yes" } else { "No" }
        ),
        format!("Language hint · {language}"),
        format!(
            "Transcript requested · {} · Alignment requested · {}",
            if context.transcript_requested {
                "Yes"
            } else {
                "No"
            },
            if context.alignment_requested {
                "Yes"
            } else {
                "No"
            }
        ),
    ] {
        spawn_wrapped_text(parent, font.clone(), line, 9.0, theme.foreground);
    }
}

fn spawn_preview_execution_plan(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    preview: &app_core::EngineRunPreview,
) {
    spawn_text(
        parent,
        font.clone(),
        "ENGINE EXECUTION PLAN",
        8.0,
        theme.primary,
    );
    for node in &preview.engine_plan.execution_nodes {
        let state = if node.required {
            "Required"
        } else {
            "Optional"
        };
        parent
            .spawn((
                Node {
                    width: percent(100),
                    min_height: px(54),
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(11), px(8)),
                    column_gap: px(12),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.3)),
                BorderColor::all(theme.border.with_alpha(0.48)),
            ))
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
                        capability_product_label(node.capability.as_str()),
                        10.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        copy,
                        font.clone(),
                        format!("Capability · {}", node.capability),
                        8.0,
                        theme.muted_foreground,
                    );
                    if !node.depends_on.is_empty() {
                        spawn_wrapped_text(
                            copy,
                            font.clone(),
                            format!("Depends on · {}", node.depends_on.join(", ")),
                            8.0,
                            theme.muted_foreground,
                        );
                    }
                });
                spawn_settings_badge(
                    row,
                    font.clone(),
                    state,
                    if node.required {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    },
                );
            });
    }
}

fn spawn_preview_resources(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    preview: &app_core::EngineRunPreview,
) {
    spawn_text(
        parent,
        font.clone(),
        "RESOLVED RUNTIME RESOURCES",
        8.0,
        theme.primary,
    );
    let fusion_mode = preview
        .engine_plan
        .workflow_execution
        .as_ref()
        .map_or(app_core::FusionModeWireV1::Algorithm, |workflow| {
            workflow.fusion_mode
        });
    let candidate_graph_exercised = preview
        .engine_plan
        .execution_nodes
        .iter()
        .any(|node| node.capability.as_str() == "fusion.candidate_graph");
    let adapter_preview = match (fusion_mode, candidate_graph_exercised) {
        (app_core::FusionModeWireV1::Algorithm, _) => {
            "Fusion Agent Adapter · Not required for Algorithm mode."
        }
        (app_core::FusionModeWireV1::AiJudgment, false) => {
            "Fusion Agent Adapter · Not exercised by this exact run. Preview does not contact the provider."
        }
        (app_core::FusionModeWireV1::AiJudgment, true) => {
            "Fusion Agent Adapter · Preview checks local readiness only; it does not contact the provider."
        }
    };
    spawn_wrapped_text(
        parent,
        font.clone(),
        adapter_preview,
        9.0,
        theme.muted_foreground,
    );
    if preview.engine_plan.resolved_resources.is_empty() {
        spawn_wrapped_text(
            parent,
            font,
            "This request declares no runtime-managed resources.",
            9.0,
            theme.muted_foreground,
        );
        return;
    }
    for resource in &preview.engine_plan.resolved_resources {
        let requirement_role = if resource.requirement.required {
            "Required"
        } else {
            "Optional"
        };
        let adapter_resource = resource.requirement.resource == "tool:fusion_agent_adapter";
        let (status_label, color, details) = if adapter_resource {
            if let Some(error) = &resource.resolution_error {
                (
                    "Status unavailable",
                    theme.editor_warning,
                    format!(
                        "{} · {requirement_role} · {error}",
                        resource.requirement.reason
                    ),
                )
            } else if let Some(status) = &resource.status {
                let missing = status.origin == app_core::ResourceOriginWireV1::Missing
                    || status.reasons.iter().any(|reason| {
                        matches!(
                            reason,
                            app_core::ReadinessReasonWireV1::Absent
                                | app_core::ReadinessReasonWireV1::ExecutableMissing
                        )
                    });
                let reasons = status
                    .reasons
                    .iter()
                    .map(readiness_reason_label)
                    .collect::<Vec<_>>()
                    .join(", ");
                let identity = status.tool_identity.as_deref().unwrap_or("Unavailable");
                let version = status.tool_version.as_deref().unwrap_or("Unavailable");
                let protocol = status
                    .tool_protocol_version
                    .map_or_else(|| "Unavailable".to_string(), |value| value.to_string());
                (
                    if status.usable {
                        "Usable"
                    } else if missing {
                        "Missing"
                    } else {
                        "Unusable"
                    },
                    if status.usable {
                        theme.primary
                    } else {
                        theme.editor_warning
                    },
                    format!(
                        "{} · {} · Identity: {} · Version: {} · Protocol: {}{}",
                        resource.requirement.reason,
                        requirement_role,
                        identity,
                        version,
                        protocol,
                        (!reasons.is_empty())
                            .then(|| format!(" · {reasons}"))
                            .unwrap_or_default()
                    ),
                )
            } else {
                (
                    "Status unavailable",
                    theme.editor_warning,
                    format!(
                        "{} · {requirement_role} · No Runtime Manager status was returned.",
                        resource.requirement.reason
                    ),
                )
            }
        } else if let Some(error) = &resource.resolution_error {
            (
                "Blocked",
                theme.editor_warning,
                format!(
                    "{} · {requirement_role} · {error}",
                    resource.requirement.reason
                ),
            )
        } else if let Some(status) = &resource.status {
            let readiness = if status.usable || status.reasons.is_empty() {
                String::new()
            } else {
                format!(
                    " · Reasons: {}",
                    status
                        .reasons
                        .iter()
                        .map(|reason| format!("{reason:?}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            };
            (
                if status.usable {
                    "Usable"
                } else {
                    "Unavailable"
                },
                if status.usable {
                    theme.primary
                } else {
                    theme.editor_warning
                },
                format!(
                    "{} · {} · Backend: {:?} · Validation: {:?}{}",
                    resource.requirement.reason,
                    requirement_role,
                    status.selected_backend,
                    status.validation_state,
                    readiness
                ),
            )
        } else {
            (
                "No status",
                theme.editor_warning,
                format!(
                    "{} · {requirement_role} · No Runtime Manager status was returned.",
                    resource.requirement.reason
                ),
            )
        };
        parent
            .spawn((
                Node {
                    width: percent(100),
                    min_height: px(50),
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(px(11), px(8)),
                    column_gap: px(12),
                    border: UiRect::all(px(1)),
                    border_radius: BorderRadius::all(px(6)),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.3)),
                BorderColor::all(theme.border.with_alpha(0.48)),
            ))
            .with_children(|row| {
                row.spawn(Node {
                    min_width: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                })
                .with_children(|copy| {
                    spawn_wrapped_text(
                        copy,
                        font.clone(),
                        &resource.requirement.resource,
                        9.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(copy, font.clone(), details, 8.0, theme.muted_foreground);
                });
                spawn_settings_badge(row, font.clone(), status_label, color);
            });
    }
}

fn spawn_preview_outputs(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    preview: &app_core::EngineRunPreview,
) {
    spawn_text(parent, font.clone(), "DECLARED OUTPUTS", 8.0, theme.primary);
    for artifact in &preview.engine_plan.artifact_declarations {
        spawn_wrapped_text(
            parent,
            font.clone(),
            format!(
                "• {} · {}",
                artifact_product_label(&artifact.semantic_type),
                if artifact.required {
                    "Required"
                } else {
                    "Optional"
                }
            ),
            9.0,
            theme.foreground,
        );
    }
}

pub(crate) fn artifact_product_label(artifact: &str) -> &str {
    match artifact {
        "candidate_vocal_chart" => "Candidate VocalChart",
        "pitch_evidence" => "PitchEvidence",
        "singing_analysis" => "Singing analysis",
        "transcript" => "Transcript",
        "alignment" => "Alignment",
        "stem:vocals" => "Vocal stem",
        "stem:instrumental" => "Instrumental stem",
        _ => artifact,
    }
}

pub(crate) fn capability_product_label(capability: &str) -> &'static str {
    match capability {
        "audio.decode" => "Decode & source validation",
        "audio.extract_vocals" => "Vocal extraction",
        "audio.extract_instrumental" => "Instrumental extraction",
        "audio.lead_isolate" => "Lead isolation",
        "speech.transcribe" => "Transcription",
        "speech.align" => "Alignment",
        "pitch.track" => "Continuous pitch",
        "notes.game" => "Note & boundary evidence",
        "fusion.singing" => "Singing fusion",
        "fusion.candidate_graph" => "Candidate graph",
        "finalize.vocal_chart" => "Candidate VocalChart",
        _ => "Analysis capability",
    }
}

fn analysis_node_execution_action(node_id: &str) -> Option<AnalysisNodeMenuAction> {
    node_id
        .starts_with("workflow.")
        .then(|| AnalysisNodeMenuAction {
            label: "Run exact workflow",
            action: UiAction::from(AnalysisCommand::RunWorkflow),
        })
}

pub(crate) fn build_analysis_node_context_menu(
    node_id: &str,
    capability_id: &str,
    label: &str,
    file_hash: &str,
    selected_run_id: Option<i64>,
    position: Vec2,
) -> AnalysisNodeContextMenu {
    AnalysisNodeContextMenu {
        node_id: node_id.to_string(),
        capability_id: capability_id.to_string(),
        label: label.to_string(),
        run_action: selected_run_id
            .is_none()
            .then(|| analysis_node_execution_action(node_id))
            .flatten(),
        compare_node_action: selected_run_id.map(|run_id| {
            UiAction::from(AnalysisCommand::CompareNodeAttemptWithPrevious(
                file_hash.to_string(),
                node_id.to_string(),
                run_id,
            ))
        }),
        view_logs_action: Some(UiAction::from(AnalysisCommand::OpenAnalysisLogViewer(
            file_hash.to_string(),
            node_id.to_string(),
        ))),
        position,
    }
}

pub(crate) struct AnalysisNodeClickTarget<'a> {
    pub(crate) node_id: &'a str,
    pub(crate) label: &'a str,
    pub(crate) file_hash: &'a str,
    pub(crate) capability_id: &'a str,
}

pub(crate) fn clamp_analysis_node_context_position(position: Vec2, viewport: Vec2) -> Vec2 {
    const MENU_WIDTH: f32 = 278.0;
    const MENU_MAX_HEIGHT: f32 = 620.0;
    const EDGE: f32 = 8.0;
    let available_height = (viewport.y * 0.86).min(MENU_MAX_HEIGHT);
    Vec2::new(
        position
            .x
            .clamp(EDGE, (viewport.x - MENU_WIDTH - EDGE).max(EDGE)),
        position
            .y
            .clamp(EDGE, (viewport.y - available_height - EDGE).max(EDGE)),
    )
}

pub(crate) fn open_analysis_node_from_pointer(
    button: PointerButton,
    menu_position: Vec2,
    viewport_size: Vec2,
    target: AnalysisNodeClickTarget,
    analysis: &mut AnalysisUiState,
    dialogs: &mut DialogState,
    invalidated: &mut UiInvalidated,
) {
    let AnalysisNodeClickTarget {
        node_id,
        label,
        file_hash,
        capability_id,
    } = target;
    match button {
        PointerButton::Primary => {
            analysis.selected_analysis_node = (analysis.selected_analysis_node.as_deref()
                != Some(node_id))
            .then(|| node_id.to_string());
            dialogs.analysis_node_context = None;
            invalidated.invalidate(UiDirtyRegion::Analysis);
        }
        PointerButton::Secondary => {
            dialogs.analysis_node_context = Some(build_analysis_node_context_menu(
                node_id,
                capability_id,
                label,
                file_hash,
                analysis.selected_analysis_history,
                clamp_analysis_node_context_position(menu_position, viewport_size),
            ));
            invalidated.invalidate(UiDirtyRegion::Dialog);
        }
        PointerButton::Middle => {}
    }
}

pub(crate) fn spawn_analysis_node_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    context: &AnalysisNodeContextMenu,
) {
    parent.spawn((
        Button,
        UiAction::from(AnalysisCommand::DismissAnalysisNodeContext),
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    // Node context menus live in the window-sized overlay region, so the
    // raw window position can be used directly without rebuilding the DAG.
    let left = context.position.x.max(8.0);
    let top = context.position.y.max(8.0);
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(278),
                max_height: percent(86),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(2),
                align_items: AlignItems::Stretch,
                overflow: Overflow::scroll_y(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                context.label.clone(),
                11.0,
                theme.foreground,
            );
            spawn_text(
                menu,
                font.clone(),
                format!("{} · Node actions", context.node_id),
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                "Inspect view",
                11.0,
                UiAction::from(AnalysisCommand::OpenAnalysisInspect(
                    context.node_id.clone(),
                    context.capability_id.clone(),
                )),
            );
            if let Some(run) = context.run_action.clone() {
                menu.spawn(Node {
                    height: px(5),
                    ..default()
                });
                spawn_text(menu, font.clone(), "EXECUTION", 7.0, theme.muted_foreground);
                spawn_menu_text_button(menu, font.clone(), theme, run.label, 11.0, run.action);
            }
            menu.spawn(Node {
                height: px(5),
                ..default()
            });
            spawn_text(menu, font.clone(), "EVIDENCE", 7.0, theme.muted_foreground);
            if let Some(compare_action) = context.compare_node_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Compare with previous attempt",
                    11.0,
                    compare_action,
                );
            }
            spawn_menu_text_button(
                menu,
                font.clone(),
                theme,
                "Open node documentation",
                11.0,
                UiAction::from(AppCommand::OpenDocumentation(Some(
                    documentation_anchor_for_node(&context.node_id).to_string(),
                ))),
            );
            if let Some(view_logs_action) = context.view_logs_action.clone() {
                spawn_menu_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "View logs",
                    11.0,
                    view_logs_action,
                );
            }
        });
}
