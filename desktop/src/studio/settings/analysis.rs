use super::*;
use crate::studio::*;

#[cfg(test)]
pub(crate) const ANALYSIS_SETTINGS_SECTION_ORDER: [&str; 3] = [
    "QUALITY & OUTPUT BEHAVIOR",
    "MODEL RUNTIME PARAMETERS",
    "AUTOMATION",
];

pub(crate) fn spawn_analysis_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    spawn_settings_header(
        parent,
        font.clone(),
        theme,
        "ANALYSIS",
        "Analysis defaults",
        "Set output intent and run behavior here. Provider choice belongs to the Analysis workspace model selector; model-owned tuning belongs to Models & runtime.",
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · QUALITY & OUTPUT BEHAVIOR",
        "What a normal analysis run should produce",
        "The Engine resolves the selected workflow under the permissive local testing policy. Missing resources are reported for the exact request instead of disabling the whole interface.",
        format!(
            "{} · testing",
            quality_label(session.config.analysis_quality())
        ),
        None,
        None,
    );
    spawn_quality_setting_row(
        parent,
        font.clone(),
        theme,
        session.config.analysis_quality(),
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Preserve continuous pitch",
        "Keep continuous F0 as independent PitchEvidence instead of replacing it with the final target-note track.",
        session.config.preserve_continuous_pitch(),
        UiAction::from(SettingsCommand::TogglePreserveContinuousPitch),
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Quantize candidate notes",
        "Apply quantization only to candidate-chart behavior, after note inference; never to continuous PitchEvidence.",
        session.config.enable_quantization(),
        UiAction::from(SettingsCommand::ToggleAnalysisQuantization),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons,
        theme,
        "Default analysis target",
        "Choose the normal Analyze action's product output. This changes requested artifacts, not model selection.",
        SettingsSelectKind::AnalysisTarget,
        session,
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "02 · MODEL RUNTIME PARAMETERS",
        "Selection and tuning are intentionally separated",
        "Choose providers from a song's Analysis workspace with Quick model selection. Configure only model-owned runtime parameters in Models & runtime.",
        "No provider overrides on this page",
        None,
        Some((
            "Open model tuning…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );
    let (model_selection_description, model_selection_action) = if let Some(file_hash) =
        session.selected_song.as_ref()
    {
        (
            "Return to the selected song's Analysis workspace and use Quick model selection. That view owns provider choice for transcription, alignment, pitch, separation, and cleanup nodes.",
            (
                "Open quick model selection".to_string(),
                UiAction::from(AnalysisCommand::OpenSongModelSelection(file_hash.clone())),
            ),
        )
    } else {
        (
            "Choose a song from the library, open Analysis, then use Quick model selection. Settings never writes provider preferences.",
            (
                "Choose a song".to_string(),
                UiAction::from(AppCommand::Home),
            ),
        )
    };
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Where to choose models",
        model_selection_description,
        Some(model_selection_action),
    );
    spawn_exact_strategy_readiness(parent, font.clone(), theme, session);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Where to tune models",
        "Models & runtime contains installation state plus the real parameters consumed by RoFormer, Qwen ASR, and RMVPE. Models without exposed runtime controls are shown as such.",
        Some((
            "Models & runtime",
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "03 · AUTOMATION",
        "Explicit execution behavior",
        "Automation may queue eligible work after a scan. It never installs resources or silently changes the selected workflow.",
        if session.config.auto_analyze() {
            "On"
        } else {
            "Off"
        },
        None,
        None,
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Auto-analyze",
        if session.config.auto_analyze() {
            "On · Eligible songs are queued; any missing component is reported by that request."
        } else {
            "Off · New songs wait for an explicit analysis action."
        },
        session.config.auto_analyze(),
        UiAction::from(SettingsCommand::ToggleAutoAnalyze),
    );
    spawn_setting_row(
        parent,
        font,
        theme,
        "Analysis defaults",
        "Restore recommended product behavior without changing installed models, per-model runtime tuning, source media, song profiles, or existing chart data.",
        Some((
            "Restore defaults",
            UiAction::from(SettingsCommand::RestoreAnalysisDefaults),
        )),
    );
}

fn strategy_readiness_copy(status: &app_core::AnalysisStrategyResourceStatus) -> (String, String) {
    let state = if status.available {
        "READY UNDER CURRENT POLICY"
    } else {
        "BLOCKED"
    };
    let reasons = if status.reasons.is_empty() {
        String::new()
    } else {
        format!(" · reasons: {}", status.reasons.join(", "))
    };
    (
        format!("{} · {state}", status.label),
        format!(
            "Exact resource model:{} · capability {} · validation {} · backend {}{}",
            status.model_id, status.capability, status.validation, status.backend, reasons
        ),
    )
}

fn spawn_exact_strategy_readiness(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
) {
    if let Some(snapshot) = session.model_settings_job.current.as_ref() {
        for status in &snapshot.strategy_resources {
            let (title, description) = strategy_readiness_copy(status);
            spawn_setting_row(
                parent,
                font.clone(),
                theme,
                title,
                description,
                None::<(String, UiAction)>,
            );
        }
        if let Some(error) = snapshot.strategy_resources_error.as_deref() {
            spawn_setting_row(
                parent,
                font,
                theme,
                "Exact strategy readiness unavailable",
                format!("Runtime Manager status query failed: {error}"),
                None::<(String, UiAction)>,
            );
        }
    } else {
        spawn_setting_row(
            parent,
            font,
            theme,
            "Exact strategy readiness",
            if session.model_settings_job.receiver.is_some() {
                "Reading exact model/capability facts from Runtime Manager…"
            } else {
                "Runtime status has not been loaded. Reopen Analysis settings to retry."
            },
            None::<(String, UiAction)>,
        );
    }
}

fn quality_label(quality: app_core::AnalysisQualityProfile) -> &'static str {
    match quality {
        app_core::AnalysisQualityProfile::Fast => "Fast",
        app_core::AnalysisQualityProfile::Balanced => "Balanced · recommended",
        app_core::AnalysisQualityProfile::Maximum => "Maximum",
    }
}

fn spawn_quality_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    selected: app_core::AnalysisQualityProfile,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(92),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), "Analysis quality", 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    "Fast prioritizes predictable cost. Balanced adds eligible quality checks. Maximum permits expensive optional processing under the local testing policy.",
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                min_width: px(180),
                max_width: px(SETTINGS_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_CONTROL_WIDTH),
                flex_grow: 1.0,
                height: px(36),
                margin: UiRect::top(px(2)),
                ..default()
            })
            .with_children(|choices| {
                for (quality, label) in [
                    (app_core::AnalysisQualityProfile::Fast, "Fast"),
                    (app_core::AnalysisQualityProfile::Balanced, "Balanced"),
                    (app_core::AnalysisQualityProfile::Maximum, "Maximum"),
                ] {
                    let active = quality == selected;
                    choices
                        .spawn((
                            Button,
                            UiAction::from(SettingsCommand::SetAnalysisQuality(quality)),
                            Node {
                                flex_grow: 1.0,
                                height: percent(100),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(if active {
                                theme.primary.with_alpha(0.13)
                            } else {
                                theme.background.with_alpha(0.36)
                            }),
                            BorderColor::all(if active {
                                theme.primary.with_alpha(0.58)
                            } else {
                                theme.border.with_alpha(0.45)
                            }),
                        ))
                        .with_children(|button| {
                            spawn_text(
                                button,
                                font.clone(),
                                label,
                                9.0,
                                if active { theme.primary } else { theme.foreground },
                            );
                        });
                }
            });
        });
}

