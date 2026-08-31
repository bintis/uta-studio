use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FusionAdapterReadinessUi {
    Checking,
    Usable,
    Missing,
    Unusable,
    StatusError,
}

pub(super) fn classify_fusion_adapter_readiness(
    refresh_pending: bool,
    status_error: bool,
    usable: Option<bool>,
) -> FusionAdapterReadinessUi {
    if refresh_pending {
        FusionAdapterReadinessUi::Checking
    } else if status_error {
        FusionAdapterReadinessUi::StatusError
    } else {
        match usable {
            Some(true) => FusionAdapterReadinessUi::Usable,
            Some(false) => FusionAdapterReadinessUi::Unusable,
            None => FusionAdapterReadinessUi::Missing,
        }
    }
}

#[cfg(test)]
pub(super) fn ai_mode_label(selected: bool, readiness: FusionAdapterReadinessUi) -> String {
    let selected = if selected { "✓ " } else { "" };
    let reason = match readiness {
        FusionAdapterReadinessUi::Usable => return format!("{selected}AI judgment"),
        FusionAdapterReadinessUi::Checking => "checking local adapter status",
        FusionAdapterReadinessUi::Missing => "adapter not configured in Models & runtime",
        FusionAdapterReadinessUi::Unusable => "adapter unusable in Models & runtime",
        FusionAdapterReadinessUi::StatusError => "adapter status unavailable",
    };
    format!("{selected}AI judgment · {reason}")
}

pub(super) fn fusion_adapter_readiness(
    session: &StudioSessionView<'_>,
) -> FusionAdapterReadinessUi {
    classify_fusion_adapter_readiness(
        session.model_settings_refresh_pending || session.model_settings_job.receiver.is_some(),
        session.model_settings_job.error.is_some()
            || session
                .model_settings_job
                .current
                .as_ref()
                .is_some_and(|snapshot| snapshot.fusion_agent_adapter_error.is_some()),
        session
            .model_settings_job
            .current
            .as_ref()
            .and_then(|snapshot| snapshot.fusion_agent_adapter.as_ref())
            .map(|status| status.usable),
    )
}

fn evidence_summary(stored: &app_core::StoredWorkflow, configured: bool) -> String {
    let labels = stored
        .definition
        .nodes
        .iter()
        .filter(|node| {
            matches!(
                node.capability_id.as_str(),
                "analysis.pitch_f0"
                    | "analysis.note_boundary"
                    | "analysis.technique"
                    | "analysis.acoustic_dsp"
            ) && if configured {
                node.execution_policy == app_core::ExecutionPolicy::Always
            } else {
                !matches!(
                    node.execution_policy,
                    app_core::ExecutionPolicy::Always | app_core::ExecutionPolicy::Disabled
                )
            }
        })
        .map(|node| {
            node.model_id
                .as_deref()
                .unwrap_or(node.capability_id.as_str())
                .to_string()
        })
        .collect::<Vec<_>>();
    if labels.is_empty() {
        "None".to_string()
    } else {
        labels.join(" · ")
    }
}

struct FusionModeChoice<'a> {
    title: &'a str,
    detail: &'a str,
    selected: bool,
    available: bool,
    action: UiAction,
}

fn spawn_mode_choice(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    choice: FusionModeChoice<'_>,
) {
    let FusionModeChoice {
        title,
        detail,
        selected,
        available,
        action,
    } = choice;
    let color = if selected {
        theme.primary
    } else if available {
        theme.foreground
    } else {
        theme.muted_foreground
    };
    let mut choice = parent.spawn((
        Node {
            min_width: px(128),
            flex_basis: px(150),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(9)),
            row_gap: px(4),
            border: UiRect::all(px(if selected { 2 } else { 1 })),
            border_radius: studio_card_radius(),
            ..default()
        },
        BackgroundColor(if selected {
            theme.primary.with_alpha(0.09)
        } else {
            theme.background.with_alpha(0.18)
        }),
        BorderColor::all(if selected {
            theme.primary.with_alpha(0.52)
        } else {
            theme.border.with_alpha(0.32)
        }),
    ));
    if available && !selected {
        choice.insert((Button, action));
    } else {
        choice.insert(Pickable::IGNORE);
    }
    choice.with_children(|card| {
        card.spawn(Node {
            width: percent(100),
            align_items: AlignItems::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn((
                    Node {
                        width: px(14),
                        height: px(14),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(color.with_alpha(0.1)),
                    BorderColor::all(color.with_alpha(0.28)),
                ))
                .with_children(|mark| {
                    spawn_text(
                        mark,
                        font.clone(),
                        if selected { "✓" } else { "" },
                        7.0,
                        color,
                    );
                });
            spawn_wrapped_text(header, font.clone(), title, 9.0, color);
        });
        spawn_wrapped_text(card, font.clone(), detail, 6.9, theme.muted_foreground);
    });
}

