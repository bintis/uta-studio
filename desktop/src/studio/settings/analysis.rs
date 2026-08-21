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
        "Configure each stage of newly generated stems, lyrics, timing, and pitch. Existing charts change only after re-analysis.",
    );
    let status = app_core::analysis_runtime_status();
    spawn_analysis_pipeline(parent, font.clone(), theme, session, &status);

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "01 · STEM SEPARATION",
        "Vocal & BGM separation",
        "Creates independent vocal and BGM sources before lyrics, pitch, and chart generation.",
        vocal_separation_label(session.config),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Separator),
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
        "Separation route",
        "Choose the catalog vocal/BGM route or the alternate six-stem route.",
        SettingsSelectKind::Separator,
        session,
    );
    match session.config.separator() {
        "karaoke" => {
            spawn_settings_section(
                parent,
                font.clone(),
                theme,
                "VOCAL OUTPUT",
                "Choose the vocal separator, then apply up to two post-processing steps in slot order.",
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Vocal separation model",
                "Dedicated model that produces the analysis vocal used by lyrics and pitch.",
                SettingsSelectKind::AudioVocalModel,
                session,
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Post-processing 1",
                "First optional vocal step. Choose Off, Denoise, or Dereverb.",
                SettingsSelectKind::AudioVocalPostprocess1,
                session,
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Post-processing 2",
                "Second optional vocal step. It runs after post-processing 1.",
                SettingsSelectKind::AudioVocalPostprocess2,
                session,
            );
            spawn_settings_section(
                parent,
                font.clone(),
                theme,
                "BGM OUTPUT",
                "Choose the BGM separator, then apply up to two independent post-processing steps in slot order.",
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "BGM separation model",
                "Dedicated BGM model that produces the chart accompaniment independently of the vocal model.",
                SettingsSelectKind::AudioAccompanimentModel,
                session,
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Post-processing 1",
                "First optional BGM step. Choose Off, Denoise, or Dereverb.",
                SettingsSelectKind::AudioBgmPostprocess1,
                session,
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Post-processing 2",
                "Second optional BGM step. It runs after post-processing 1.",
                SettingsSelectKind::AudioBgmPostprocess2,
                session,
            );
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Karaoke model",
                "Optional karaoke side output; it never replaces the analysis vocal.",
                SettingsSelectKind::AudioKaraokeModel,
                session,
            );
        }
        "demucs" => {
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Six-stem model",
                "Demucs model that produces vocals, drums, bass, guitar, piano, and other stems.",
                SettingsSelectKind::AudioMultistemModel,
                session,
            );
        }
        _ => {}
    }
    if session.config.separator() != "openvino_demucs" {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "PyTorch backend",
            "Compute route for the selected RoFormer or Demucs model. Whole-model CPU fallback is recorded.",
            SettingsSelectKind::AudioTorchBackend,
            session,
        );
        if session.config.separator() == "karaoke"
            && settings_select_value(SettingsSelectKind::AudioKaraokeModel, session.config)
                == "uvr_mdxnet_karaoke_2"
        {
            spawn_select_setting_row(
                parent,
                font.clone(),
                icons.clone(),
                theme,
                "Karaoke ONNX backend",
                "Compute route used only by the selected MDX-NET Karaoke model.",
                SettingsSelectKind::AudioOnnxBackend,
                session,
            );
        }
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "Precision policy",
            "Precision requested for the selected catalog model.",
            SettingsSelectKind::AudioPrecisionPolicy,
            session,
        );
    }
    let separation_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Separation);
    if session.config.separator() != "openvino_demucs" {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "Separation profile",
            if session.config.separator() == "karaoke" {
                "Balanced is recommended. Memory saver uses shorter RoFormer segments; Quality increases segment context and overlap."
            } else {
                "Balanced is recommended. Quality adds shifts and overlap, increasing processing time substantially."
            },
            SettingsSelectKind::SeparatorPreset,
            session,
        );
        spawn_setting_row(
            parent,
            font.clone(),
            theme,
            "Advanced separation tuning",
            "Model-specific memory, quality, and overlap controls. Existing stems change only after re-analysis.",
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
    } else {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "FIXED OPENVINO PROFILE",
            "Segment dimensions and overlap are compiled into the installed OpenVINO Demucs graph. Select UVR Karaoke or Demucs to use adjustable separation profiles.",
        );
    }
    if separation_advanced && session.config.separator() == "karaoke" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer segment size",
            "Model default is used until edited. Smaller values reduce memory; larger values may improve continuity. Range: 64–1024.",
            session.config.separator_segment_size(),
            NumericSetting::SeparatorSegmentSize,
            UiAction::from(SettingsCommand::AdjustSeparatorSegmentSize(-32)),
            UiAction::from(SettingsCommand::AdjustSeparatorSegmentSize(32)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer overlap",
            "More overlap can reduce chunk seams at the cost of additional processing. Range: 2–32.",
            session.config.separator_overlap(),
            NumericSetting::SeparatorOverlap,
            UiAction::from(SettingsCommand::AdjustSeparatorOverlap(-1)),
            UiAction::from(SettingsCommand::AdjustSeparatorOverlap(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer batch size",
            "Lower this first if separation runs out of system or accelerator memory. Range: 1–8.",
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
            "Peak normalization applied by the separator before stems enter the lossless cache. Range: 1–100%.",
            session.config.separator_normalization_pct(),
            NumericSetting::SeparatorNormalization,
            UiAction::from(SettingsCommand::AdjustSeparatorNormalization(-1)),
            UiAction::from(SettingsCommand::AdjustSeparatorNormalization(1)),
        );
    } else if separation_advanced && session.config.separator() == "demucs" {
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs shifts",
            "More random shifts can improve separation quality but multiply inference cost. Range: 1–8.",
            session.config.demucs_shifts(),
            NumericSetting::DemucsShifts,
            UiAction::from(SettingsCommand::AdjustDemucsShifts(-1)),
            UiAction::from(SettingsCommand::AdjustDemucsShifts(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs overlap",
            "Overlap between inference windows. Range: 1–95%.",
            session.config.demucs_overlap_pct(),
            NumericSetting::DemucsOverlap,
            UiAction::from(SettingsCommand::AdjustDemucsOverlap(-1)),
            UiAction::from(SettingsCommand::AdjustDemucsOverlap(1)),
        );
    }

    let parakeet = session.config.asr_engine() == "parakeet";
    let intel_whisper = !parakeet && session.config.compute_backend.as_deref() == Some("intel");
    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "02 · LYRICS TRANSCRIPTION",
        "Lyrics transcription",
        "Recognizes sung words. Fallback settings appear separately when the primary engine needs them.",
        transcription_summary(session.config),
        Some(analysis_stage_status(
            &status,
            Some(transcription_model_target(session.config)),
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
        "Primary transcription engine",
        "Whisper is broadly compatible; Parakeet is faster for its supported languages.",
        SettingsSelectKind::AsrEngine,
        session,
    );
    if parakeet || intel_whisper {
        spawn_settings_section(
            parent,
            font.clone(),
            theme,
            "COMPATIBILITY FALLBACK",
            if parakeet {
                "Whisper is used only for unsupported languages or when Parakeet returns no usable words."
            } else {
                "Standard Whisper is retained for cases the Intel OpenVINO path cannot process."
            },
        );
    }
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        if parakeet || intel_whisper {
            "Whisper fallback model"
        } else {
            "Whisper model"
        },
        if parakeet || intel_whisper {
            "This does not replace the primary engine; it is loaded only when compatibility fallback is needed."
        } else {
            "Turbo is the balanced default; larger models trade speed for detail."
        },
        SettingsSelectKind::WhisperModel,
        session,
    );
    let transcription_advanced =
        session.open_analysis_advanced == Some(AnalysisAdvancedSection::Transcription);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced transcription tuning",
        "Memory and search controls for this transcription stage.",
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
            if parakeet || intel_whisper {
                "Whisper fallback precision"
            } else {
                "Recognition precision"
            },
            "Whisper search breadth. Values are clamped between 1 and 16.",
            session.config.beam_size(),
            NumericSetting::BeamSize,
            UiAction::from(SettingsCommand::AdjustBeamSize(-1)),
            UiAction::from(SettingsCommand::AdjustBeamSize(1)),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            if parakeet {
                "Parakeet batch size"
            } else {
                "Whisper batch size"
            },
            "Lower this if this transcription engine runs out of GPU or system memory.",
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
        "03 · WORD TIMING",
        "Word timing & alignment",
        "Refines recognized or supplied lyrics into editable word timings.",
        align_backend_label(session.config.align_backend()),
        Some(analysis_stage_status(
            &status,
            alignment_model_target(session.config),
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
        "Alignment engine",
        if session.config.align_backend() == "mms_karaoke" {
            "MMS Karaoke targets known Japanese lyrics. Automatic transcription retains its compatible timing path."
        } else if parakeet {
            "Used for compatibility fallback and supplied lyrics; Parakeet's direct timestamps can skip this stage."
        } else {
            "Choose how recognized or supplied lyrics are refined into word timings."
        },
        SettingsSelectKind::AlignBackend,
        session,
    );

    spawn_settings_stage_header(
        parent,
        font.clone(),
        theme,
        "04 · MELODY",
        "Melody & pitch",
        "Detects sung pitch after vocal separation and creates editable notes.",
        pitch_model_label(session.config.pitch_model()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Pitch),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::from(SettingsCommand::SettingsTab(SettingsTab::Models)),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons,
        theme,
        "Pitch detection model",
        "Detects the sung fundamental frequency used to create note pitches.",
        SettingsSelectKind::PitchModel,
        session,
    );
    let pitch_advanced = session.open_analysis_advanced == Some(AnalysisAdvancedSection::Pitch);
    spawn_setting_row(
        parent,
        font.clone(),
        theme,
        "Advanced pitch tuning",
        "Controls how strongly detected vocals are filtered before notes are created.",
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
            "Vocal detection sensitivity",
            "Lower for soft singing; raise to remove more silence. Range: 0–60%.",
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
        "Controls when the four-stage pipeline starts; these are not model settings.",
    );
    spawn_switch_setting_row(
        parent,
        font.clone(),
        theme,
        "Auto-analyze",
        if session.config.auto_analyze() {
            "On · Unanalyzed songs are queued after a library scan."
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
        "Restore every stage and its advanced controls to the recommended starting values.",
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
