use super::*;
use crate::studio::*;

#[cfg(test)]
pub(crate) const ANALYSIS_SETTINGS_SECTION_ORDER: [&str; 6] = [
    "QUALITY & OUTPUT BEHAVIOR",
    "AUDIO PREPARATION",
    "LYRICS & ALIGNMENT",
    "PITCH, NOTES & FUSION",
    "ADVANCED PERFORMANCE / MODEL-OWNED PARAMETERS",
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
        "Configure defaults for future runs",
        "Existing chart data changes only after explicit re-analysis. Models & runtime owns installation and acceleration; this page owns analysis behavior.",
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · QUALITY & OUTPUT BEHAVIOR",
        "How future Candidate and partial analysis actions behave",
        "These are product defaults. You can still override outputs and quality for one run in the confirmation screen before analysis starts.",
        format!(
            "{} · Production",
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
        "02 · AUDIO PREPARATION",
        "Semantic preparation before analysis",
        "Vocal extraction, lead isolation, instrumental extraction and optional cleanup remain distinct. Per-song topology is configured in Processing Studio and resolved by the Engine under Production policy.",
        "Automatic · request-specific",
        None,
        None,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Preparation ownership",
        "The exact Plan Preview shows which source role, separation branch and cleanup stages will run. Instrumental generation remains independent from the analysis-lead branch.",
        None::<(&str, UiAction)>,
    );
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "03 · LYRICS & ALIGNMENT",
        "Transcript authority and timing defaults",
        "Online lyric search is an explicit Song Detail action. Preview never downloads or writes lyrics; supplied canonical text, reference text and generated transcription retain distinct authority.",
        "Automatic · Qwen baseline",
        None,
        None,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Lyrics execution",
        "The Engine requests transcription and forced alignment only when the selected outputs require them. Optional challengers never replace caller-canonical lyrics.",
        None::<(&str, UiAction)>,
    );
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "04 · PITCH, NOTES & FUSION",
        "Continuous evidence and semantic-note policy",
        "Continuous F0, note boundaries, onset support and global Candidate fusion remain separate concepts. Per-song expert policy is configured in Processing Studio.",
        "RMVPE + GAME · managed fusion",
        None,
        None,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Expert policy",
        "Plan Preview is authoritative for the selected primary evidence, conditional challengers, degraded fallback and requested outputs.",
        None::<(&str, UiAction)>,
    );
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "05 · ADVANCED PERFORMANCE / MODEL-OWNED PARAMETERS",
        "Request-owned performance behavior",
        "Only parameters represented by a versioned request or workflow contract belong here. Installed tools, acceleration and downloadable artifacts remain in Models & runtime.",
        "Production defaults",
        None,
        None,
    );
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced execution",
        "Use Processing Studio for per-song execution conditions and exact graph intent. Packaged worker-private tensor parameters are not duplicated as Studio settings.",
        None::<(&str, UiAction)>,
    );
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "06 · AUTOMATION",
        "When analysis should start automatically",
        "Auto-analysis only queues eligible songs. It does not install models, change the selected workflow, or alter per-model parameters.",
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
                    "Fast prioritizes predictable cost. Balanced adds eligible quality checks. Maximum permits expensive optional processing under Production policy.",
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