// Retained only as non-compiled migration reference for legacy configuration keys.
#[cfg(any())]
pub(crate) fn spawn_advanced_controls(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    session: &StudioSessionView<'_>,
) {
    let separation_open =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Separation);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "RoFormer model parameters",
        "These values are consumed by compatible RoFormer separation and cleanup workers. Provider selection remains in the Analysis workspace.",
        Some((
            if separation_open { "Hide" } else { "Show" },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Separation,
            )),
        )),
    );
    if separation_open {
        for (label, description, value, setting, down, up) in [
            (
                "Segment size",
                "RoFormer processing window. Range: 64–1024.",
                session.config.separator_segment_size(),
                NumericSetting::SeparatorSegmentSize,
                SettingsCommand::AdjustSeparatorSegmentSize(-32),
                SettingsCommand::AdjustSeparatorSegmentSize(32),
            ),
            (
                "Overlap",
                "Overlap for the owning separator. Range: 2–32.",
                session.config.separator_overlap(),
                NumericSetting::SeparatorOverlap,
                SettingsCommand::AdjustSeparatorOverlap(-1),
                SettingsCommand::AdjustSeparatorOverlap(1),
            ),
            (
                "Batch size",
                "Owning separator batch size. Range: 1–8.",
                session.config.separator_batch_size(),
                NumericSetting::SeparatorBatchSize,
                SettingsCommand::AdjustSeparatorBatchSize(-1),
                SettingsCommand::AdjustSeparatorBatchSize(1),
            ),
            (
                "Output normalization",
                "Compatibility workflow normalization. Range: 1–100%.",
                session.config.separator_normalization_pct(),
                NumericSetting::SeparatorNormalization,
                SettingsCommand::AdjustSeparatorNormalization(-1),
                SettingsCommand::AdjustSeparatorNormalization(1),
            ),
        ] {
            spawn_number_setting_row(
                parent,
                font.clone(),
                theme,
                label,
                description,
                value,
                setting,
                UiAction::from(down),
                UiAction::from(up),
            );
        }
    }

    let asr_open = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Asr);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Qwen ASR runtime parameters",
        "Beam and batch sizes are consumed by the local transcription route. They tune the selected Qwen ASR provider without selecting it.",
        Some((
            if asr_open { "Hide" } else { "Show" },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Asr,
            )),
        )),
    );
    if asr_open {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Beam size",
            "Search beam used by the compatible Qwen ASR route. Range: 1–64.",
            session.config.beam_size(),
            NumericSetting::AsrBeamSize,
            UiAction::from(SettingsCommand::AdjustAsrBeamSize(-1)),
            UiAction::from(SettingsCommand::AdjustAsrBeamSize(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Batch size",
            "Inference batch size used by the compatible Qwen ASR route. Range: 1–32.",
            session.config.batch_size(),
            NumericSetting::AsrBatchSize,
            UiAction::from(SettingsCommand::AdjustAsrBatchSize(-1)),
            UiAction::from(SettingsCommand::AdjustAsrBatchSize(1)),
        );
    }
    let pitch_open = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Pitch);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "RMVPE runtime parameters",
        "Voiced sensitivity belongs to the RMVPE-compatible route and remains separate from provider selection and note-boundary thresholds.",
        Some((
            if pitch_open { "Hide" } else { "Show" },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Pitch,
            )),
        )),
    );
    if pitch_open {
        spawn_number_setting_row(
            parent,
            font,
            theme,
            "Voiced sensitivity",
            "Lower for soft singing; raise to suppress silence. Range: 0–60%.",
            (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32,
            NumericSetting::VocalThreshold,
            UiAction::from(SettingsCommand::AdjustVocalThreshold(-1)),
            UiAction::from(SettingsCommand::AdjustVocalThreshold(1)),
        );
    }
}

