use super::*;
use crate::studio::*;

pub(crate) const ANALYSIS_MODEL_PANEL_WIDTH: f32 = 338.0;

#[derive(Component)]
pub(crate) struct AnalysisModelPanelScroll;

pub(crate) fn current_analysis_model_panel_context(
    session: &StudioSessionView<'_>,
) -> (String, String) {
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
    let live = history
        .map(|history| &history.snapshot)
        .or_else(|| active_task.and_then(|task| task.live.as_ref()));
    let stage = live.map(|live| live.stage.as_str()).unwrap_or("preparing");
    let selected_stage = session.selected_analysis_stage.as_deref().unwrap_or(stage);
    let selected_stage_index = analysis_stage_index(selected_stage);
    let (fallback_node_id, _) = stage_primary_node_and_artifact(selected_stage_index);
    let selected_node_id = session
        .selected_analysis_node
        .as_deref()
        .filter(|node_id| analysis_node_stage_index(node_id) == Some(selected_stage_index))
        .unwrap_or(fallback_node_id);
    let status = if let Some(history) = history {
        if history.status == "completed" {
            "DONE"
        } else if history.status == "failed" {
            "FAILED"
        } else {
            "HISTORY"
        }
    } else if let Some(task) = active_task {
        match task.status {
            app_core::QueuedStatus::Analyzing(_) => "RUNNING",
            app_core::QueuedStatus::Queued => "QUEUED",
            app_core::QueuedStatus::Failed(_) => "FAILED",
        }
    } else {
        "READY"
    };
    (selected_node_id.to_string(), status.to_string())
}

fn category_label(category: AnalysisModelCategory) -> &'static str {
    match category {
        AnalysisModelCategory::Bgm => "BGM",
        AnalysisModelCategory::Vocals => "人声",
        AnalysisModelCategory::Lyrics => "歌词",
        AnalysisModelCategory::Pitch => "音高",
    }
}

fn spawn_segment_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    selected: bool,
    action: UiAction,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(0),
                height: px(30),
                flex_grow: 1.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(px(8)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(if selected {
                theme.primary.with_alpha(0.78)
            } else {
                theme.background.with_alpha(0.52)
            }),
            BorderColor::all(if selected {
                theme.primary.with_alpha(0.86)
            } else {
                theme.border.with_alpha(0.45)
            }),
        ))
        .with_children(|button| {
            spawn_text(
                button,
                font,
                label,
                9.0,
                if selected {
                    theme.background
                } else {
                    theme.muted_foreground
                },
            );
        });
}

fn spawn_model_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    icon: &'static str,
    label: &'static str,
    kind: SettingsSelectKind,
) {
    let open = session.open_settings_select == Some(kind);
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(44),
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                padding: UiRect::axes(px(9), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.40)),
            BorderColor::all(if open {
                theme.primary.with_alpha(0.70)
            } else {
                theme.border.with_alpha(0.42)
            }),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: percent(100),
                min_height: px(30),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|row| {
                spawn_text(row, font.clone(), icon, 12.0, theme.primary);
                spawn_text(row, font.clone(), label, 9.0, theme.foreground);
                row.spawn(Node {
                    min_width: px(8),
                    flex_grow: 1.0,
                    ..default()
                });
                let current = settings_select_value(kind, session.config);
                row.spawn((
                    Button,
                    UiAction::from(SettingsCommand::OpenSettingsSelect(kind)),
                    Node {
                        width: px(158),
                        height: px(30),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::horizontal(px(9)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.72)),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.72)
                    } else {
                        theme.border.with_alpha(0.58)
                    }),
                ))
                .with_children(|select| {
                    spawn_text(
                        select,
                        font.clone(),
                        settings_select_label(kind, current),
                        8.0,
                        theme.foreground,
                    );
                    spawn_text(select, font.clone(), "v", 8.0, theme.muted_foreground);
                });
            });
            if open {
                for (value, option_label) in settings_select_options(
                    kind,
                    session.config.compute_backend.as_deref() == Some("intel"),
                ) {
                    card.spawn((
                        Button,
                        UiAction::from(SettingsCommand::SelectSettingsValue(
                            kind,
                            (*value).to_string(),
                        )),
                        Node {
                            width: percent(100),
                            min_height: px(28),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(9)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(if settings_select_value(kind, session.config) == *value {
                            theme.primary.with_alpha(0.18)
                        } else {
                            theme.card.with_alpha(0.30)
                        }),
                    ))
                    .with_children(|option| {
                        spawn_text(option, font.clone(), *option_label, 8.0, theme.foreground);
                    });
                }
            }
        });
}

