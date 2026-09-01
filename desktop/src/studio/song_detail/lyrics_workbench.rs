use super::*;
use crate::studio::*;

fn spawn_timed_transcript_structure(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    config: &AppConfig,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
) {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&editor.initial_text) else {
        return;
    };
    let Some(segments) = value.get("segments").and_then(serde_json::Value::as_array) else {
        return;
    };
    parent
        .spawn((
            Node {
                width: percent(100),
                max_height: px(220),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(5),
                overflow: Overflow::scroll_y(),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(5)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.42)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|list| {
            spawn_text(
                list,
                font.clone(),
                "SEGMENTS AND WORD/TOKEN TIMING",
                8.0,
                theme.primary,
            );
            if !editor.waveform.peaks.is_empty() {
                spawn_text(
                    list,
                    font.clone(),
                    localized_message(
                        config,
                        UiMessage::AudioWaveform,
                        &[(
                            "{duration}",
                            &format_duration(editor.waveform.duration_secs),
                        )],
                    ),
                    7.5,
                    theme.muted_foreground,
                );
                list.spawn(Node {
                    width: percent(100),
                    height: px(46),
                    align_items: AlignItems::Center,
                    column_gap: px(1),
                    overflow: Overflow::clip(),
                    ..default()
                })
                .with_children(|waveform| {
                    let stride = (editor.waveform.peaks.len() / 160).max(1);
                    for (minimum, maximum) in
                        editor.waveform.peaks.iter().step_by(stride).take(160)
                    {
                        let amplitude = minimum.abs().max(maximum.abs()).clamp(0.02, 1.0);
                        waveform.spawn((
                            Node {
                                min_width: px(1),
                                flex_grow: 1.0,
                                height: px(2.0 + amplitude * 40.0),
                                ..default()
                            },
                            BackgroundColor(theme.primary.with_alpha(0.55)),
                        ));
                    }
                });
            } else {
                spawn_text(
                    list,
                    font.clone(),
                    "Loading transcript waveform…",
                    7.5,
                    theme.muted_foreground,
                );
            }
            let mut rendered_words = 0usize;
            for (segment_index, segment) in segments.iter().take(50).enumerate() {
                let start = segment.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0);
                let end = segment.get("end").and_then(|value| value.as_f64()).unwrap_or(start);
                let text = segment.get("text").and_then(|value| value.as_str()).unwrap_or("");
                list.spawn(Node {
                    width: percent(100),
                    align_items: AlignItems::Center,
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(5),
                    row_gap: px(3),
                    ..default()
                })
                .with_children(|row| {
                    spawn_text(row, font.clone(), format!("S{}", segment_index + 1), 8.0, theme.muted_foreground);
                    spawn_text_button(row, font.clone(), theme, format!("▶ {start:.3}s"), 8.0, UiAction::from(EditorCommand::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64)));
                    for (label, edge, delta) in [
                        ("start −10", TranscriptBoundaryEdge::Start, -10),
                        ("start +10", TranscriptBoundaryEdge::Start, 10),
                        ("end −10", TranscriptBoundaryEdge::End, -10),
                        ("end +10", TranscriptBoundaryEdge::End, 10),
                    ] {
                        spawn_text_button(row, font.clone(), theme, label, 7.5, UiAction::from(EditorCommand::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Segment(segment_index), edge, delta)));
                    }
                    spawn_transcript_drag_handle(row, font.clone(), theme, "drag start ↔", TranscriptBoundaryTarget::Segment(segment_index), TranscriptBoundaryEdge::Start);
                    spawn_transcript_drag_handle(row, font.clone(), theme, "drag end ↔", TranscriptBoundaryTarget::Segment(segment_index), TranscriptBoundaryEdge::End);
                    spawn_bounded_wrapped_text(row, font.clone(), format!("{start:.3}–{end:.3}  {text}"), 8.5, theme.foreground);
                });
                if let Some(words) = segment.get("words").and_then(serde_json::Value::as_array) {
                    for (word_index, word) in words.iter().enumerate() {
                        if rendered_words >= 200 { break; }
                        rendered_words += 1;
                        let start = word.get("start").and_then(|value| value.as_f64()).unwrap_or(0.0);
                        let end = word.get("end").and_then(|value| value.as_f64()).unwrap_or(start);
                        let text = word.get("word").or_else(|| word.get("text")).and_then(|value| value.as_str()).unwrap_or("");
                        list.spawn(Node {
                            width: percent(100), padding: UiRect::left(px(20)),
                            align_items: AlignItems::Center, flex_wrap: FlexWrap::Wrap,
                            column_gap: px(4), row_gap: px(3), ..default()
                        }).with_children(|row| {
                            spawn_text_button(row, font.clone(), theme, format!("▶ {start:.3}s"), 7.5, UiAction::from(EditorCommand::PreviewTranscriptAt(editor.file_hash.clone(), (start * 1000.0).round() as i64)));
                            for (label, edge, delta) in [
                                ("S−", TranscriptBoundaryEdge::Start, -10),
                                ("S+", TranscriptBoundaryEdge::Start, 10),
                                ("E−", TranscriptBoundaryEdge::End, -10),
                                ("E+", TranscriptBoundaryEdge::End, 10),
                            ] {
                                spawn_text_button(row, font.clone(), theme, label, 7.0, UiAction::from(EditorCommand::AdjustTranscriptBoundary(TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, edge, delta)));
                            }
                            spawn_transcript_drag_handle(row, font.clone(), theme, "drag S ↔", TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, TranscriptBoundaryEdge::Start);
                            spawn_transcript_drag_handle(row, font.clone(), theme, "drag E ↔", TranscriptBoundaryTarget::Word { segment: segment_index, word: word_index }, TranscriptBoundaryEdge::End);
                            spawn_text(row, font.clone(), format!("{start:.3}–{end:.3}  {text}"), 8.0, theme.foreground);
                        });
                    }
                }
            }
            if segments.len() > 50 || rendered_words >= 200 {
                spawn_wrapped_text(list, font, "The structured list is bounded for responsiveness; the lossless JSON editor below retains every segment, word, and extension field.", 8.0, theme.muted_foreground);
            }
        });
}

