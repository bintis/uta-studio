use super::*;
use crate::studio::*;

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
        "GENERATION",
        "Analysis",
        "Configure result-shaping defaults for the next Workflow run. Existing chart data changes only after explicit re-analysis.",
    );
    let status = app_core::analysis_runtime_status();
    spawn_analysis_pipeline(parent, font.clone(), theme, session, &status);

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · SEPARATION & RESTORATION",
        "Vocal, BGM, lead, and harmony",
        "Processing Studio controls topology. These defaults select models and processing strength for new workflows.",
        vocal_separation_label(session.config),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::RoFormer),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Primary vocal separator",
        "Select a validated catalog model. Dragging transformations in Processing Studio determines their actual order.",
        SettingsSelectKind::AudioVocalModel,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "BGM separator",
        "The BGM lane is independent from the vocal lane.",
        SettingsSelectKind::AudioAccompanimentModel,
        session,
    );
    for (label, description, kind) in [
        (
            "Vocal processing 1",
            "First default restoration node; choose Off, Denoise, or Dereverb.",
            SettingsSelectKind::AudioVocalPostprocess1,
        ),
        (
            "Vocal processing 2",
            "Second default restoration node. Duplicate capabilities remain valid workflow instances.",
            SettingsSelectKind::AudioVocalPostprocess2,
        ),
        (
            "BGM processing 1",
            "First independent BGM restoration node.",
            SettingsSelectKind::AudioBgmPostprocess1,
        ),
        (
            "BGM processing 2",
            "Second independent BGM restoration node.",
            SettingsSelectKind::AudioBgmPostprocess2,
        ),
    ] {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            label,
            description,
            kind,
            session,
        );
    }
    let separation_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Separation);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced separation tuning",
        "Quality, memory, and overlap values belong to Analysis; runtime installation and backend validation do not.",
        Some((
            if separation_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Separation,
            )),
        )),
    );
    if separation_advanced {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Segment size",
            "Model-specific processing window. Range: 64–1024.",
            session.config.separator_segment_size(),
            NumericSetting::SeparatorSegmentSize,
            UiAction::from(SettingsCommand::AdjustSeparatorSegmentSize(-32)),
            UiAction::from(SettingsCommand::AdjustSeparatorSegmentSize(32)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Overlap",
            "More overlap can reduce seams while increasing processing cost. Range: 2–32.",
            session.config.separator_overlap(),
            NumericSetting::SeparatorOverlap,
            UiAction::from(SettingsCommand::AdjustSeparatorOverlap(-1)),
            UiAction::from(SettingsCommand::AdjustSeparatorOverlap(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Batch size",
            "Conservative value is recommended on Intel Arc. Range: 1–8.",
            session.config.separator_batch_size(),
            NumericSetting::SeparatorBatchSize,
            UiAction::from(SettingsCommand::AdjustSeparatorBatchSize(-1)),
            UiAction::from(SettingsCommand::AdjustSeparatorBatchSize(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Output normalization",
            "Peak normalization before lossless cache commit. Range: 1–100%.",
            session.config.separator_normalization_pct(),
            NumericSetting::SeparatorNormalization,
            UiAction::from(SettingsCommand::AdjustSeparatorNormalization(-1)),
            UiAction::from(SettingsCommand::AdjustSeparatorNormalization(1)),
        );
    }

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "02 · TRANSCRIPT FUSION",
        "Canonical lyrics",
        "FireRed and Qwen remain independent evidence. Canonical lyrics are produced by token-level fusion, not winner-takes-all selection.",
        transcription_summary(session.config),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::FireRed),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );
    let transcription_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Transcription);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced transcript tuning",
        "Search and batching values are applied to the owning expert only.",
        Some((
            if transcription_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Transcription,
            )),
        )),
    );
    if transcription_advanced {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Search breadth",
            "Bounded transcript search breadth. Range: 1–16.",
            session.config.beam_size(),
            NumericSetting::BeamSize,
            UiAction::from(SettingsCommand::AdjustBeamSize(-1)),
            UiAction::from(SettingsCommand::AdjustBeamSize(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Transcript batch size",
            "Lower this under memory pressure. It does not alter dependency order.",
            session.config.batch_size(),
            NumericSetting::BatchSize,
            UiAction::from(SettingsCommand::AdjustBatchSize(-1)),
            UiAction::from(SettingsCommand::AdjustBatchSize(1)),
        );
    }

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "03 · FORCED ALIGNMENT",
        "Canonical word boundaries",
        "The pinned native aligner consumes frozen Canonical Lyrics and lead-vocal audio. Transcript timestamps remain secondary evidence.",
        "Qwen3 Forced Aligner · Vulkan",
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::QwenAlign),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "04 · PITCH, BOUNDARY & FUSION",
        "Canonical singing track",
        "RMVPE continuous F0, GAME boundaries, optional experts, acoustic evidence, and global sequence optimization remain distinct.",
        "RMVPE + GAME + conditional experts",
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Pitch),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );
    let pitch_advanced = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Pitch);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced pitch tuning",
        "Adjust voiced sensitivity. Continuous F0 is never rounded directly into final notes.",
        Some((
            if pitch_advanced {
                "Hide advanced"
            } else {
                "Show advanced"
            },
            UiAction::from(SettingsCommand::ToggleAnalysisAdvanced(
                AnalysisAdvancedSection::Pitch,
            )),
        )),
    );
    if pitch_advanced {
        let threshold = (session.config.vocal_detection_threshold_pct() * 100.0).round() as u32;
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Voiced sensitivity",
            "Lower for soft singing; raise to suppress silence. Range: 0–60%.",
            threshold,
            NumericSetting::VocalThreshold,
            UiAction::from(SettingsCommand::AdjustVocalThreshold(-1)),
            UiAction::from(SettingsCommand::AdjustVocalThreshold(1)),
        );
    }

    spawn_settings_section(
        parent,
        font.clone(),
        theme,
        "AUTOMATION",
        "Starting analysis never installs or downloads runtime components.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Auto-analyze",
        if session.config.auto_analyze() {
            "On · Ready songs are queued after a scan; unavailable runtime keeps them blocked."
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
        "Restore recommended result-shaping values without changing installed models or existing chart data.",
        Some((
            "Restore defaults",
            UiAction::from(SettingsCommand::RestoreAnalysisDefaults),
        )),
    );
}

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
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
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
                width: px(SETTINGS_CONTROL_WIDTH),
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
                        control.spawn((
                            EditableText {
                                max_characters: Some(2),
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
                align_items: AlignItems::Center,
                padding: UiRect::axes(px(20), px(16)),
                column_gap: px(32),
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
                width: px(SETTINGS_CONTROL_WIDTH),
                flex_shrink: 0.0,
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
                align_items: AlignItems::Center,
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
                ..default()
            })
            .with_children(|copy| {
                spawn_text(copy, font.clone(), label, 12.0, theme.foreground);
                spawn_wrapped_text(copy, font.clone(), description, 9.0, theme.muted_foreground);
            });
            row.spawn(Node {
                width: px(SETTINGS_CONTROL_WIDTH),
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