/// Step 4 exposes one product decision only. Evidence participation belongs
/// exclusively to Stage 3; the Engine owns normalization, candidate
/// construction and every internal baseline/fallback decision.
pub(super) fn spawn_fusion_stage_card(
    lane: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    stored: &app_core::StoredWorkflow,
    adapter_readiness: FusionAdapterReadinessUi,
) {
    let fusion_mode = app_core::fusion_mode(&stored.definition);
    let algorithm_selected = fusion_mode == app_core::FusionModeV1::Algorithm;
    let ai_selected = fusion_mode == app_core::FusionModeV1::AiJudgment;
    let ai_available = adapter_readiness == FusionAdapterReadinessUi::Usable;
    let selected_label = if algorithm_selected {
        "Algorithm"
    } else {
        "AI judgment"
    };

    lane.spawn((
        Node {
            width: percent(100),
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(10)),
            row_gap: px(8),
            border: UiRect::all(px(1)),
            border_radius: studio_card_radius(),
            ..default()
        },
        BackgroundColor(theme.card.with_alpha(0.2)),
        BorderColor::all(theme.border.with_alpha(0.38)),
    ))
    .with_children(|card| {
        card.spawn(Node {
            width: percent(100),
            min_width: px(0),
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|header| {
            header.spawn(Node {
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
                    "STEP 4 · FINAL FUSION",
                    6.8,
                    theme.muted_foreground,
                );
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    "Final decision mode",
                    10.5,
                    theme.foreground,
                );
            });
            header
                .spawn((
                    Node {
                        min_height: px(22),
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(px(8), px(3)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BackgroundColor(theme.primary.with_alpha(0.1)),
                    BorderColor::all(theme.primary.with_alpha(0.3)),
                ))
                .with_children(|badge| {
                    spawn_text(badge, font.clone(), selected_label, 7.0, theme.primary);
                });
        });
        spawn_wrapped_text(
            card,
            font.clone(),
            "Choose only how the final path is selected. Configure expert participation in Step 3; the Engine owns evidence normalization and candidate construction.",
            7.4,
            theme.muted_foreground,
        );

        card.spawn(Node {
            width: percent(100),
            min_width: px(0),
            column_gap: px(6),
            row_gap: px(6),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|modes| {
            spawn_mode_choice(
                modes,
                font.clone(),
                theme,
                FusionModeChoice {
                    title: "Algorithm",
                    detail: "Deterministic Engine candidate-path selection.",
                    selected: algorithm_selected,
                    available: true,
                    action: UiAction::from(AnalysisCommand::SetWorkflowParameter(
                        "evidence_fusion".to_string(),
                        "fusion_mode".to_string(),
                        serde_json::Value::String("algorithm".to_string()),
                    )),
                },
            );
            spawn_mode_choice(
                modes,
                font.clone(),
                theme,
                FusionModeChoice {
                    title: "AI judgment",
                    detail: match adapter_readiness {
                        FusionAdapterReadinessUi::Usable => "Verified adapter is available.",
                        FusionAdapterReadinessUi::Checking => "Checking local adapter status.",
                        FusionAdapterReadinessUi::Missing => {
                            "Configure an adapter in Models & runtime."
                        }
                        FusionAdapterReadinessUi::Unusable => {
                            "Configured adapter is not currently usable."
                        }
                        FusionAdapterReadinessUi::StatusError => {
                            "Adapter status is currently unavailable."
                        }
                    },
                    selected: ai_selected,
                    available: ai_available,
                    action: UiAction::from(AnalysisCommand::SetWorkflowParameter(
                        "evidence_fusion".to_string(),
                        "fusion_mode".to_string(),
                        serde_json::Value::String("ai".to_string()),
                    )),
                },
            );
        });

        if ai_selected {
            let readiness = match adapter_readiness {
                FusionAdapterReadinessUi::Usable => {
                    "The verified adapter will select only from the Engine's real candidate pool."
                }
                FusionAdapterReadinessUi::Checking => {
                    "Saved AI mode is selected, but analysis is blocked while adapter status is checked."
                }
                FusionAdapterReadinessUi::Missing => {
                    "Saved AI mode is selected, but analysis is blocked until an adapter is configured."
                }
                FusionAdapterReadinessUi::Unusable => {
                    "Saved AI mode is selected, but analysis is blocked because the adapter is unusable."
                }
                FusionAdapterReadinessUi::StatusError => {
                    "Saved AI mode is selected, but analysis is blocked because adapter status is unavailable."
                }
            };
            card.spawn((
                Node {
                    width: percent(100),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(8)),
                    row_gap: px(4),
                    border: UiRect::all(px(1)),
                    border_radius: studio_card_radius(),
                    ..default()
                },
                BackgroundColor(theme.editor_warning.with_alpha(0.055)),
                BorderColor::all(theme.editor_warning.with_alpha(0.28)),
            ))
            .with_children(|warning| {
                spawn_wrapped_text(
                    warning,
                    font.clone(),
                    readiness,
                    7.4,
                    theme.editor_warning,
                );
                spawn_wrapped_text(
                    warning,
                    font.clone(),
                    "Candidate metadata and canonical lyrics may be sent to its external AI provider; no source audio or project files are included. Any adapter, provider, timeout, cancellation, or validation failure stops analysis without Algorithm fallback.",
                    6.8,
                    theme.muted_foreground,
                );
            });
        }

        card.spawn(Node {
            width: percent(100),
            min_width: px(0),
            column_gap: px(6),
            row_gap: px(6),
            flex_wrap: FlexWrap::Wrap,
            ..default()
        })
        .with_children(|evidence| {
            for (label, value, color) in [
                (
                    "Configured evidence",
                    evidence_summary(stored, true),
                    theme.primary,
                ),
                (
                    "Potential evidence",
                    evidence_summary(stored, false),
                    theme.muted_foreground,
                ),
            ] {
                evidence
                    .spawn((
                        Node {
                            min_width: px(112),
                            flex_basis: px(142),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(7)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: studio_card_radius(),
                            overflow: Overflow::clip(),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.16)),
                        BorderColor::all(theme.border.with_alpha(0.3)),
                    ))
                    .with_children(|fact| {
                        spawn_text(
                            fact,
                            font.clone(),
                            label,
                            6.4,
                            theme.muted_foreground,
                        );
                        spawn_wrapped_text(fact, font.clone(), value, 7.2, color);
                    });
            }
        });
        spawn_wrapped_text(
            card,
            font,
            "Evidence normalization -> candidate construction -> final path selection -> canonical singing track",
            6.8,
            theme.muted_foreground,
        );
    });
}
