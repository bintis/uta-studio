//! Activity center: analysis session graph, stage nodes, and history.

use crate::studio::*;

#[derive(Resource)]
pub(crate) struct AnalysisRefreshTimer(pub(crate) Timer);

#[derive(Component)]
pub(crate) struct AnalysisGraphViewport;

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

    pub(crate) fn left_port(self) -> Vec2 {
        Vec2::new(self.x, self.y + self.height / 2.0)
    }

    pub(crate) fn right_port(self) -> Vec2 {
        Vec2::new(self.x + self.width, self.y + self.height / 2.0)
    }
}

#[derive(Clone, Copy)]
pub(crate) enum AnalysisGraphStageState {
    Waiting,
    Running(usize),
    Complete,
}

pub(crate) fn spawn_activity_center(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    parent.spawn((
        Button,
        UiAction::CloseActivity,
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
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(420),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(20)),
                row_gap: px(12),
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
                        UiAction::CloseActivity,
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
                format!("JOBS  ·  {}", session.analysis_tasks.len()),
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
                for task in session.analysis_tasks.iter().take(10) {
                    let (status, progress, failed) = analysis_status_copy(&task.status);
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
                            });
                            if let Some(live) = task.live.as_ref() {
                                spawn_text(
                                    card,
                                    font.clone(),
                                    format!("{} · {}%", live.operation, live.stage_progress),
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
                UiAction::SetLibraryView(LibraryView::Queue),
            );
        });
}

pub(crate) fn analysis_status_copy(
    status: &app_core::QueuedStatus,
) -> (String, Option<usize>, bool) {
    match status {
        app_core::QueuedStatus::Queued => ("Queued".to_string(), None, false),
        app_core::QueuedStatus::Analyzing(progress) => {
            (format!("Analyzing · {progress}%"), Some(*progress), false)
        }
        app_core::QueuedStatus::Failed(message) => (
            if message.trim().is_empty() {
                "Failed".to_string()
            } else {
                format!("Failed · {message}")
            },
            None,
            true,
        ),
    }
}

pub(crate) fn analysis_stage_index(stage: &str) -> usize {
    match stage {
        "preparing" | "key_detection" => 0,
        "separation" => 1,
        "pitch" => 2,
        "audio_preprocessing" => 3,
        "transcription" => 4,
        "alignment" => 5,
        "finalizing" | "complete" => 6,
        _ => 0,
    }
}

pub(crate) fn analysis_stage_matches(route_stage: &str, selected_stage: &str) -> bool {
    route_stage == selected_stage
        || (selected_stage == "preparing" && route_stage == "key_detection")
        || (selected_stage == "finalizing" && route_stage == "complete")
}

pub(crate) fn analysis_stage_details(
    stage: &str,
) -> (&'static str, &'static str, &'static str, &'static str) {
    match stage {
        "preparing" => (
            "Prepare",
            "Validates the source, resolves analysis settings, and detects musical context before model execution.",
            "Authorized source media and analysis profile",
            "Validated audio, runtime plan, tempo and key context",
        ),
        "separation" => (
            "Separate",
            "Extracts a vocal-focused stem while preserving the original source unchanged.",
            "Validated source audio",
            "Lossless vocal and instrumental analysis stems",
        ),
        "pitch" => (
            "Pitch",
            "Tracks the sung fundamental frequency and converts the contour into editable note guidance.",
            "Separated vocal stem",
            "Pitch contour and note candidates",
        ),
        "audio_preprocessing" => (
            "Preprocess",
            "Normalizes the analysis signal and prepares model-specific audio windows without rewriting source media.",
            "Vocal analysis stem",
            "Model-ready audio windows and vocal regions",
        ),
        "transcription" => (
            "Transcribe",
            "Recognizes lyric text and produces the timing evidence supported by the selected speech model.",
            "Preprocessed vocal regions and language preference",
            "Recognized lyric tokens and provisional timestamps",
        ),
        "alignment" => (
            "Align",
            "Refines recognized or supplied lyrics against the audio into editor-ready character and word timing.",
            "Lyrics, provisional timestamps, and vocal audio",
            "Character and word-level aligned lyrics",
        ),
        "finalizing" => (
            "Finalize",
            "Validates and commits generated analysis assets before the song becomes available for authoring.",
            "Aligned lyrics, pitch data, metadata, and stems",
            "Cached chart analysis and library metadata",
        ),
        _ => (
            "Analysis step",
            "Executes one stage of the configured analysis pipeline.",
            "Previous stage output",
            "Next stage input",
        ),
    }
}