#[cfg(any())]
#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_number_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: u32,
    setting: NumericSetting,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                min_width: px(180),
                max_width: px(SETTINGS_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_CONTROL_WIDTH),
                flex_grow: 1.0,
                margin: UiRect::top(px(2)),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control.spawn((
                            EditableText {
                                max_characters: Some(4),
                                ..EditableText::new(value.to_string())
                            },
                            setting,
                            Node {
                                min_width: px(56),
                                height: px(20),
                                flex_grow: 1.0,
                                align_self: AlignSelf::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            },
                            ui_text_font(font.clone(), 11.0),
                            TextColor(theme.foreground),
                            TextLayout::justify(Justify::Center),
                            TextCursorStyle {
                                color: theme.primary,
                                selected_text_color: Some(theme.primary_foreground),
                                ..default()
                            },
                            TabIndex(0),
                        ));
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

pub(crate) fn spawn_switch_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    enabled: bool,
    action: UiAction,
) {
    let label = label.into();
    let description = description.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(76),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(24),
                row_gap: px(12),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(260),
                flex_basis: px(360),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(
                    copy,
                    font.clone(),
                    description,
                    10.0,
                    theme.muted_foreground,
                );
            });
            row.spawn(Node {
                min_width: px(180),
                max_width: px(SETTINGS_CONTROL_WIDTH),
                flex_basis: px(SETTINGS_CONTROL_WIDTH),
                flex_grow: 1.0,
                margin: UiRect::top(px(2)),
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Button,
                        action,
                        Node {
                            width: px(42),
                            height: px(24),
                            align_items: AlignItems::Center,
                            justify_content: if enabled {
                                JustifyContent::FlexEnd
                            } else {
                                JustifyContent::FlexStart
                            },
                            padding: UiRect::horizontal(px(3)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(if enabled {
                            theme.primary.with_alpha(0.86)
                        } else {
                            theme.background.with_alpha(0.7)
                        }),
                        BorderColor::all(if enabled {
                            theme.primary.with_alpha(0.9)
                        } else {
                            theme.border.with_alpha(0.75)
                        }),
                    ))
                    .with_children(|switch| {
                        switch.spawn((
                            Node {
                                width: px(16),
                                height: px(16),
                                border_radius: BorderRadius::MAX,
                                ..default()
                            },
                            BackgroundColor(if enabled {
                                theme.primary_foreground
                            } else {
                                theme.muted_foreground.with_alpha(0.8)
                            }),
                        ));
                    });
            });
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_shift_setting_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: impl Into<String>,
    description: impl Into<String>,
    value: impl Into<String>,
    decrement: UiAction,
    increment: UiAction,
) {
    let label = label.into();
    let description = description.into();
    let value = value.into();
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(68),
                flex_shrink: 0.0,
                align_items: AlignItems::FlexStart,
                padding: UiRect::axes(px(20), px(13)),
                column_gap: px(22),
                border: UiRect::bottom(px(1)),
                ..default()
            },
            BorderColor::all(theme.border.with_alpha(0.42)),
        ))
        .with_children(|row| {
            row.spawn(Node {
                min_width: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: px(4),
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(copy, font.clone(), description, 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
                margin: UiRect::top(px(2)),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                ..default()
            })
            .with_children(|control_column| {
                control_column
                    .spawn((
                        Node {
                            width: px(142),
                            height: px(34),
                            align_items: AlignItems::Center,
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.34)),
                        BorderColor::all(theme.border.with_alpha(0.4)),
                    ))
                    .with_children(|control| {
                        spawn_text_button(control, font.clone(), theme, "−", 15.0, decrement);
                        control
                            .spawn(Node {
                                min_width: px(68),
                                flex_grow: 1.0,
                                height: percent(100),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::Center,
                                ..default()
                            })
                            .with_children(|value_node| {
                                spawn_text(value_node, font.clone(), value, 10.0, theme.foreground);
                            });
                        spawn_text_button(control, font.clone(), theme, "+", 15.0, increment);
                    });
            });
        });
}

#[cfg(test)]
mod parity_tests {
    use super::strategy_readiness_copy;

    #[test]
    fn exact_strategy_copy_names_the_model_capability_and_runtime_fact() {
        let status = app_core::AnalysisStrategyResourceStatus {
            strategy_id: "vocal_extraction".to_string(),
            label: "Vocal extraction".to_string(),
            model_id: "bs_roformer_vocals_ep317".to_string(),
            capability: "audio.extract_vocals".to_string(),
            available: true,
            backend: "openvino".to_string(),
            validation: "benchmark_candidate".to_string(),
            reasons: Vec::new(),
        };
        let (title, description) = strategy_readiness_copy(&status);
        assert!(title.contains("READY UNDER CURRENT POLICY"));
        assert!(description.contains("model:bs_roformer_vocals_ep317"));
        assert!(description.contains("audio.extract_vocals"));
        assert!(description.contains("benchmark_candidate"));
    }

    #[test]
    fn canonical_models_page_does_not_render_legacy_advanced_controls() {
        let models_page = include_str!("models.rs");
        assert!(!models_page.contains("spawn_advanced_controls("));
        assert!(models_page.contains("No editable model parameters"));
    }
}