fn spawn_transcript_drag_handle(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &'static str,
    target: TranscriptBoundaryTarget,
    edge: TranscriptBoundaryEdge,
) {
    parent
        .spawn((
            Button,
            UiPointerApi(&["ui.pointer.transcript_boundary_drag"]),
            Node {
                padding: UiRect::axes(px(5), px(3)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(3)),
                ..default()
            },
            BackgroundColor(theme.primary.with_alpha(0.08)),
            BorderColor::all(theme.primary.with_alpha(0.35)),
            children![(
                Text::new(label),
                ui_text_font(font, 7.0),
                TextColor(theme.primary),
            )],
        ))
        .observe(
            move |mut drag: On<Pointer<Drag>>,
                  ui_scale: Res<UiScale>,
                  mut shell: ResMut<ShellState>,
                  mut dialogs: ResMut<DialogState>,
                  mut inputs: Query<&mut EditableText, With<LyricsEditorInput>>| {
                if drag.button != PointerButton::Primary || drag.delta.x.abs() < f32::EPSILON {
                    return;
                }
                drag.propagate(false);
                let Ok(mut input) = inputs.single_mut() else {
                    return;
                };
                let current = input.value().to_string();
                let result = serde_json::from_str::<serde_json::Value>(&current)
                    .map_err(|error| format!("Invalid JSON: {error}"))
                    .and_then(|mut value| {
                        adjust_transcript_boundary_value(
                            &mut value,
                            target,
                            edge,
                            f64::from(drag.delta.x / ui_scale.0) * 0.005,
                            &shell.config,
                        )?;
                        let rendered = serde_json::to_string_pretty(&value).unwrap_or_default();
                        let editor = dialogs
                            .lyrics_editor
                            .as_mut()
                            .ok_or_else(|| "TimedTranscript editor is closed.".to_string())?;
                        let mut draft = editor
                            .artifact_draft
                            .clone()
                            .ok_or_else(|| "TimedTranscript draft is unavailable.".to_string())?;
                        draft.replace_json(value)?;
                        editor.initial_text = rendered.clone();
                        editor.artifact_draft = Some(draft);
                        input.editor_mut().set_text(&rendered);
                        Ok(())
                    });
                shell.notice = result.err();
            },
        )
        .observe(
            |mut release: On<Pointer<DragEnd>>, mut invalidated: ResMut<UiInvalidated>| {
                release.propagate(false);
                invalidated.invalidate(UiDirtyRegion::Library);
            },
        )
        .observe(
            |mut cancel: On<Pointer<Cancel>>, mut invalidated: ResMut<UiInvalidated>| {
                cancel.propagate(false);
                invalidated.invalidate(UiDirtyRegion::Library);
            },
        );
}

