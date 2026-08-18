use super::*;
use crate::studio::*;

pub(crate) fn spawn_analysis_settings(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
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
        "01 · VOCAL SEPARATION",
        "Vocal separation",
        "Creates a clean vocal source before lyrics and pitch are analyzed.",
        separator_label(session.config.separator()),
        Some(analysis_stage_status(
            &status,
            Some(app_core::ModelDownloadTarget::Separator),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
        )),
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Separation engine",
        "Choose the model family that creates vocal and instrumental stems.",
        SettingsSelectKind::Separator,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Analysis vocal model",
        "Catalog model used for analysis vocals. Existing chart data changes only after re-analysis.",
        SettingsSelectKind::AudioVocalModel,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Denoise model",
        "Optional vocal denoise. Turn off to keep the raw extracted vocal.",
        SettingsSelectKind::AudioDenoise,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Dereverb model",
        "Optional vocal dereverb. Turn off independently of denoise.",
        SettingsSelectKind::AudioDereverb,
        session,
    );
    if audio_denoise_value(&session.config) != "none"
        && audio_dereverb_value(&session.config) != "none"
    {
        spawn_select_setting_row(
            parent,
            font.clone(),
            icons.clone(),
            theme,
            "Cleanup order",
            "Order of denoise and dereverb when both are enabled.",
            SettingsSelectKind::AudioCleanupOrder,
            session,
        );
    }
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Accompaniment model",
        "Independent high-quality accompaniment. Do not treat this as the complement of the vocal model.",
        SettingsSelectKind::AudioAccompanimentModel,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Karaoke model",
        "Optional karaoke accompaniment. Turning this off does not block charting.",
        SettingsSelectKind::AudioKaraokeModel,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "PyTorch backend",
        "Requested compute route for RoFormer and Demucs. Fallback to CPU is recorded if the whole model cannot run here.",
        SettingsSelectKind::AudioTorchBackend,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "ONNX / OpenVINO backend",
        "Requested route for MDX ONNX models such as Karaoke 2. OpenVINO runs in a helper process.",
        SettingsSelectKind::AudioOnnxBackend,
        session,
    );
    spawn_select_setting_row(
        parent,
        font.clone(),
        icons.clone(),
        theme,
        "Precision policy",
        "First release keeps FP32 until each model has CPU/XPU comparison tests.",
        SettingsSelectKind::AudioPrecisionPolicy,
        session,
    );
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
                UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Separation),
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
            UiAction::AdjustSeparatorSegmentSize(-32),
            UiAction::AdjustSeparatorSegmentSize(32),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer overlap",
            "More overlap can reduce chunk seams at the cost of additional processing. Range: 2–32.",
            session.config.separator_overlap(),
            NumericSetting::SeparatorOverlap,
            UiAction::AdjustSeparatorOverlap(-1),
            UiAction::AdjustSeparatorOverlap(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "RoFormer batch size",
            "Lower this first if separation runs out of system or accelerator memory. Range: 1–8.",
            session.config.separator_batch_size(),
            NumericSetting::SeparatorBatchSize,
            UiAction::AdjustSeparatorBatchSize(-1),
            UiAction::AdjustSeparatorBatchSize(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Output normalization",
            "Peak normalization applied by the separator before stems enter the lossless cache. Range: 1–100%.",
            session.config.separator_normalization_pct(),
            NumericSetting::SeparatorNormalization,
            UiAction::AdjustSeparatorNormalization(-1),
            UiAction::AdjustSeparatorNormalization(1),
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
            UiAction::AdjustDemucsShifts(-1),
            UiAction::AdjustDemucsShifts(1),
        );
        spawn_number_setting_row(
            parent,
            font.clone(),
            theme,
            "Demucs overlap",
            "Overlap between inference windows. Range: 1–95%.",
            session.config.demucs_overlap_pct(),
            NumericSetting::DemucsOverlap,
            UiAction::AdjustDemucsOverlap(-1),
            UiAction::AdjustDemucsOverlap(1),
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
        transcription_summary(&session.config),
        Some(analysis_stage_status(
            &status,
            Some(transcription_model_target(&session.config)),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
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
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Transcription),
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
            UiAction::AdjustBeamSize(-1),
            UiAction::AdjustBeamSize(1),
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
            UiAction::AdjustBatchSize(-1),
            UiAction::AdjustBatchSize(1),
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
            alignment_model_target(&session.config),
        )),
        Some((
            "Manage models…".to_string(),
            UiAction::SettingsTab(SettingsTab::Models),
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
            UiAction::SettingsTab(SettingsTab::Models),
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
            UiAction::ToggleAnalysisAdvanced(AnalysisAdvancedSection::Pitch),
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
            UiAction::AdjustVocalThreshold(-1),
            UiAction::AdjustVocalThreshold(1),
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
        UiAction::ToggleAutoAnalyze,
    );
    spawn_setting_row(
        parent,
        font,
        theme,
        "Analysis defaults",
        "Restore every stage and its advanced controls to the recommended starting values.",
        Some(("Restore defaults", UiAction::RestoreAnalysisDefaults)),
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
