use super::*;
use crate::studio::*;

pub(crate) fn spawn_cache_delete_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
) {
    let title = app_core::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .map(|song| song.title)
        .unwrap_or_else(|| "this song".to_string());
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(90),
        children![(
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Delete generated song data?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "Generated stems, transcripts, pitch data, and derived variants for “{title}” will be removed. The source song remains untouched."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmDeleteSongCache,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Delete generated data"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

/// Phase 5 §5.4 "Compare / Merge / Replace" confirmation modal. Fetches a
/// fresh `candidate_chart_status` at render time (same pattern as
/// `spawn_cache_delete_confirmation` re-fetching the song title) rather than
/// threading the summary through `pending_chart_replace`, so the numbers
/// shown are never stale relative to whatever analysis has run since the
/// button was clicked.
pub(crate) fn spawn_chart_replace_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
) {
    let title = app_core::load_song_by_hash(file_hash)
        .ok()
        .flatten()
        .map(|song| song.title)
        .unwrap_or_else(|| "this song".to_string());
    let pinned = app_core::authored_chart_is_pinned(file_hash);
    let body = if pinned {
        format!(
            "The authored chart for “{title}” is pinned. Unpin that revision before replacing it with the candidate. Keep Authored leaves the saved chart unchanged."
        )
    } else {
        match app_core::candidate_chart_status(file_hash) {
            app_core::CandidateChartStatus::CandidateAvailable(summary) => {
                let mut changed = Vec::new();
                if summary.lyrics_changed {
                    changed.push("lyrics timing");
                }
                if summary.pitch_evidence_changed {
                    changed.push("pitch evidence");
                }
                format!(
                    "The current candidate analysis for “{title}” has {} note(s) across {} phrase(s), \
                     versus {} note(s) across {} phrase(s) in your saved chart. Updated: {}. \
                     Replacing discards your edits; the next time you open the editor it rebuilds \
                     from this candidate instead.",
                    summary.candidate_note_count,
                    summary.candidate_phrase_count,
                    summary.authored_note_count,
                    summary.authored_phrase_count,
                    changed.join(" & "),
                )
            }
            _ => format!(
                "No candidate analysis is currently available for “{title}”. Replacing would discard \
                 your saved chart and rebuild from whatever analysis output already exists on disk."
            ),
        }
    };
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(90),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(480),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(
                        dialog,
                        font.clone(),
                        if pinned {
                            "Authored chart is pinned"
                        } else {
                            "Replace authored chart with the candidate?"
                        },
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(dialog, font.clone(), body, 10.0, theme.muted_foreground);
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Keep my chart",
                                10.0,
                                UiAction::KeepAuthoredChart,
                            );
                            if pinned {
                                spawn_text(
                                    actions,
                                    font,
                                    "Unpin required",
                                    10.0,
                                    theme.editor_warning,
                                );
                            } else {
                                spawn_text_button(
                                    actions,
                                    font,
                                    theme,
                                    "Replace with candidate",
                                    10.0,
                                    UiAction::ConfirmReplaceAuthoredChart,
                                );
                            }
                        });
                });
        });
}

