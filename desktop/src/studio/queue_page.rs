use crate::studio::*;

fn queue_status(task: &app_core::AnalysisTask) -> (String, Color, usize) {
    match (&task.status, task.live.as_ref()) {
        (app_core::QueuedStatus::Staged, _) => (
            "READY TO START".to_string(),
            Color::srgb(0.55, 0.68, 0.92),
            0,
        ),
        (app_core::QueuedStatus::Queued, _) => {
            ("WAITING".to_string(), Color::srgb(0.60, 0.63, 0.72), 0)
        }
        (app_core::QueuedStatus::Analyzing(progress), live) => (
            "PROCESSING".to_string(),
            Color::srgb(0.42, 0.76, 0.72),
            live.map_or(*progress, |snapshot| snapshot.overall_progress)
                .clamp(0, 100),
        ),
        (app_core::QueuedStatus::Failed(_), _) => {
            ("FAILED".to_string(), Color::srgb(0.86, 0.40, 0.44), 0)
        }
    }
}

fn queue_detail(task: &app_core::AnalysisTask) -> String {
    match (&task.status, task.live.as_ref()) {
        (app_core::QueuedStatus::Staged, Some(live))
        | (app_core::QueuedStatus::Queued, Some(live)) => live
            .engine
            .as_ref()
            .map(|engine| {
                format!(
                    "Exact request {} · workflow can still be edited before start",
                    engine.request_id
                )
            })
            .unwrap_or_else(|| "Waiting for an exact Engine request".to_string()),
        (app_core::QueuedStatus::Analyzing(_), Some(live)) => {
            let operation = if live.operation.trim().is_empty() {
                live.stage.as_str()
            } else {
                live.operation.as_str()
            };
            format!("{} · {} · {}", operation, live.model, live.device)
        }
        (app_core::QueuedStatus::Analyzing(_), None) => {
            "Engine is processing this song".to_string()
        }
        (app_core::QueuedStatus::Failed(error), _) => error.clone(),
        (_, None) => "Exact request is stored locally".to_string(),
    }
}

fn spawn_queue_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    task: &app_core::AnalysisTask,
    index: usize,
    theme: &StudioTheme,
) {
    let (status, accent, progress) = queue_status(task);
    let movable = matches!(
        task.status,
        app_core::QueuedStatus::Staged | app_core::QueuedStatus::Queued
    );
    let running = matches!(task.status, app_core::QueuedStatus::Analyzing(_));
    let file_hash = task.file_hash.clone();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(112),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(14)),
                row_gap: px(10),
                border: UiRect::all(px(1)),
                border_radius: studio_card_radius(),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.34)),
            BorderColor::all(accent.with_alpha(0.34)),
        ))
        .with_children(|card| {
            if running {
                card.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        bottom: px(0),
                        width: percent(progress as f32),
                        ..default()
                    },
                    BackgroundColor(accent.with_alpha(0.09)),
                    Pickable::IGNORE,
                ));
            }
            card.spawn(Node {
                width: percent(100),
                min_width: px(0),
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: px(12),
                ..default()
            })
            .with_children(|header| {
                spawn_text(
                    header,
                    font.clone(),
                    format!("{:02}", index + 1),
                    10.0,
                    accent,
                );
                header
                    .spawn(Node {
                        min_width: px(0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        ..default()
                    })
                    .with_children(|identity| {
                        spawn_bounded_wrapped_text(
                            identity,
                            font.clone(),
                            &task.title,
                            13.0,
                            theme.foreground,
                        );
                        spawn_bounded_wrapped_text(
                            identity,
                            font.clone(),
                            &task.artist,
                            9.0,
                            theme.muted_foreground,
                        );
                    });
                spawn_text(
                    header,
                    font.clone(),
                    if running {
                        format!("{status} · {progress}%")
                    } else {
                        status
                    },
                    9.0,
                    accent,
                );
            });
            spawn_bounded_wrapped_text(
                card,
                font.clone(),
                queue_detail(task),
                9.0,
                theme.muted_foreground,
            );
            card.spawn(Node {
                width: percent(100),
                flex_direction: FlexDirection::Row,
                flex_wrap: FlexWrap::Wrap,
                justify_content: JustifyContent::FlexEnd,
                column_gap: px(7),
                row_gap: px(7),
                ..default()
            })
            .with_children(|actions| {
                if movable {
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Earlier",
                        UiAction::from(AnalysisCommand::MoveAnalysisQueueItem(
                            file_hash.clone(),
                            true,
                        )),
                    );
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Later",
                        UiAction::from(AnalysisCommand::MoveAnalysisQueueItem(
                            file_hash.clone(),
                            false,
                        )),
                    );
                }
                if matches!(task.status, app_core::QueuedStatus::Staged) {
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Start",
                        UiAction::from(AnalysisCommand::StartQueuedAnalysis(file_hash.clone())),
                    );
                }
                if matches!(task.status, app_core::QueuedStatus::Staged) {
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Edit",
                        UiAction::from(AnalysisCommand::OpenProcessingStudio(file_hash.clone())),
                    );
                }
                if !running {
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Remove",
                        UiAction::from(AnalysisCommand::DeleteAnalysisQueueItem(file_hash.clone())),
                    );
                } else {
                    spawn_compact_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Stop",
                        UiAction::from(AnalysisCommand::CancelAnalysisRun(file_hash.clone())),
                    );
                }
            });
        });
}

pub(crate) fn spawn_analysis_queue_page(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            min_height: px(0),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            overflow: Overflow::scroll_y(),
            padding: UiRect::axes(px(28), px(22)),
            row_gap: px(12),
            ..default()
        })
        .with_children(|list| {
            if session.analysis_tasks.is_empty() {
                spawn_wrapped_text(
                    list,
                    font,
                    "Processing Queue is empty. Add a song from its detail page or preview a workflow run.",
                    11.0,
                    theme.muted_foreground,
                );
                return;
            }
            for (index, task) in session.analysis_tasks.iter().enumerate() {
                spawn_queue_card(list, font.clone(), task, index, theme);
            }
        });
}