fn lyrics_provider_status(
    editor: &NativeLyricsEditor,
    provider: app_core::LyricsProvider,
) -> (String, bool) {
    if let Some(failure) = editor
        .provider_errors
        .iter()
        .find(|failure| failure.provider == provider)
    {
        return (
            format!("{} · ERROR · {}", provider.display_name(), failure.message),
            true,
        );
    }
    if editor.searching {
        return (format!("{} · SEARCHING", provider.display_name()), false);
    }
    let count = editor
        .candidates
        .iter()
        .filter(|candidate| candidate.provider == provider)
        .count();
    (
        format!(
            "{} · {count} candidate{}",
            provider.display_name(),
            if count == 1 { "" } else { "s" }
        ),
        false,
    )
}

fn lyrics_candidate_badges(candidate: &app_core::LyricsCandidate) -> String {
    let mut badges = Vec::new();
    if candidate.has_timed_lyrics {
        badges.push("TIMED");
    }
    if candidate.has_translation {
        badges.push("TRANSLATION");
    }
    if candidate.has_romanization {
        badges.push("ROMANIZATION");
    }
    if badges.is_empty() {
        badges.push("PLAIN");
    }
    badges.join(" / ")
}

fn spawn_lyrics_provider_status_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
    provider: app_core::LyricsProvider,
) {
    let (status, failed) = lyrics_provider_status(editor, provider);
    parent
        .spawn((
            Node {
                width: percent(49),
                min_height: px(34),
                padding: UiRect::axes(px(9), px(6)),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.34)),
            BorderColor::all(if failed {
                theme.destructive.with_alpha(0.58)
            } else {
                theme.border.with_alpha(0.62)
            }),
        ))
        .with_children(|card| {
            spawn_wrapped_text(
                card,
                font,
                status,
                8.0,
                if failed {
                    theme.destructive
                } else if editor.searching {
                    theme.primary
                } else {
                    theme.muted_foreground
                },
            );
        });
}

fn spawn_lyrics_candidate_card(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    candidate_index: usize,
    candidate: &app_core::LyricsCandidate,
    fetching_candidate: Option<usize>,
) {
    parent
        .spawn((
            Node {
                width: percent(49),
                min_height: px(96),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(9)),
                row_gap: px(5),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(7)),
                ..default()
            },
            BackgroundColor(theme.background.with_alpha(0.38)),
            BorderColor::all(theme.border.with_alpha(0.62)),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                column_gap: px(7),
                ..default()
            })
            .with_children(|header| {
                spawn_text(
                    header,
                    font.clone(),
                    candidate.provider.display_name(),
                    8.0,
                    theme.primary,
                );
                header.spawn(Node {
                    flex_grow: 1.0,
                    ..default()
                });
                spawn_text(
                    header,
                    font.clone(),
                    lyrics_candidate_badges(candidate),
                    7.0,
                    theme.muted_foreground,
                );
            });
            spawn_wrapped_text(
                card,
                font.clone(),
                candidate.track_name.clone(),
                10.0,
                theme.foreground,
            );
            spawn_wrapped_text(
                card,
                font.clone(),
                format!(
                    "{}{} · {}{}",
                    candidate.artist_name,
                    if candidate.album_name.trim().is_empty() {
                        String::new()
                    } else {
                        format!(" · {}", candidate.album_name)
                    },
                    format_duration(candidate.duration_secs),
                    if candidate.loaded {
                        format!(" · {} lines · loaded", candidate.lines.len())
                    } else {
                        " · metadata only".to_string()
                    }
                ),
                8.0,
                theme.muted_foreground,
            );
            card.spawn(Node {
                width: percent(100),
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(5),
                row_gap: px(5),
                ..default()
            })
            .with_children(|actions| {
                if !candidate.loaded {
                    spawn_action_button(
                        actions,
                        font.clone(),
                        theme,
                        if fetching_candidate == Some(candidate_index) {
                            "Loading…"
                        } else {
                            "Load"
                        },
                        UiAction::from(EditorCommand::LoadLyricsCandidate(candidate_index)),
                    );
                    return;
                }
                if !candidate.lines.is_empty() || candidate.synced_lyrics.is_some() {
                    spawn_text_button(
                        actions,
                        font.clone(),
                        theme,
                        "Plain",
                        8.5,
                        UiAction::from(EditorCommand::UseLyricsCandidate(
                            candidate_index,
                            LyricsCandidateUseMode::Plain,
                        )),
                    );
                }
                if candidate.synced_lyrics.is_some() {
                    spawn_action_button(
                        actions,
                        font.clone(),
                        theme,
                        "Timed LRC",
                        UiAction::from(EditorCommand::UseLyricsCandidate(
                            candidate_index,
                            LyricsCandidateUseMode::TimedLrc,
                        )),
                    );
                }
                if candidate.translation.is_some() {
                    spawn_text_button(
                        actions,
                        font.clone(),
                        theme,
                        "Translation",
                        8.5,
                        UiAction::from(EditorCommand::UseLyricsCandidate(
                            candidate_index,
                            LyricsCandidateUseMode::Translation,
                        )),
                    );
                }
                if candidate.romanization.is_some() {
                    spawn_text_button(
                        actions,
                        font.clone(),
                        theme,
                        "Romanization",
                        8.5,
                        UiAction::from(EditorCommand::UseLyricsCandidate(
                            candidate_index,
                            LyricsCandidateUseMode::Romanization,
                        )),
                    );
                }
            });
        });
}