pub(crate) fn spawn_artifact_delete_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    revision: &app_core::ArtifactRevision,
) {
    let file_name = revision
        .path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| revision.id.clone());
    let impact_copy =
        app_core::preview_artifact_downstream_impact(&artifact_ref_from_revision(revision))
            .map(|impact| {
                format!(
                    "Impact preview · {} downstream node(s) · Authored Chart preserved{}.",
                    impact.affected_nodes.len(),
                    if impact.export_may_need_regeneration {
                        " · exports may need regeneration"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|error| format!("Impact preview unavailable: {error}"));
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        // Above the activity center overlay (ZIndex 100) that this
        // confirmation is always triggered from.
        ZIndex(110),
        children![(
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Delete this artifact revision?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "“{file_name}” will be removed from the cache and its revision history. This does not touch the source song."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Text::new(impact_copy),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelDeleteArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmDeleteArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Delete revision"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

/// Phase 6 `invalidate_artifact_revision` / Phase 7 §7.6 "Invalidate".
/// Same modal shape as `spawn_artifact_delete_confirmation`, but the copy
/// makes explicit that (unlike Delete) the file and its revision history
/// both survive -- only the "trustworthy/Active-eligible" status changes.
pub(crate) fn spawn_artifact_invalidate_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    revision: &app_core::ArtifactRevision,
) {
    let file_name = revision
        .path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| revision.id.clone());
    let impact_copy =
        app_core::preview_artifact_downstream_impact(&artifact_ref_from_revision(revision))
            .map(|impact| {
                format!(
                    "Impact preview · {} downstream node(s) · Authored Chart preserved{}.",
                    impact.affected_nodes.len(),
                    if impact.export_may_need_regeneration {
                        " · exports may need regeneration"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|error| format!("Impact preview unavailable: {error}"));
    parent.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.78)),
        ZIndex(110),
        children![(
            Node {
                width: px(460),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(24)),
                row_gap: px(11),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card),
            BorderColor::all(theme.border),
            children![
                (
                    Text::new("Invalidate this artifact revision?"),
                    ui_text_font(font.clone(), 17.0),
                    TextColor(theme.foreground),
                ),
                (
                    Text::new(format!(
                        "“{file_name}” will be marked stale/wrong and, if it's currently Active, stop being the one this song uses. The file and its revision history are kept -- this doesn't delete anything."
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Text::new(impact_copy),
                    ui_text_font(font.clone(), 9.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::default(),
                ),
                (
                    Node {
                        width: percent(100),
                        justify_content: JustifyContent::FlexEnd,
                        column_gap: px(8),
                        ..default()
                    },
                    children![
                        (
                            Button,
                            UiAction::CancelInvalidateArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            children![(
                                Text::new("Cancel"),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(theme.muted_foreground),
                            )],
                        ),
                        (
                            Button,
                            UiAction::ConfirmInvalidateArtifactRevision,
                            Node {
                                padding: UiRect::axes(px(13), px(8)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.destructive.with_alpha(0.18)),
                            children![(
                                Text::new("Invalidate revision"),
                                ui_text_font(font, 10.0),
                                TextColor(theme.destructive),
                            )],
                        )
                    ],
                )
            ],
        )],
    ));
}

pub(crate) fn spawn_artifact_active_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    revision: &app_core::ArtifactRevision,
) {
    let impact =
        app_core::preview_artifact_downstream_impact(&artifact_ref_from_revision(revision))
            .map(|impact| {
                format!(
                    "Will affect {} downstream node(s). Authored Chart remains preserved{}.",
                    impact.affected_nodes.len(),
                    if impact.export_may_need_regeneration {
                        " Exports may need regeneration"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|error| format!("Impact preview unavailable: {error}"));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(110),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(470),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Set this revision Active?",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(dialog, font.clone(), impact, 10.0, theme.muted_foreground);
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CancelSetActiveArtifactRevision,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Set Active",
                                UiAction::ConfirmSetActiveArtifactRevision,
                            );
                        });
                });
        });
}

pub(crate) fn spawn_intermediate_capture_confirmation(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    file_hash: &str,
) {
    let existing = app_core::intermediate_capture_request(file_hash)
        .ok()
        .flatten();
    let estimate = app_core::load_active_artifact(file_hash, app_core::ArtifactKind::VocalStem)
        .and_then(|revision| {
            app_core::preview_artifact(&artifact_ref_from_revision(&revision)).ok()
        })
        .and_then(|preview| match preview {
            app_core::ArtifactPreview::AudioMetadata { duration_ms, .. } => duration_ms,
            _ => None,
        })
        .map(|duration_ms| {
            let pcm_upper_bytes = duration_ms.saturating_mul(48);
            format!(
                "Estimated upper bound before FLAC compression: {:.1} MiB.",
                pcm_upper_bytes as f64 / (1024.0 * 1024.0)
            )
        })
        .unwrap_or_else(|| "Disk use will be measured after preprocessing.".to_string());
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(112),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(500),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Capture preprocessed audio?",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "This normally ephemeral audio may contain isolated vocals. It is stored losslessly in the private artifact cache and appears in Node I/O and Lineage.",
                        10.0,
                        theme.muted_foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        estimate,
                        10.0,
                        theme.muted_foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "Ordinary runs remain unchanged.",
                        10.0,
                        theme.muted_foreground,
                    );
                    if let Some(existing) = existing.as_ref() {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            if existing.persistent {
                                "Current setting: capture every run."
                            } else {
                                "Current setting: capture once on the next successful run."
                            },
                            10.0,
                            theme.primary,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            if existing.is_some() {
                                spawn_text_button(
                                    actions,
                                    font.clone(),
                                    theme,
                                    "Disable",
                                    10.0,
                                    UiAction::ConfirmDisableIntermediateCapture,
                                );
                            }
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CancelCaptureIntermediate,
                            );
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Every run",
                                10.0,
                                UiAction::ConfirmCaptureIntermediatePersistent,
                            );
                            spawn_action_button(
                                actions,
                                font.clone(),
                                theme,
                                "Capture once",
                                UiAction::ConfirmCaptureIntermediateOnce,
                            );
                        });
                });
        });
}

