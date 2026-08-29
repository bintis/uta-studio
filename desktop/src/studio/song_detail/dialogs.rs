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
                            UiAction::from(AnalysisCommand::CancelDeleteSongCache),
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
                            UiAction::from(AnalysisCommand::ConfirmDeleteSongCache),
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

pub(crate) fn spawn_chart_delete_confirmation(
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
                    spawn_text(dialog, font.clone(), "Delete chart?", 17.0, theme.foreground);
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        if pinned {
                            "This authored chart is pinned. Unpin the artifact revision before deleting it. Source media, CandidateChart and analysis evidence are retained."
                                .to_string()
                        } else {
                            format!(
                                "Delete the authored chart for {title}? Source media, CandidateChart and analysis evidence are retained. A retained revision can be reactivated later in Artifact Workbench."
                            )
                        },
                        10.0,
                        theme.muted_foreground,
                    );
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
                                UiAction::from(AnalysisCommand::CancelDeleteAuthoredChart),
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
                                    "Delete chart",
                                    10.0,
                                    UiAction::from(AnalysisCommand::ConfirmDeleteAuthoredChart),
                                );
                            }
                        });
                });
        });
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
                                UiAction::from(AnalysisCommand::KeepAuthoredChart),
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
                                    UiAction::from(AnalysisCommand::ConfirmReplaceAuthoredChart),
                                );
                            }
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
                            UiAction::from(EditorCommand::ToggleLanguagePicker),
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
                                            UiAction::from(EditorCommand::SelectAnalysisLanguage((*code).into())),
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
                        UiAction::from(EditorCommand::ToggleLanguageReprocess),
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
                                UiAction::from(EditorCommand::CloseLanguageEditor),
                            );
                            spawn_action_button(
                                actions,
                                font,
                                theme,
                                "Save & reprocess",
                                UiAction::from(EditorCommand::SaveLanguageEditor),
                            );
                        });
                });
        });
}