pub(crate) fn spawn_analysis_model_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    current_node_id: &str,
    current_status: &str,
) {
    parent
        .spawn((
            AnalysisModelPanelScroll,
            ScrollPosition::default(),
            Node {
                position_type: PositionType::Absolute,
                right: px(0),
                top: px(0),
                bottom: px(0),
                width: px(ANALYSIS_MODEL_PANEL_WIDTH),
                flex_direction: FlexDirection::Column,
                row_gap: px(9),
                padding: UiRect::all(px(12)),
                overflow: Overflow::scroll_y(),
                border: UiRect::left(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.97)),
            BorderColor::all(theme.border.with_alpha(0.58)),
            ZIndex(20),
        ))
        .with_children(|panel| {
            panel
                .spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::FlexStart,
                    justify_content: JustifyContent::SpaceBetween,
                    ..default()
                })
                .with_children(|header| {
                    header
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|copy| {
                            spawn_text(copy, font.clone(), "AI 模型", 15.0, theme.foreground);
                            spawn_text(
                                copy,
                                font.clone(),
                                "快速为节点选择模型",
                                8.0,
                                theme.muted_foreground,
                            );
                        });
                    spawn_text_button(
                        header,
                        font.clone(),
                        theme,
                        "×",
                        13.0,
                        UiAction::from(AnalysisCommand::CloseAnalysisModelPanel),
                    );
                });

            panel
                .spawn((
                    Node {
                        width: percent(100),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(5),
                        padding: UiRect::all(px(10)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(7)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.42)),
                    BorderColor::all(theme.border.with_alpha(0.48)),
                ))
                .with_children(|current| {
                    spawn_text(current, font.clone(), "当前节点", 8.0, theme.muted_foreground);
                    current
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::SpaceBetween,
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_text(
                                row,
                                font.clone(),
                                analysis_graph_node_label(current_node_id, current_node_id),
                                11.0,
                                theme.foreground,
                            );
                            spawn_text(row, font.clone(), current_status, 8.0, theme.primary);
                        });
                    if let Some(model) =
                        analysis_graph_configured_model_tag(current_node_id, session.config)
                    {
                        spawn_text(
                            current,
                            font.clone(),
                            model,
                            8.0,
                            theme.muted_foreground,
                        );
                    }
                });

            panel
                .spawn(Node {
                    width: percent(100),
                    column_gap: px(3),
                    ..default()
                })
                .with_children(|tabs| {
                    for category in [
                        AnalysisModelCategory::Bgm,
                        AnalysisModelCategory::Vocals,
                        AnalysisModelCategory::Lyrics,
                        AnalysisModelCategory::Pitch,
                    ] {
                        spawn_segment_button(
                            tabs,
                            font.clone(),
                            theme,
                            category_label(category),
                            session.analysis_model_category == category,
                            UiAction::from(AnalysisCommand::SetAnalysisModelCategory(category)),
                        );
                    }
                });

            let selected = session.analysis_model_category;
            for (category, icon, label, kind) in [
                (
                    AnalysisModelCategory::Bgm,
                    "♫",
                    "BGM separation",
                    SettingsSelectKind::AudioAccompanimentModel,
                ),
                (
                    AnalysisModelCategory::Bgm,
                    "1",
                    "Post-processing 1",
                    SettingsSelectKind::AudioBgmPostprocess1,
                ),
                (
                    AnalysisModelCategory::Bgm,
                    "2",
                    "Post-processing 2",
                    SettingsSelectKind::AudioBgmPostprocess2,
                ),
                (
                    AnalysisModelCategory::Vocals,
                    "●",
                    "Vocal separation",
                    SettingsSelectKind::AudioVocalModel,
                ),
                (
                    AnalysisModelCategory::Vocals,
                    "1",
                    "Post-processing 1",
                    SettingsSelectKind::AudioVocalPostprocess1,
                ),
                (
                    AnalysisModelCategory::Vocals,
                    "2",
                    "Post-processing 2",
                    SettingsSelectKind::AudioVocalPostprocess2,
                ),
                (
                    AnalysisModelCategory::Pitch,
                    "●",
                    "Pitch",
                    SettingsSelectKind::PitchModel,
                ),
                (
                    AnalysisModelCategory::Lyrics,
                    "A",
                    "Transcribe",
                    SettingsSelectKind::WhisperModel,
                ),
                (
                    AnalysisModelCategory::Lyrics,
                    "A",
                    "Align",
                    SettingsSelectKind::AlignBackend,
                ),
            ] {
                if selected == category {
                    spawn_model_row(panel, font.clone(), theme, session, icon, label, kind);
                }
            }

            spawn_wrapped_text(
                panel,
                font.clone(),
                "这些选择会保存为分析默认值；已有谱面只会在重新分析后改变。模型安装仍由“设置 > 模型与运行环境”管理。",
                8.0,
                theme.muted_foreground,
            );
            spawn_compact_action_button(
                panel,
                font,
                theme,
                "调节运行参数…",
                UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
            );
        });
}