pub(crate) fn spawn_analysis_session_overview(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSession,
    theme: &StudioTheme,
) {
    let active_task = session
        .analysis_tasks
        .iter()
        .find(|task| matches!(task.status, app_core::QueuedStatus::Analyzing(_)))
        .or_else(|| {
            session
                .analysis_tasks
                .iter()
                .find(|task| matches!(task.status, app_core::QueuedStatus::Queued))
        });
    let history = session
        .selected_analysis_history
        .and_then(|id| {
            session
                .analysis_history
                .iter()
                .find(|history| history.id == id)
        })
        .or_else(|| {
            active_task
                .is_none()
                .then(|| session.analysis_history.first())
                .flatten()
        });
    let history_task = history.map(|history| app_core::AnalysisTask {
        file_hash: history.file_hash.clone(),
        title: history.title.clone(),
        artist: history.artist.clone(),
        status: app_core::QueuedStatus::Analyzing(if history.status == "completed" {
            100
        } else {
            0
        }),
        live: Some(history.snapshot.clone()),
    });
    let Some(task) = history_task.as_ref().or(active_task) else {
        return;
    };
    let viewing_history = history_task.is_some();

    let progress = match &task.status {
        app_core::QueuedStatus::Analyzing(progress) => (*progress).clamp(0, 100),
        _ => 0,
    };
    let stage = task
        .live
        .as_ref()
        .map(|live| live.stage.as_str())
        .unwrap_or("preparing");
    let stage_index = analysis_stage_index(stage);
    let operation = task
        .live
        .as_ref()
        .map(|live| live.operation.as_str())
        .unwrap_or("Waiting for the analysis runtime");
    let detail = task
        .live
        .as_ref()
        .map(|live| live.detail.as_str())
        .unwrap_or("The task is queued and will start when the current analysis completes.");
    let selected_stage = session.selected_analysis_stage.as_deref().unwrap_or(stage);
    let selected_stage_index = analysis_stage_index(selected_stage);
    let selected_route = task.live.as_ref().and_then(|live| {
        live.stage_routes
            .iter()
            .rev()
            .find(|route| analysis_stage_matches(&route.stage, selected_stage))
    });
    let selected_is_current = analysis_stage_matches(stage, selected_stage);
    let selected_progress = selected_route
        .map(|route| route.stage_progress.clamp(0, 100))
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.stage_progress.clamp(0, 100))
                    .unwrap_or(0)
            })
        })
        .unwrap_or_else(|| {
            if selected_stage_index < stage_index {
                100
            } else {
                0
            }
        });
    let selected_trace_missing = selected_route.is_none() && selected_progress >= 100;
    let selected_pending_copy = if selected_trace_missing {
        "Not recorded in this analysis session"
    } else {
        "Pending"
    };
    let (selected_label, selected_purpose, selected_input, selected_output) =
        analysis_stage_details(selected_stage);
    let selected_status = if selected_progress >= 100 {
        "COMPLETE"
    } else if selected_is_current {
        "RUNNING"
    } else if selected_stage_index < stage_index {
        "COMPLETE"
    } else {
        "WAITING"
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
    let selected_requested_device = selected_route
        .map(|route| route.requested_device.as_str())
        .or_else(|| {
            selected_is_current.then(|| {
                task.live
                    .as_ref()
                    .map(|live| live.requested_device.as_str())
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
    let history_error = history.and_then(|history| history.error_message.as_deref());

    parent
        .spawn((
            Node {
                width: percent(100),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(30), px(26)),
                row_gap: px(16),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|session_card| {
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    column_gap: px(20),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(10),
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
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
                                if viewing_history {
                                    "ANALYSIS SESSION HISTORY"
                                } else {
                                    "LIVE ANALYSIS SESSION"
                                },
                                9.0,
                                theme.primary,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                task.title.clone(),
                                25.0,
                                theme.foreground,
                            );
                            spawn_text(
                                copy,
                                font.clone(),
                                task.artist.clone(),
                                11.0,
                                theme.muted_foreground,
                            );
                        });
                    if viewing_history && active_task.is_some() {
                        spawn_text_button(
                            header,
                            font.clone(),
                            theme,
                            "View live",
                            9.0,
                            UiAction::SelectAnalysisHistory(None),
                        );
                    }
                    spawn_text(
                        header,
                        font.clone(),
                        format!("{progress:02}%"),
                        30.0,
                        theme.foreground,
                    );
                });

            session_card
                .spawn(Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    row_gap: px(4),
                    ..default()
                })
                .with_children(|current| {
                    spawn_text(
                        current,
                        font.clone(),
                        "CURRENT OPERATION",
                        8.0,
                        theme.muted_foreground,
                    );
                    spawn_text(current, font.clone(), operation, 18.0, theme.foreground);
                    spawn_wrapped_text(current, font.clone(), detail, 10.0, theme.muted_foreground);
                    if let Some(live) = task.live.as_ref() {
                        if let Some(fallback_from) = live.fallback_from.as_deref() {
                            current
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(10),
                                    margin: UiRect::top(px(8)),
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
                                    route
                                        .spawn((
                                            Node {
                                                min_width: px(58),
                                                padding: UiRect::axes(px(10), px(6)),
                                                justify_content: JustifyContent::Center,
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(theme.editor_warning.with_alpha(0.08)),
                                            BorderColor::all(theme.editor_warning.with_alpha(0.48)),
                                        ))
                                        .with_children(|source| {
                                            spawn_text(
                                                source,
                                                font.clone(),
                                                fallback_from.to_ascii_uppercase(),
                                                9.0,
                                                theme.editor_warning,
                                            );
                                        });
                                    route.spawn((
                                        Node {
                                            width: px(34),
                                            height: px(2),
                                            ..default()
                                        },
                                        BackgroundColor(theme.editor_warning.with_alpha(0.68)),
                                    ));
                                    spawn_text(
                                        route,
                                        font.clone(),
                                        ">",
                                        10.0,
                                        theme.editor_warning,
                                    );
                                    route
                                        .spawn((
                                            Node {
                                                min_width: px(58),
                                                padding: UiRect::axes(px(10), px(6)),
                                                justify_content: JustifyContent::Center,
                                                border: UiRect::all(px(1)),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(theme.pitch_contour.with_alpha(0.09)),
                                            BorderColor::all(theme.pitch_contour.with_alpha(0.52)),
                                        ))
                                        .with_children(|destination| {
                                            spawn_text(
                                                destination,
                                                font.clone(),
                                                live.device.to_ascii_uppercase(),
                                                9.0,
                                                theme.pitch_contour,
                                            );
                                        });
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
                    }
                });

            session_card
                .spawn((
                    Node {
                        width: percent(100),
                        height: px(5),
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
                        BackgroundColor(theme.primary),
                    ));
                });

            let active_stage_progress = task
                .live
                .as_ref()
                .map(|live| live.stage_progress.clamp(0, 100))
                .unwrap_or(0);
            session_card
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    column_gap: px(10),
                    ..default()
                })
                .with_children(|heading| {
                    spawn_text(
                        heading,
                        font.clone(),
                        "DATA DEPENDENCY GRAPH",
                        8.0,
                        theme.primary,
                    );
                    spawn_text(
                        heading,
                        font.clone(),
                        "Stages consume the connected artifacts · Drag canvas or Shift + wheel to pan",
                        8.0,
                        theme.muted_foreground,
                    );
                });

            let prepare = AnalysisGraphBox::new(30.0, 210.0, 150.0, 92.0);
            let separate = AnalysisGraphBox::new(220.0, 210.0, 150.0, 92.0);
            let vocal = AnalysisGraphBox::new(410.0, 219.0, 150.0, 74.0);
            let instrumental = AnalysisGraphBox::new(410.0, 350.0, 150.0, 74.0);
            let pitch = AnalysisGraphBox::new(620.0, 35.0, 150.0, 92.0);
            let note_guide = AnalysisGraphBox::new(810.0, 44.0, 150.0, 74.0);
            let preprocess = AnalysisGraphBox::new(620.0, 210.0, 150.0, 92.0);
            let transcribe = AnalysisGraphBox::new(810.0, 210.0, 150.0, 92.0);
            let align = AnalysisGraphBox::new(1000.0, 210.0, 150.0, 92.0);
            let timed_lyrics = AnalysisGraphBox::new(1190.0, 219.0, 150.0, 74.0);
            let finalize = AnalysisGraphBox::new(1380.0, 210.0, 150.0, 92.0);
            let chart = AnalysisGraphBox::new(1570.0, 219.0, 150.0, 74.0);
            let utz = AnalysisGraphBox::new(1760.0, 105.0, 130.0, 74.0);
            let ultrastar = AnalysisGraphBox::new(1760.0, 330.0, 130.0, 74.0);

            let stage_complete = |index: usize| {
                index < stage_index
                    || (index == stage_index && active_stage_progress >= 100)
                    || progress >= 100
            };
            let prepare_ready = stage_complete(0);
            let stems_ready = stage_complete(1);
            let pitch_ready = stage_complete(2);
            let preprocess_ready = stage_complete(3);
            let transcript_ready = stage_complete(4);
            let lyrics_ready = stage_complete(5);
            let chart_ready = stage_complete(6);

            session_card
                .spawn((
                    AnalysisGraphViewport,
                    ScrollPosition(Vec2::new(session.analysis_graph_scroll_offset, 0.0)),
                    Node {
                        width: percent(100),
                        height: px(445),
                        overflow: Overflow::scroll_x(),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.34)),
                    BorderColor::all(theme.border.with_alpha(0.5)),
                ))
                .with_children(|viewport| {
                    viewport
                        .spawn(Node {
                            position_type: PositionType::Relative,
                            width: px(1930),
                            height: px(430),
                            flex_shrink: 0.0,
                            ..default()
                        })
                        .with_children(|graph| {
                            spawn_analysis_graph_lane(
                                graph,
                                font.clone(),
                                theme,
                                590.0,
                                16.0,
                                780.0,
                                132.0,
                                "MELODY · PITCH CONTOUR AND NOTE GUIDE",
                            );
                            spawn_analysis_graph_lane(
                                graph,
                                font.clone(),
                                theme,
                                390.0,
                                184.0,
                                980.0,
                                142.0,
                                "LYRICS · VOCAL PREPROCESSING AND TIMING",
                            );
                            spawn_analysis_graph_lane(
                                graph,
                                font.clone(),
                                theme,
                                390.0,
                                334.0,
                                980.0,
                                90.0,
                                "ACCOMPANIMENT · LOSSLESS INSTRUMENTAL STEM",
                            );
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[prepare.right_port(), separate.left_port()],
                                prepare_ready,
                            );
                            for target in [vocal, instrumental] {
                                let from = separate.right_port();
                                let to = target.left_port();
                                spawn_analysis_graph_path(
                                    graph,
                                    theme,
                                    &[from, Vec2::new(390.0, from.y), Vec2::new(390.0, to.y), to],
                                    stems_ready,
                                );
                            }
                            for target in [pitch, preprocess] {
                                let from = vocal.right_port();
                                let to = target.left_port();
                                spawn_analysis_graph_path(
                                    graph,
                                    theme,
                                    &[from, Vec2::new(590.0, from.y), Vec2::new(590.0, to.y), to],
                                    stems_ready,
                                );
                            }
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[pitch.right_port(), note_guide.left_port()],
                                pitch_ready,
                            );
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[preprocess.right_port(), transcribe.left_port()],
                                preprocess_ready,
                            );
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[transcribe.right_port(), align.left_port()],
                                transcript_ready,
                            );
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[align.right_port(), timed_lyrics.left_port()],
                                lyrics_ready,
                            );
                            let final_port = finalize.left_port();
                            let note_port = note_guide.right_port();
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[
                                    note_port,
                                    Vec2::new(1160.0, note_port.y),
                                    Vec2::new(1160.0, final_port.y),
                                    final_port,
                                ],
                                pitch_ready,
                            );
                            let lyric_port = timed_lyrics.right_port();
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[
                                    lyric_port,
                                    Vec2::new(1360.0, lyric_port.y),
                                    Vec2::new(1360.0, final_port.y),
                                    final_port,
                                ],
                                lyrics_ready,
                            );
                            let instrumental_port = instrumental.right_port();
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[
                                    instrumental_port,
                                    Vec2::new(1350.0, instrumental_port.y),
                                    Vec2::new(1350.0, final_port.y),
                                    final_port,
                                ],
                                stems_ready,
                            );
                            spawn_analysis_graph_path(
                                graph,
                                theme,
                                &[finalize.right_port(), chart.left_port()],
                                chart_ready,
                            );
                            for target in [utz, ultrastar] {
                                let from = chart.right_port();
                                let to = target.left_port();
                                spawn_analysis_graph_path(
                                    graph,
                                    theme,
                                    &[from, Vec2::new(1740.0, from.y), Vec2::new(1740.0, to.y), to],
                                    chart_ready,
                                );
                            }

                            for (index, stage_id, label, bounds) in [
                                (0, "preparing", "Prepare", prepare),
                                (1, "separation", "Separate", separate),
                                (2, "pitch", "Pitch", pitch),
                                (3, "audio_preprocessing", "Preprocess", preprocess),
                                (4, "transcription", "Transcribe", transcribe),
                                (5, "alignment", "Align", align),
                                (6, "finalizing", "Finalize", finalize),
                            ] {
                                let state = if stage_complete(index) {
                                    AnalysisGraphStageState::Complete
                                } else if index == stage_index {
                                    AnalysisGraphStageState::Running(active_stage_progress)
                                } else {
                                    AnalysisGraphStageState::Waiting
                                };
                                let (route, warning) = analysis_graph_route_summary(
                                    task,
                                    stage_id,
                                    stage_complete(index),
                                );
                                spawn_analysis_stage_node(
                                    graph,
                                    font.clone(),
                                    theme,
                                    bounds,
                                    index,
                                    stage_id,
                                    label,
                                    state,
                                    selected_stage == stage_id,
                                    &route,
                                    warning,
                                );
                            }
                            for (bounds, title, detail, ready) in [
                                (vocal, "Vocal stem", "vocals.flac · lossless", stems_ready),
                                (
                                    instrumental,
                                    "Instrumental stem",
                                    "instrumental.flac · lossless",
                                    stems_ready,
                                ),
                                (
                                    note_guide,
                                    "Note guide",
                                    "Pitch contour + notes",
                                    pitch_ready,
                                ),
                                (
                                    timed_lyrics,
                                    "Timed lyrics",
                                    "Aligned lyric timing",
                                    lyrics_ready,
                                ),
                                (
                                    chart,
                                    "Editable chart",
                                    "Authoring-ready assets",
                                    chart_ready,
                                ),
                            ] {
                                spawn_analysis_artifact_node(
                                    graph,
                                    font.clone(),
                                    theme,
                                    bounds,
                                    "ARTIFACT",
                                    title,
                                    detail,
                                    ready,
                                    false,
                                );
                            }
                            for (bounds, title, detail) in [
                                (utz, "UTZ package", "Explicit export target"),
                                (ultrastar, "UltraStar chart", "Explicit export target"),
                            ] {
                                spawn_analysis_artifact_node(
                                    graph,
                                    font.clone(),
                                    theme,
                                    bounds,
                                    "OUTPUT",
                                    title,
                                    detail,
                                    chart_ready,
                                    true,
                                );
                            }
                        });
                })
                .observe(
                    |mut drag: On<Pointer<Drag>>,
                     ui_scale: Res<UiScale>,
                     mut session: ResMut<StudioSession>,
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
                        position.x = (position.x - delta.x)
                            .clamp(0.0, (content.x - size.x).max(0.0));
                        session.analysis_graph_scroll_offset = position.x;
                    },
                );

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
                                    selected_stage_index + 1,
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
                                format!("{selected_status} · {selected_progress}%"),
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
                    inspector
                        .spawn(Node {
                            width: percent(100),
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: px(9),
                            row_gap: px(9),
                            ..default()
                        })
                        .with_children(|facts| {
                            for (label, value) in [
                                ("IMPLEMENTATION", selected_implementation),
                                ("MODEL / ALGORITHM", selected_model),
                                ("REQUESTED DEVICE", selected_requested_device),
                                ("ACTUAL DEVICE", selected_actual_device),
                                ("INPUT", selected_input),
                                ("OUTPUT", selected_output),
                            ] {
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
                                            theme.foreground,
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
                    if let Some(error) = history_error {
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
        });
}

pub(crate) fn analysis_graph_route_summary(
    task: &app_core::AnalysisTask,
    stage_id: &str,
    completed: bool,
) -> (String, bool) {
    let route = task.live.as_ref().and_then(|live| {
        live.stage_routes
            .iter()
            .rev()
            .find(|route| analysis_stage_matches(&route.stage, stage_id))
    });
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
    let model = (!route.model.trim().is_empty())
        .then(|| route.model.as_str())
        .unwrap_or("default");
    (format!("{implementation} · {model}"), warning)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_stage_node(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    bounds: AnalysisGraphBox,
    index: usize,
    stage_id: &str,
    label: &str,
    state: AnalysisGraphStageState,
    selected: bool,
    route: &str,
    warning: bool,
) {
    let (status, progress, status_color) = match state {
        AnalysisGraphStageState::Waiting => ("WAITING", 0, theme.muted_foreground),
        AnalysisGraphStageState::Running(progress) => ("RUNNING", progress, theme.primary),
        AnalysisGraphStageState::Complete => ("COMPLETE", 100, theme.pitch_contour),
    };
    let running = matches!(state, AnalysisGraphStageState::Running(_));
    let complete = matches!(state, AnalysisGraphStageState::Complete);
    parent
        .spawn((
            Button,
            UiAction::SelectAnalysisStage(stage_id.to_string()),
            Node {
                position_type: PositionType::Absolute,
                left: px(bounds.x),
                top: px(bounds.y),
                width: px(bounds.width),
                height: px(bounds.height),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(7),
                overflow: Overflow::clip(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(if running {
                theme.primary.with_alpha(0.16)
            } else if selected {
                theme.card.with_alpha(0.9)
            } else {
                theme.card.with_alpha(0.68)
            }),
            BorderColor::all(if selected {
                theme.primary.with_alpha(0.92)
            } else if running {
                theme.primary.with_alpha(0.62)
            } else if complete {
                theme.pitch_contour.with_alpha(0.42)
            } else {
                theme.border.with_alpha(0.68)
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
        });
}

#[allow(clippy::too_many_arguments)]
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
) {
    let accent = if output {
        theme.primary
    } else {
        theme.pitch_contour
    };
    parent
        .spawn((
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
        ))
        .with_children(|node| {
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
    for pair in points.windows(2) {
        let from = pair[0];
        let to = pair[1];
        let horizontal = (from.y - to.y).abs() <= 0.5;
        let left = from.x.min(to.x);
        let top = from.y.min(to.y);
        parent.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(if horizontal {
                    (to.x - from.x).abs().max(2.0)
                } else {
                    2.0
                }),
                height: px(if horizontal {
                    2.0
                } else {
                    (to.y - from.y).abs().max(2.0)
                }),
                border_radius: BorderRadius::MAX,
                ..default()
            },
            BackgroundColor(color),
            ZIndex(0),
            Pickable::IGNORE,
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_analysis_graph_lane(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    label: &str,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(x),
                top: px(y),
                width: px(width),
                height: px(height),
                padding: UiRect::axes(px(12), px(8)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(10)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.14)),
            BorderColor::all(theme.border.with_alpha(0.22)),
            ZIndex(0),
            Pickable::IGNORE,
        ))
        .with_children(|lane| {
            spawn_text(
                lane,
                font,
                label,
                6.5,
                theme.muted_foreground.with_alpha(0.62),
            );
        });
}

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
    let shift = keys.any_pressed([KeyCode::ShiftLeft, KeyCode::ShiftRight]);
    let delta = wheel
        .read()
        .map(|event| {
            let scale = match event.unit {
                bevy::input::mouse::MouseScrollUnit::Line => 34.0,
                bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
            };
            if event.x.abs() > f32::EPSILON {
                -event.x * scale
            } else if shift {
                -event.y * scale
            } else {
                0.0
            }
        })
        .sum::<f32>();
    if delta.abs() <= f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    position.x = (position.x + delta).clamp(0.0, (content.x - size.x).max(0.0));
    session.analysis_graph_scroll_offset = position.x;
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