pub(crate) fn spawn_lyrics_workbench_page(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|page| {
            let Some(editor) = session.lyrics_editor.as_ref() else {
                page.spawn(Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                })
                .with_children(|empty| {
                    empty
                        .spawn(Node {
                            max_width: px(560),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|message| {
                            spawn_text(
                                message,
                                font.clone(),
                                "Lyrics Workbench unavailable",
                                18.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                message,
                                font.clone(),
                                "Return to a song or the chart editor and open Lyrics Workbench again.",
                                10.0,
                                theme.muted_foreground,
                            );
                            spawn_action_button(
                                message,
                                font.clone(),
                                theme,
                                "Back",
                                UiAction::from(AppCommand::Back),
                            );
                        });
                });
                return;
            };

            spawn_lyrics_workbench_content(
                page,
                font.clone(),
                session.config,
                theme,
                editor,
                session.notice.as_deref(),
            );
            spawn_lyrics_workbench_footer(page, font, theme);
        });
}

pub(crate) fn handle_lyrics_workbench_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    shell: Res<ShellState>,
    mut content: Query<(&ComputedNode, &mut ScrollPosition), With<LyricsWorkbenchContent>>,
) {
    if shell.route != StudioRoute::LyricsWorkbench {
        return;
    }
    let Ok((node, mut position)) = content.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        delta -= match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => event.y * 38.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => event.y,
        };
    }
    if delta.abs() <= f32::EPSILON {
        return;
    }
    let max_scroll = (node.content_size().y - node.size().y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max_scroll);
}