pub(crate) fn spawn_analysis_toolbar_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    button: (UiIcon, &'static str, bool, UiAction),
) {
    let (icon, label, active, action) = button;
    parent
        .spawn((
            Button,
            action,
            Node {
                height: px(31),
                align_items: AlignItems::Center,
                column_gap: px(5),
                padding: UiRect::horizontal(px(9)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(if active {
                theme.primary.with_alpha(0.17)
            } else {
                Color::NONE
            }),
            BorderColor::all(if active {
                theme.primary.with_alpha(0.75)
            } else {
                theme.border.with_alpha(0.20)
            }),
        ))
        .with_children(|button| {
            spawn_icon(
                button,
                icons,
                icon,
                14.0,
                if active {
                    theme.primary
                } else {
                    theme.muted_foreground
                },
            );
            spawn_text(
                button,
                font,
                label,
                9.0,
                if active {
                    theme.primary
                } else {
                    theme.muted_foreground
                },
            );
        });
}

pub(crate) fn spawn_analysis_header_toolbar(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
    file_hash: &str,
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
    let history = session.selected_analysis_history.and_then(|id| {
        session
            .analysis_history
            .iter()
            .find(|history| history.id == id)
    });
    let live = history
        .map(|history| &history.snapshot)
        .or_else(|| active_task.and_then(|task| task.live.as_ref()));
    let focus_target = |node_id: &str| {
        analysis_node_stage_index(node_id).map(|bucket| {
            (
                estimated_analysis_graph_center_scroll(
                    node_id,
                    clamp_analysis_graph_zoom(session.analysis_graph_zoom),
                    session.analysis_graph_viewport_width,
                )
                .round() as i32,
                bucket_stage_id(bucket).to_string(),
            )
        })
    };
    let current_focus = live
        .and_then(|live| live.node_id.as_deref())
        .and_then(focus_target);
    let problem_focus = live.and_then(|snapshot| {
        snapshot
            .stage_routes
            .iter()
            .find(|route| route.node_event.as_deref() == Some("node_failed"))
            .and_then(|route| route.node_id.as_deref())
            .and_then(focus_target)
    });
    parent
        .spawn(Node {
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::FlexEnd,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(3),
            row_gap: px(4),
            ..default()
        })
        .with_children(|toolbar| {
            if let Some(active) = active_task {
                if app_core::analysis_stop_requested(&active.file_hash) {
                    spawn_text(toolbar, font.clone(), "停止中…", 9.0, theme.destructive);
                } else {
                    spawn_analysis_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        (
                            UiIcon::Analyze,
                            "停止分析",
                            false,
                            UiAction::from(AnalysisCommand::StopAnalysis(active.file_hash.clone())),
                        ),
                    );
                }
            } else if analysis_start_unavailable(file_hash).is_none() {
                spawn_analysis_toolbar_button(
                    toolbar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    (
                        UiIcon::Analyze,
                        "开始分析",
                        false,
                        UiAction::from(AnalysisCommand::StartAnalysis(file_hash.to_string())),
                    ),
                );
            }
            spawn_analysis_toolbar_button(
                toolbar,
                font.clone(),
                icons.clone(),
                theme,
                (
                    UiIcon::Fit,
                    "适合窗口",
                    session.analysis_graph_fit_active,
                    UiAction::from(AnalysisCommand::FitAnalysisGraph(
                        session.analysis_graph_viewport_width.round() as i32,
                    )),
                ),
            );
            spawn_analysis_toolbar_button(
                toolbar,
                font.clone(),
                icons.clone(),
                theme,
                (
                    UiIcon::MiniView,
                    if session.analysis_mini_view {
                        "完整视图"
                    } else {
                        "迷你视图"
                    },
                    session.analysis_mini_view,
                    UiAction::from(AnalysisCommand::ToggleAnalysisMiniView),
                ),
            );
            spawn_analysis_toolbar_button(
                toolbar,
                font.clone(),
                icons.clone(),
                theme,
                (
                    UiIcon::Plan,
                    "计划预览",
                    false,
                    UiAction::from(AnalysisCommand::OpenPlanPreview(file_hash.to_string())),
                ),
            );
            if history.is_some() && active_task.is_some() {
                spawn_analysis_toolbar_button(
                    toolbar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    (
                        UiIcon::Analyze,
                        "查看实时任务",
                        false,
                        UiAction::from(AnalysisCommand::SelectAnalysisHistory(None)),
                    ),
                );
            }
            if let Some((scroll, stage_id)) = current_focus {
                spawn_analysis_toolbar_button(
                    toolbar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    (
                        UiIcon::Focus,
                        "当前节点",
                        false,
                        UiAction::from(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)),
                    ),
                );
            }
            if let Some((scroll, stage_id)) = problem_focus {
                spawn_analysis_toolbar_button(
                    toolbar,
                    font.clone(),
                    icons.clone(),
                    theme,
                    (
                        UiIcon::Warning,
                        "问题节点",
                        false,
                        UiAction::from(AnalysisCommand::FocusAnalysisGraphNode(scroll, stage_id)),
                    ),
                );
            }
            spawn_analysis_toolbar_button(
                toolbar,
                font.clone(),
                icons,
                theme,
                (
                    UiIcon::ModelTune,
                    "快速选模",
                    session.analysis_model_panel_open,
                    UiAction::from(AnalysisCommand::ToggleAnalysisModelPanel),
                ),
            );
        });
}
