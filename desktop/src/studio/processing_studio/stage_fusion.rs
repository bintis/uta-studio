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
    spawn_wrapped_text(
        lane,
        font.clone(),
        "STEP 4 · FINAL FUSION",
        9.0,
        theme.primary,
    );
    spawn_wrapped_text(
        lane,
        font.clone(),
        "Choose only how the final path is selected. Configure expert participation in Step 3; the Engine owns evidence normalization and candidate construction.",
        8.0,
        theme.muted_foreground,
    );

    let fusion_mode = app_core::fusion_mode(&stored.definition);
    lane.spawn(Node {
        width: percent(100),
        column_gap: px(6),
        row_gap: px(6),
        flex_wrap: FlexWrap::Wrap,
        ..default()
    })
    .with_children(|modes| {
        let algorithm_selected = fusion_mode == app_core::FusionModeV1::Algorithm;
        action_button(
            modes,
            font.clone(),
            theme,
            if algorithm_selected {
                "✓ Algorithm".to_string()
            } else {
                "Algorithm".to_string()
            },
            UiAction::from(AnalysisCommand::SetWorkflowParameter(
                "evidence_fusion".to_string(),
                "fusion_mode".to_string(),
                serde_json::Value::String("algorithm".to_string()),
            )),
        );

        let ai_selected = fusion_mode == app_core::FusionModeV1::AiJudgment;
        if adapter_readiness == FusionAdapterReadinessUi::Usable {
            action_button(
                modes,
                font.clone(),
                theme,
                ai_mode_label(ai_selected, adapter_readiness),
                UiAction::from(AnalysisCommand::SetWorkflowParameter(
                    "evidence_fusion".to_string(),
                    "fusion_mode".to_string(),
                    serde_json::Value::String("ai".to_string()),
                )),
            );
        } else {
            disabled_action_button(
                modes,
                font.clone(),
                theme,
                ai_mode_label(ai_selected, adapter_readiness),
            );
        }
    });

    if fusion_mode == app_core::FusionModeV1::AiJudgment {
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
        spawn_wrapped_text(lane, font.clone(), readiness, 8.0, theme.editor_warning);
        spawn_wrapped_text(
            lane,
            font.clone(),
            "Candidate metadata and canonical lyrics may be sent to its external AI provider; no source audio or project files are included. Any adapter, provider, timeout, cancellation, or validation failure stops analysis without Algorithm fallback.",
            8.0,
            theme.editor_warning,
        );
    }

    spawn_wrapped_text(
        lane,
        font.clone(),
        format!("Configured evidence · {}", evidence_summary(stored, true)),
        8.0,
        theme.primary,
    );
    spawn_wrapped_text(
        lane,
        font.clone(),
        format!("Potential evidence · {}", evidence_summary(stored, false)),
        8.0,
        theme.muted_foreground,
    );
    spawn_wrapped_text(
        lane,
        font,
        "Evidence normalization -> candidate construction -> final path selection -> canonical singing track",
        8.0,
        theme.muted_foreground,
    );
}