fn spawn_lyrics_workbench_content(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    config: &AppConfig,
    theme: &StudioTheme,
    editor: &NativeLyricsEditor,
    notice: Option<&str>,
) {
    parent
        .spawn((
            LyricsWorkbenchContent,
            Node {
                width: percent(100),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(28), px(18)),
                row_gap: px(8),
                overflow: Overflow::scroll_y(),
                ..default()
            },
        ))
        .with_children(|dialog| {
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            align_items: AlignItems::Center,
                            column_gap: px(8),
                            ..default()
                        })
                        .with_children(|header| {
                            header
                                .spawn(Node {
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(2),
                                    ..default()
                                })
                                .with_children(|titles| {
                                    spawn_text(
                                        titles,
                                        font.clone(),
                                        "LYRICS WORKBENCH",
                                        8.0,
                                        theme.primary,
                                    );
                                    spawn_text(
                                        titles,
                                        font.clone(),
                                        if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                                            "TimedTranscript JSON"
                                        } else if editor.mode == LyricsInputMode::TimedLrc {
                                            "Timed LRC"
                                        } else {
                                            "Plain lyrics"
                                        },
                                        18.0,
                                        theme.foreground,
                                    );
                                });
                            header.spawn(Node {
                                flex_grow: 1.0,
                                ..default()
                            });
                            spawn_text_button(
                                header,
                                font.clone(),
                                theme,
                                "Close",
                                9.0,
                                UiAction::from(EditorCommand::CloseLyricsEditor),
                            );
                        });

                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                            "Edit the lossless segment/word structure. Start/end values must remain ordered and every word must remain inside its segment."
                        } else {
                            "Search four lyric sources, load the needed representation, then review it in the native Unicode editor. Ctrl+A/C/X/V and multiline copy/paste are supported by the focused text field."
                        },
                        9.0,
                        theme.muted_foreground,
                    );

                    if editor.artifact_draft.is_none() {
                        spawn_text(dialog, font.clone(), "SEARCH TITLE", 7.5, theme.primary);
                        dialog
                            .spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                column_gap: px(7),
                                ..default()
                            })
                            .with_children(|search| {
                                search.spawn((
                                    LyricsSearchTitleInput,
                                    EditableText {
                                        visible_width: Some(58.0),
                                        max_characters: Some(300),
                                        ..EditableText::new(&editor.search_title)
                                    },
                                    Node {
                                        min_width: px(0),
                                        flex_grow: 1.0,
                                        padding: UiRect::axes(px(9), px(6)),
                                        border: UiRect::all(px(1)),
                                        border_radius: BorderRadius::all(px(5)),
                                        ..default()
                                    },
                                    ui_text_font(font.clone(), 10.0),
                                    TextColor(theme.foreground),
                                    TextLayout::no_wrap(),
                                    TextCursorStyle {
                                        color: theme.primary,
                                        selected_text_color: Some(theme.primary_foreground),
                                        ..default()
                                    },
                                    BackgroundColor(theme.background.with_alpha(0.58)),
                                    BorderColor::all(theme.border.with_alpha(0.72)),
                                    TabIndex(0),
                                ));
                                spawn_action_button(
                                    search,
                                    font.clone(),
                                    theme,
                                    if editor.searching {
                                        "Searching…"
                                    } else {
                                        "Search all sources"
                                    },
                                    UiAction::from(EditorCommand::SearchAllLyricsSources),
                                );
                            });

                        dialog
                            .spawn(Node {
                                width: percent(100),
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(8),
                                row_gap: px(6),
                                ..default()
                            })
                            .with_children(|providers| {
                                for provider in app_core::LyricsProvider::ALL {
                                    spawn_lyrics_provider_status_card(
                                        providers,
                                        font.clone(),
                                        theme,
                                        editor,
                                        provider,
                                    );
                                }
                            });

                        dialog
                            .spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(7),
                                row_gap: px(6),
                                ..default()
                            })
                            .with_children(|options| {
                                spawn_text_button(
                                    options,
                                    font.clone(),
                                    theme,
                                    if editor.mode == LyricsInputMode::TimedLrc {
                                        "Use plain editor"
                                    } else {
                                        "Use timed LRC editor"
                                    },
                                    9.0,
                                    UiAction::from(EditorCommand::ToggleLyricsInputMode),
                                );
                                spawn_text_button(
                                    options,
                                    font.clone(),
                                    theme,
                                    "Extract lyrics",
                                    9.0,
                                    UiAction::from(EditorCommand::ExtractLyrics),
                                );
                            });

                        if editor.searching || !editor.candidates.is_empty() {
                            let start = editor.candidate_page * LYRICS_CANDIDATE_SLOTS;
                            let end = (start + LYRICS_CANDIDATE_SLOTS)
                                .min(editor.candidates.len());
                            let total_pages = editor
                                .candidates
                                .len()
                                .saturating_add(LYRICS_CANDIDATE_SLOTS - 1)
                                / LYRICS_CANDIDATE_SLOTS;
                            dialog
                                .spawn(Node {
                                    width: percent(100),
                                    align_items: AlignItems::Center,
                                    column_gap: px(7),
                                    ..default()
                                })
                                .with_children(|results_header| {
                                    spawn_text(
                                        results_header,
                                        font.clone(),
                                        if editor.searching {
                                            "SEARCH RESULTS · loading"
                                        } else {
                                            "SEARCH RESULTS"
                                        },
                                        7.5,
                                        theme.primary,
                                    );
                                    results_header.spawn(Node {
                                        flex_grow: 1.0,
                                        ..default()
                                    });
                                    if total_pages > 1 {
                                        spawn_text_button(
                                            results_header,
                                            font.clone(),
                                            theme,
                                            "Previous",
                                            8.0,
                                            UiAction::from(
                                                EditorCommand::PreviousLyricsCandidatePage,
                                            ),
                                        );
                                        spawn_text(
                                            results_header,
                                            font.clone(),
                                            format!(
                                                "{} / {}",
                                                editor.candidate_page + 1,
                                                total_pages
                                            ),
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                        spawn_text_button(
                                            results_header,
                                            font.clone(),
                                            theme,
                                            "Next",
                                            8.0,
                                            UiAction::from(EditorCommand::NextLyricsCandidatePage),
                                        );
                                    }
                                });
                            dialog
                                .spawn(Node {
                                    width: percent(100),
                                    flex_wrap: FlexWrap::Wrap,
                                    column_gap: px(8),
                                    row_gap: px(7),
                                    ..default()
                                })
                                .with_children(|candidates| {
                                    for candidate_index in start..end {
                                        spawn_lyrics_candidate_card(
                                            candidates,
                                            font.clone(),
                                            theme,
                                            candidate_index,
                                            &editor.candidates[candidate_index],
                                            editor.fetching_candidate,
                                        );
                                    }
                                });
                        }
                    }

                    if editor.mode == LyricsInputMode::StructuredTimedTranscript {
                        spawn_timed_transcript_structure(
                            dialog,
                            font.clone(),
                            config,
                            theme,
                            editor,
                        );
                    } else {
                        dialog
                            .spawn(Node {
                                width: percent(100),
                                align_items: AlignItems::Center,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: px(6),
                                row_gap: px(6),
                                ..default()
                            })
                            .with_children(|tools| {
                                spawn_text(tools, font.clone(), "EDITOR", 7.5, theme.primary);
                                spawn_text_button(
                                    tools,
                                    font.clone(),
                                    theme,
                                    "Normalize",
                                    8.5,
                                    UiAction::from(EditorCommand::NormalizeLyricsEditor),
                                );
                                spawn_text_button(
                                    tools,
                                    font.clone(),
                                    theme,
                                    "Strip timing",
                                    8.5,
                                    UiAction::from(EditorCommand::StripLyricsTiming),
                                );
                                spawn_text_button(
                                    tools,
                                    font.clone(),
                                    theme,
                                    "Clear",
                                    8.5,
                                    UiAction::from(EditorCommand::ClearLyricsEditor),
                                );
                            });
                    }

                    dialog.spawn((
                        LyricsEditorInput,
                        EditableText {
                            visible_lines: Some(14.0),
                            visible_width: Some(96.0),
                            allow_newlines: true,
                            max_characters: Some(100_000),
                            ..EditableText::new(&editor.initial_text)
                        },
                        Node {
                            width: percent(100),
                            min_height: px(150),
                            flex_grow: 1.0,
                            padding: UiRect::all(px(10)),
                            overflow: Overflow::scroll(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(5)),
                            ..default()
                        },
                        ui_text_font(font.clone(), 10.5),
                        TextColor(theme.foreground),
                        TextLayout {
                            linebreak: bevy::text::LineBreak::WordOrCharacter,
                            ..default()
                        },
                        TextCursorStyle {
                            color: theme.primary,
                            selected_text_color: Some(theme.primary_foreground),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.65)),
                        BorderColor::all(theme.border.with_alpha(0.72)),
                        TabIndex(1),
                        AutoFocus,
                    ));

                    if let Some(notice) = notice {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            notice,
                            8.5,
                            theme.destructive,
                        );
                    }
                    if let Some(draft) = editor.artifact_draft.as_ref()
                        && let Ok(impact) = app_core::preview_artifact_edit_impact(draft)
                    {
                        spawn_wrapped_text(
                            dialog,
                            font.clone(),
                            format!(
                                "Impact preview · {} downstream node(s) · Authored Chart preserved{}",
                                impact.affected_nodes.len(),
                                if impact.export_may_need_regeneration {
                                    " · exports may need regeneration"
                                } else {
                                    ""
                                }
                            ),
                            8.5,
                            theme.muted_foreground,
                        );
                    }

        });
}

fn spawn_lyrics_workbench_footer(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                flex_shrink: 0.0,
                justify_content: JustifyContent::FlexEnd,
                align_items: AlignItems::Center,
                flex_wrap: FlexWrap::Wrap,
                padding: UiRect::axes(px(28), px(12)),
                column_gap: px(7),
                row_gap: px(6),
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.96)),
            BorderColor::all(theme.border.with_alpha(0.72)),
        ))
        .with_children(|actions| {
            spawn_text_button(
                actions,
                font.clone(),
                theme,
                "Cancel",
                9.0,
                UiAction::from(EditorCommand::CloseLyricsEditor),
            );
            spawn_text_button(
                actions,
                font.clone(),
                theme,
                "Save",
                9.0,
                UiAction::from(EditorCommand::SaveLyricsEditor),
            );
            spawn_action_button(
                actions,
                font,
                theme,
                "Save + Align",
                UiAction::from(EditorCommand::SaveLyricsEditorAndAlign),
            );
        });
}