pub(crate) fn spawn_language_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLanguageEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0),
                right: px(0),
                top: px(0),
                bottom: px(0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.78)),
            ZIndex(92),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: px(470),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(24)),
                        row_gap: px(11),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "LANGUAGE", 8.0, theme.primary);
                    spawn_text(
                        dialog,
                        font.clone(),
                        "Change analysis language",
                        17.0,
                        theme.foreground,
                    );
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "Choose a supported language or let the analyzer detect it. The chosen action runs only after saving.",
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog
                        .spawn((
                            Button,
                            UiAction::ToggleLanguagePicker,
                            Node {
                                width: percent(100),
                                height: px(40),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(11)),
                                column_gap: px(8),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(5)),
                                ..default()
                            },
                            BackgroundColor(theme.background.with_alpha(0.65)),
                            BorderColor::all(if editor.picker_open {
                                theme.primary.with_alpha(0.64)
                            } else {
                                theme.border.with_alpha(0.72)
                            }),
                        ))
                        .with_children(|selector| {
                            spawn_text(
                                selector,
                                font.clone(),
                                analysis_language_label(&editor.initial_language),
                                11.0,
                                theme.foreground,
                            );
                            selector.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text(
                                selector,
                                font.clone(),
                                editor.initial_language.to_ascii_uppercase(),
                                8.0,
                                theme.muted_foreground,
                            );
                            spawn_text(
                                selector,
                                font.clone(),
                                if editor.picker_open { "^" } else { "v" },
                                9.0,
                                theme.primary,
                            );
                        });
                    if editor.picker_open {
                        dialog
                            .spawn((
                                ScrollPosition::default(),
                                Node {
                                    width: percent(100),
                                    max_height: px(238),
                                    flex_direction: FlexDirection::Column,
                                    padding: UiRect::all(px(5)),
                                    row_gap: px(2),
                                    overflow: Overflow::scroll_y(),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(5)),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.82)),
                                BorderColor::all(theme.border.with_alpha(0.72)),
                            ))
                            .with_children(|options| {
                                for (code, label) in ANALYSIS_LANGUAGE_OPTIONS {
                                    let selected = editor.initial_language == *code;
                                    options
                                        .spawn((
                                            Button,
                                            UiAction::SelectAnalysisLanguage((*code).into()),
                                            Node {
                                                width: percent(100),
                                                min_height: px(30),
                                                align_items: AlignItems::Center,
                                                padding: UiRect::horizontal(px(9)),
                                                column_gap: px(8),
                                                border_radius: BorderRadius::all(px(4)),
                                                ..default()
                                            },
                                            BackgroundColor(if selected {
                                                theme.primary.with_alpha(0.13)
                                            } else {
                                                Color::NONE
                                            }),
                                        ))
                                        .with_children(|option| {
                                            spawn_text(
                                                option,
                                                font.clone(),
                                                *label,
                                                9.0,
                                                if selected {
                                                    theme.foreground
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                            option.spawn(Node {
                                                flex_grow: 1.0,
                                                ..default()
                                            });
                                            spawn_text(
                                                option,
                                                font.clone(),
                                                code.to_ascii_uppercase(),
                                                8.0,
                                                if selected {
                                                    theme.primary
                                                } else {
                                                    theme.muted_foreground
                                                },
                                            );
                                        });
                                }
                            });
                    }
                    spawn_text_button(
                        dialog,
                        font.clone(),
                        theme,
                        if editor.force_transcribe {
                            "Action: transcribe vocals again"
                        } else {
                            "Action: realign current lyrics"
                        },
                        10.0,
                        UiAction::ToggleLanguageReprocess,
                    );
                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            9.0,
                            theme.destructive,
                        );
                    }
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            justify_content: JustifyContent::FlexEnd,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|actions| {
                            spawn_text_button(
                                actions,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::CloseLanguageEditor,
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save & reprocess",
                                UiAction::SaveLanguageEditor,
                            );
                        });
                });
        });
}
