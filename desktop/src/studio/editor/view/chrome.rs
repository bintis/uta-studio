//! Editor rendering: chrome, dock, timeline, lyric lane, and playhead.

use super::*;
use crate::studio::*;

pub(crate) fn spawn_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSessionView<'_>,
    theme: &StudioTheme,
) {
    let Some(editor) = session.editor.as_ref() else {
        parent
            .spawn((
                Node {
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    ..default()
                },
                BackgroundColor(theme.background),
            ))
            .with_children(|page| {
                page.spawn((
                    Node {
                        width: percent(100),
                        min_height: px(WORKSPACE_TOP_BAR_MIN_HEIGHT),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.72)),
                    BorderColor::all(theme.border.with_alpha(0.4)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons,
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::from(AppCommand::Back),
                        false,
                        false,
                        34.0,
                    );
                    spawn_text(toolbar, font.clone(), "Chart editor", 12.0, theme.foreground);
                });
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
                            width: px(460),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            row_gap: px(10),
                            ..default()
                        })
                        .with_children(|message| {
                            let loading = session.editor_load_job.receiver.is_some();
                            spawn_text(
                                message,
                                font.clone(),
                                if loading {
                                    "Preparing chart editor…"
                                } else {
                                    "Chart editor unavailable"
                                },
                                18.0,
                                theme.foreground,
                            );
                            spawn_wrapped_text(
                                message,
                                font.clone(),
                                session.notice.as_deref().unwrap_or(
                                    "The chart editor could not be loaded. Return to the song and review its analysis status.",
                                ),
                                11.0,
                                theme.muted_foreground,
                            );
                        });
                });
            });
        return;
    };
    let song = session.selected_song();
    let notes = chart_notes(&editor.document);
    let ghosts = other_track_notes(&editor.document);
    let tracks_visible = !editor.tracks_hidden;
    let lyrics = chart_lyrics(&editor.document);
    // The lyric or note(s) the current selection is bound to, so its match
    // can be highlighted — the format ties a note's pitch and its lyric
    // together, and clicking either one should show the other. A
    // syllable held across a pitch change spans more than one note (see
    // `ChartLyricView::continuation_notes`), so all of them highlight
    // together rather than just the one carrying the lyric text.
    let selected_lyric = if let Some(word) = editor.selected_word {
        lyrics
            .iter()
            .find(|lyric| lyric.segment == word.segment && lyric.word == word.word && lyric.guided)
    } else {
        editor.selected_note.and_then(|note_index| {
            lyrics.iter().find(|lyric| {
                lyric.note == note_index || lyric.continuation_notes.contains(&note_index)
            })
        })
    };
    let bound_notes: BTreeSet<usize> = selected_lyric
        .map(|lyric| {
            std::iter::once(lyric.note)
                .chain(lyric.continuation_notes.iter().copied())
                .collect()
        })
        .unwrap_or_default();
    let bound_word = selected_lyric.map(|lyric| WordSelection {
        segment: lyric.segment,
        word: lyric.word,
    });

    parent
        .spawn((
            Node {
                min_width: px(0),
                min_height: px(0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(theme.background),
        ))
        .with_children(|editor_root| {
            editor_root
                .spawn((
                    Node {
                        width: percent(100),
                        min_height: px(WORKSPACE_TOP_BAR_MIN_HEIGHT),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        column_gap: px(8),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.72)),
                    BorderColor::all(theme.border.with_alpha(0.4)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::from(AppCommand::Back),
                        false,
                        false,
                        34.0,
                    );
                    toolbar
                        .spawn(Node {
                            min_width: px(0),
                            width: px(280),
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|identity| {
                            spawn_text(
                                identity,
                                font.clone(),
                                song.as_ref()
                                    .map(|song| song.title.as_str())
                                    .unwrap_or("Chart editor"),
                                12.0,
                                theme.foreground,
                            );
                            spawn_text(
                                identity,
                                font.clone(),
                                song.as_ref()
                                    .map(|song| song.artist.as_str())
                                    .unwrap_or("Uta! Studio"),
                                9.0,
                                theme.muted_foreground,
                            );
                        });
                    if editor.dirty {
                        toolbar
                            .spawn((
                                Node {
                                    padding: UiRect::axes(px(7), px(3)),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme.primary.with_alpha(0.12)),
                            ))
                            .with_children(|badge| {
                                spawn_text(badge, font.clone(), "UNSAVED", 8.0, theme.primary);
                            });
                    }
                    toolbar.spawn(Node {
                        flex_grow: 1.0,
                        ..default()
                    });
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Undo,
                        UiAction::from(EditorCommand::Editor(EditorAction::Undo)),
                        false,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Redo,
                        UiAction::from(EditorCommand::Editor(EditorAction::Redo)),
                        false,
                        false,
                        34.0,
                    );
                    // A direct route into lyric text editing: selects the
                    // line at the playhead and opens the inspector, without
                    // first requiring a click in the lyric lane.
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::Music,
                        "Edit line",
                        UiAction::from(EditorCommand::Editor(EditorAction::EditLyricLine)),
                        false,
                    );
                    if let Some(song) = song.as_ref() {
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::Music,
                            "Lyrics workbench",
                            UiAction::from(EditorCommand::OpenLyricsEditor(song.file_hash.clone())),
                            false,
                        );
                    }
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::Music,
                        if editor.visible_evidence.contains(&app_core::EvidenceKind::Disagreement) {
                            "Evidence on"
                        } else {
                            "Evidence off"
                        },
                        UiAction::from(EditorCommand::ToggleEvidence(
                            app_core::EvidenceKind::Disagreement,
                        )),
                        false,
                    );
                    if !editor.evidence.review_regions.is_empty() {
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::ArrowLeft,
                            "Previous review",
                            UiAction::from(EditorCommand::ReviewPrevious),
                            false,
                        );
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::Play,
                            format!(
                                "Review · {}/{}",
                                editor.review_index.unwrap_or(0) + 1,
                                editor.evidence.review_regions.len()
                            ),
                            UiAction::from(EditorCommand::ReviewNext),
                            false,
                        );
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::CircleCheck,
                            "Mark reviewed",
                            UiAction::from(EditorCommand::MarkReviewRegion),
                            false,
                        );
                    }
                    if let Some(suggestion) = editor.suggestions.first() {
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::CircleCheck,
                            "Accept suggestion",
                            UiAction::from(EditorCommand::AcceptSuggestion(
                                suggestion.id.clone(),
                            )),
                            false,
                        );
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::Trash,
                            "Ignore suggestion",
                            UiAction::from(EditorCommand::IgnoreSuggestion(
                                suggestion.id.clone(),
                            )),
                            false,
                        );
                    }
                    {
                        let problems = &editor.problems_cache.1;
                        spawn_toolbar_button(
                            toolbar,
                            font.clone(),
                            icons.clone(),
                            theme,
                            UiIcon::CircleCheck,
                            if problems.total() == 0 {
                                "Checks".to_string()
                            } else {
                                format!("Checks · {}", problems.total())
                            },
                            UiAction::from(EditorCommand::Editor(EditorAction::ToggleProblemsPanel)),
                            problems.blocks_saving(),
                        );
                    }
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::PanelRight,
                        "View",
                        UiAction::from(EditorCommand::ToggleEditorLayoutMenu),
                        false,
                    );
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::Save,
                        if editor.dirty { "Files *" } else { "Files" },
                        UiAction::from(EditorCommand::ToggleEditorFileMenu),
                        false,
                    );
                });

            editor_root
                .spawn(Node {
                    min_width: px(0),
                    min_height: px(0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Row,
                    ..default()
                })
                .with_children(|workspace| {
                    workspace
                        .spawn(Node {
                            min_width: px(0),
                            min_height: px(0),
                            flex_grow: 1.0,
                            flex_direction: FlexDirection::Column,
                            ..default()
                        })
                        .with_children(|timeline_column| {
                            if tracks_visible {
                                spawn_editor_tracks(
                                    timeline_column,
                                    font.clone(),
                                    icons.clone(),
                                    editor,
                                    theme,
                                );
                            }
                            spawn_editor_timeline(
                                timeline_column,
                                font.clone(),
                                editor,
                                &notes,
                                &ghosts,
                                &bound_notes,
                                theme,
                            );
                            if !editor.lyrics_hidden {
                                timeline_column.spawn(Node {
                                    width: percent(100),
                                    height: px(10),
                                    flex_shrink: 0.0,
                                    ..default()
                                });
                                spawn_editor_lyrics(
                                    timeline_column,
                                    font.clone(),
                                    editor,
                                    &lyrics,
                                    bound_word,
                                    theme,
                                );
                            }
                            if !editor.dock_hidden {
                                spawn_editor_dock(
                                    timeline_column,
                                    font.clone(),
                                    icons.clone(),
                                    editor,
                                    session.open_editor_select,
                                    theme,
                                );
                            }
                            if !editor.status_hidden {
                                timeline_column
                                    .spawn((
                                        Node {
                                            width: percent(100),
                                            height: px(28),
                                            flex_shrink: 0.0,
                                            align_items: AlignItems::Center,
                                            padding: UiRect::horizontal(px(16)),
                                            border: UiRect::top(px(1)),
                                            ..default()
                                        },
                                        BackgroundColor(theme.card.with_alpha(0.42)),
                                        BorderColor::all(theme.border.with_alpha(0.45)),
                                    ))
                                    .with_children(|status| {
                                        let selected_notes = editor.selected_note_indices().len();
                                        let selected_lyrics = editor.selected_word_indices().len();
                                        let selection = if selected_notes > 0 {
                                            format!("{selected_notes} note(s) selected")
                                        } else if selected_lyrics > 0 {
                                            format!("{selected_lyrics} lyric item(s) selected")
                                        } else {
                                            "No selection".to_string()
                                        };
                                        spawn_text(
                                            status,
                                            font.clone(),
                                            format!(
                                                "{selection} · {:.1}–{:.1}s · {} notes",
                                                editor.viewport_start,
                                                editor.viewport_end(),
                                                notes.len()
                                            ),
                                            8.0,
                                            theme.muted_foreground,
                                        );
                                        status.spawn(Node {
                                            flex_grow: 1.0,
                                            ..default()
                                        });
                                        status
                                            .spawn((
                                                EditorShortcutsHoverTrigger,
                                                Interaction::None,
                                                Node::default(),
                                            ))
                                            .with_children(|trigger| {
                                                spawn_text(
                                                    trigger,
                                                    font.clone(),
                                                    "Double-click lyric to edit · drag edges to resize · wheel / Shift / Ctrl / Alt to navigate · hover or press H for all shortcuts",
                                                    8.0,
                                                    theme.muted_foreground,
                                                );
                                            });
                                    });
                            }
                        });
                    if editor.inspector_open {
                        spawn_editor_inspector(workspace, font.clone(), editor, &notes, theme);
                    }
                });

            if editor.problems_panel_open {
                spawn_problems_panel(editor_root, font.clone(), theme, editor);
            }
            spawn_shortcuts_panel(editor_root, font.clone(), theme, editor);
            spawn_all_lyrics_panel(editor_root, font.clone(), theme, editor);
            if let Some(notice) = session.notice.as_deref() {
                editor_root
                    .spawn((
                        Node {
                            width: percent(100),
                            min_height: px(28),
                            align_items: AlignItems::Center,
                            padding: UiRect::horizontal(px(16)),
                            ..default()
                        },
                        BackgroundColor(theme.muted.with_alpha(0.5)),
                        children![(
                            Text::new(notice),
                            ui_text_font(font, 9.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::default(),
                        )],
                    ));
            }
        });
}

pub(crate) fn spawn_editor_dock(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    editor: &NativeEditor,
    open_select: Option<EditorDockSelectKind>,
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: percent(100),
                min_height: px(48),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(px(12)),
                column_gap: px(6),
                border: UiRect::top(px(1)),
                overflow: Overflow::scroll_x(),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.7)),
            BorderColor::all(theme.border.with_alpha(0.58)),
        ))
        .with_children(|dock| {
            let audio_available = editor.audio_status.loaded && editor.audio_status.error.is_none();
            if audio_available {
                spawn_icon_button(
                    dock,
                    icons.clone(),
                    theme,
                    if editor.audio_status.playing {
                        UiIcon::Pause
                    } else {
                        UiIcon::Play
                    },
                    UiAction::from(EditorCommand::Editor(EditorAction::TogglePlayback)),
                    true,
                    false,
                    36.0,
                );
                spawn_icon_button(
                    dock,
                    icons.clone(),
                    theme,
                    UiIcon::ArrowLeft,
                    UiAction::from(EditorCommand::Editor(EditorAction::SeekStart)),
                    false,
                    false,
                    30.0,
                );
                dock.spawn((
                    EditorClockText,
                    Text::new(format_editor_clock(
                        editor.visible_position,
                        editor.audio_status.duration_secs,
                    )),
                    ui_text_font(font.clone(), 10.0),
                    TextColor(theme.foreground),
                    TextLayout::no_wrap(),
                ));
                let mut audio_options = vec![
                    ("instrumental".to_string(), "Instrumental".to_string()),
                    ("original".to_string(), "Original".to_string()),
                ];
                if editor.chart.audio.vocals.is_some() {
                    audio_options.insert(0, ("vocals".to_string(), "Vocals".to_string()));
                }
                if let Some(context) = editor.source_context.as_ref() {
                    audio_options.extend(context.audio_artifacts.iter().map(|artifact| {
                        (
                            format!("artifact:{}", artifact.revision.revision_id),
                            artifact.label.clone(),
                        )
                    }));
                }
                let audio_option_refs = audio_options
                    .iter()
                    .map(|(value, label)| (value.as_str(), label.as_str()))
                    .collect::<Vec<_>>();
                let audio_label = audio_options
                    .iter()
                    .find(|(value, _)| value == &editor.audio_source)
                    .map(|(_, label)| label.as_str())
                    .unwrap_or("Workflow artifact");
                spawn_editor_select(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    EditorDockSelectKind::AudioSource,
                    UiIcon::Music,
                    audio_label,
                    editor.audio_source.as_str(),
                    &audio_option_refs,
                    open_select == Some(EditorDockSelectKind::AudioSource),
                );
                spawn_editor_select(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    EditorDockSelectKind::AuditionMode,
                    UiIcon::Sparkles,
                    match editor.audition_mode {
                        AuditionMode::Pitch => "Pitch only",
                        AuditionMode::Mixed => "Audio + pitch",
                        AuditionMode::Audio => "Audio only",
                    },
                    editor.audition_mode.label(),
                    &[
                        ("audio", "Audio only"),
                        ("pitch", "Pitch only"),
                        ("mixed", "Audio + pitch"),
                    ],
                    open_select == Some(EditorDockSelectKind::AuditionMode),
                );
                // Ranged audition: hear the selection, or the run-up and
                // run-out around it, without losing the transport position.
                for (label, action) in [
                    ("Play range", EditorAction::AuditionSelection),
                    ("In", EditorAction::AuditionBeforeSelection),
                    ("Out", EditorAction::AuditionAfterSelection),
                    ("Screen", EditorAction::AuditionVisible),
                ] {
                    spawn_text_button(
                        dock,
                        font.clone(),
                        theme,
                        label,
                        9.0,
                        UiAction::from(EditorCommand::Editor(action)),
                    );
                }
                if editor.audition_until.is_some() {
                    spawn_text_button(
                        dock,
                        font.clone(),
                        theme,
                        "Stop",
                        9.0,
                        UiAction::from(EditorCommand::Editor(EditorAction::StopAudition)),
                    );
                }
                spawn_text_button(
                    dock,
                    font.clone(),
                    theme,
                    if editor.tap_mode {
                        match editor.tap.remaining() {
                            0 => "Tapping".to_string(),
                            remaining => format!("Tapping · {remaining} left"),
                        }
                    } else {
                        "Tap".to_string()
                    },
                    9.0,
                    UiAction::from(EditorCommand::Editor(EditorAction::ToggleTapMode)),
                );
            } else {
                spawn_icon(
                    dock,
                    icons.clone(),
                    UiIcon::Play,
                    16.0,
                    theme.muted_foreground.with_alpha(0.35),
                );
                spawn_text(
                    dock,
                    font.clone(),
                    "Audio unavailable · chart editing remains enabled",
                    9.0,
                    theme.muted_foreground,
                );
            }
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            let selected_notes = editor.selected_note_indices().len();
            let selected_lyrics = editor.selected_word_indices().len();
            let lyric_context = selected_notes == 0 && selected_lyrics > 0;
            let mut tools = Vec::new();
            let context_label = if lyric_context {
                tools.push((
                    UiIcon::Add,
                    "Add",
                    UiAction::from(EditorCommand::Editor(EditorAction::AddLyric)),
                    false,
                ));
                tools.push((
                    UiIcon::Scissors,
                    "Split",
                    UiAction::from(EditorCommand::Editor(EditorAction::SplitSelection)),
                    false,
                ));
                if selected_lyrics > 1 {
                    tools.push((
                        UiIcon::Combine,
                        "Merge",
                        UiAction::from(EditorCommand::Editor(EditorAction::MergeSelection)),
                        false,
                    ));
                }
                tools.push((
                    UiIcon::Trash,
                    "Delete",
                    UiAction::from(EditorCommand::Editor(EditorAction::DeleteSelection)),
                    true,
                ));
                format!("LYRICS · {selected_lyrics}")
            } else if selected_notes > 0 {
                tools.push((
                    UiIcon::Scissors,
                    "Split",
                    UiAction::from(EditorCommand::Editor(EditorAction::SplitSelection)),
                    false,
                ));
                if selected_notes > 1 {
                    tools.push((
                        UiIcon::Combine,
                        "Merge",
                        UiAction::from(EditorCommand::Editor(EditorAction::MergeSelection)),
                        false,
                    ));
                }
                tools.extend([
                    (
                        UiIcon::Copy,
                        "Copy",
                        UiAction::from(EditorCommand::Editor(EditorAction::CopyNotes)),
                        false,
                    ),
                    (
                        UiIcon::Copy,
                        "Duplicate",
                        UiAction::from(EditorCommand::Editor(EditorAction::DuplicateNotes)),
                        false,
                    ),
                    (
                        UiIcon::Sparkles,
                        "Type",
                        UiAction::from(EditorCommand::Editor(EditorAction::CycleNoteKind)),
                        false,
                    ),
                    (
                        UiIcon::Grid,
                        "Quantize",
                        UiAction::from(EditorCommand::Editor(EditorAction::QuantizeNotes)),
                        false,
                    ),
                    (
                        UiIcon::Trash,
                        "Delete",
                        UiAction::from(EditorCommand::Editor(EditorAction::DeleteSelection)),
                        true,
                    ),
                ]);
                format!("NOTES · {selected_notes}")
            } else {
                tools.extend([
                    (
                        UiIcon::Add,
                        // Reminds whoever clicked it that the tool is armed
                        // and waiting for a click-drag on the canvas.
                        if editor.note_insert_armed {
                            "Click canvas…"
                        } else {
                            "Note"
                        },
                        UiAction::from(EditorCommand::Editor(EditorAction::AddNote)),
                        false,
                    ),
                    (
                        UiIcon::Add,
                        "Lyric",
                        UiAction::from(EditorCommand::Editor(EditorAction::AddLyric)),
                        false,
                    ),
                ]);
                if !editor.clipboard_notes.is_empty() {
                    tools.push((
                        UiIcon::Clipboard,
                        "Paste",
                        UiAction::from(EditorCommand::Editor(EditorAction::PasteNotes)),
                        false,
                    ));
                }
                tools.push((
                    UiIcon::Repair,
                    "Repair",
                    UiAction::from(EditorCommand::Editor(EditorAction::RepairChart)),
                    false,
                ));
                "CREATE".to_string()
            };
            spawn_text(
                dock,
                font.clone(),
                context_label,
                8.0,
                theme.muted_foreground,
            );
            for (icon, label, action, destructive) in tools {
                spawn_toolbar_button(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    icon,
                    label,
                    action,
                    destructive,
                );
            }
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            // Binding pairs an unpitched lyric with a lyric-less pitch note.
            // Hold B or C and click a note or lyric to name the pair
            // explicitly; these buttons bind or unbind the nearest one to
            // whatever is already selected.
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Bind",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::BindNearest)),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Unbind",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::UnbindSelection)),
            );
            // Which side's start/end a bind keeps when the lyric and the
            // pitch note it's landing on disagree — a one-time alignment
            // choice, not a lasting link between the two.
            dock.spawn((
                Button,
                UiAction::from(EditorCommand::Editor(EditorAction::ToggleBindAlignment)),
                Node {
                    height: px(24),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(7)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(theme.muted.with_alpha(0.4)),
                children![(
                    Text::new(format!("Align: {}", editor.bind_alignment.label())),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(theme.muted_foreground),
                    TextLayout::no_wrap(),
                )],
            ));
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            // Locks notes and lyrics against an accidental drag once their
            // timing is dialed in; arrow-key nudging still works.
            dock.spawn((
                Button,
                UiAction::from(EditorCommand::Editor(EditorAction::ToggleLockMode)),
                Node {
                    height: px(24),
                    align_items: AlignItems::Center,
                    padding: UiRect::horizontal(px(7)),
                    border_radius: BorderRadius::all(px(4)),
                    ..default()
                },
                BackgroundColor(if editor.lock_mode {
                    theme.primary.with_alpha(0.24)
                } else {
                    theme.muted.with_alpha(0.4)
                }),
                children![(
                    Text::new(if editor.lock_mode { "Locked" } else { "Lock" }),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(if editor.lock_mode {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    }),
                    TextLayout::no_wrap(),
                )],
            ));
            {
                // Always visible rather than appearing only once a song has
                // beat data — otherwise the feature reads as not existing at
                // all. Unavailable (no `music_analysis.json` beats, e.g. not
                // yet re-analyzed, or Essentia isn't installed) gets its own
                // dimmer style, distinct from "available but switched off".
                let has_beats = !editor.beats.is_empty();
                let on = has_beats && editor.beat_grid_visible;
                dock.spawn((
                    Button,
                    UiAction::from(EditorCommand::Editor(EditorAction::ToggleBeatGrid)),
                    Node {
                        height: px(24),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(7)),
                        border_radius: BorderRadius::all(px(4)),
                        ..default()
                    },
                    BackgroundColor(if on {
                        theme.primary.with_alpha(0.24)
                    } else if has_beats {
                        theme.muted.with_alpha(0.4)
                    } else {
                        theme.muted.with_alpha(0.16)
                    }),
                    children![(
                        Text::new("Beat grid"),
                        ui_text_font(font.clone(), 8.0),
                        TextColor(if on {
                            theme.primary
                        } else if has_beats {
                            theme.muted_foreground
                        } else {
                            theme.muted_foreground.with_alpha(0.5)
                        }),
                        TextLayout::no_wrap(),
                    )],
                ));
            }
            dock.spawn((
                Node {
                    width: px(1),
                    height: px(24),
                    margin: UiRect::horizontal(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.6)),
            ));
            spawn_editor_select(
                dock,
                font.clone(),
                icons.clone(),
                theme,
                EditorDockSelectKind::SnapGrid,
                UiIcon::Grid,
                format!("Grid {}", format_snap_grid(editor.snap_seconds)),
                &editor.snap_seconds.to_string(),
                &[
                    ("0", "Grid off"),
                    ("0.01", "10 ms"),
                    ("0.025", "25 ms"),
                    ("0.05", "50 ms"),
                    ("0.1", "100 ms"),
                    ("0.25", "250 ms"),
                ],
                open_select == Some(EditorDockSelectKind::SnapGrid),
            );
            dock.spawn(Node {
                min_width: px(10),
                flex_grow: 1.0,
                ..default()
            });
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Fit selection",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::FitSelection)),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Fit song",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::FitSong)),
            );
            spawn_icon_button(
                dock,
                icons.clone(),
                theme,
                UiIcon::ZoomOut,
                UiAction::from(EditorCommand::Editor(EditorAction::ZoomOutTime)),
                false,
                false,
                32.0,
            );
            spawn_text(
                dock,
                font.clone(),
                format!("{:.0}s", editor.viewport_duration),
                9.0,
                theme.muted_foreground,
            );
            spawn_icon_button(
                dock,
                icons,
                theme,
                UiIcon::ZoomIn,
                UiAction::from(EditorCommand::Editor(EditorAction::ZoomInTime)),
                false,
                false,
                32.0,
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Pitch ↓",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::PanPitchDown)),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Pitch ↑",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::PanPitchUp)),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Range +",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::ZoomOutPitch)),
            );
            spawn_text_button(
                dock,
                font,
                theme,
                "Range −",
                9.0,
                UiAction::from(EditorCommand::Editor(EditorAction::ZoomInPitch)),
            );
        });
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn spawn_editor_select(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    theme: &StudioTheme,
    kind: EditorDockSelectKind,
    icon: UiIcon,
    label: impl Into<String>,
    current_value: &str,
    options: &[(&str, &str)],
    open: bool,
) {
    let label = label.into();
    parent
        .spawn((
            Node {
                position_type: PositionType::Relative,
                height: px(32),
                flex_shrink: 0.0,
                ..default()
            },
            ZIndex(if open { 70 } else { 0 }),
        ))
        .with_children(|control| {
            control
                .spawn((
                    Button,
                    UiAction::from(EditorCommand::OpenEditorSelect(kind)),
                    Node {
                        height: px(32),
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(9)),
                        column_gap: px(6),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(6)),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(if open { 0.72 } else { 0.38 })),
                    BorderColor::all(if open {
                        theme.primary.with_alpha(0.72)
                    } else {
                        theme.border.with_alpha(0.44)
                    }),
                ))
                .with_children(|button| {
                    spawn_icon(button, icons.clone(), icon, 14.0, theme.foreground);
                    spawn_text(button, font.clone(), label, 9.0, theme.foreground);
                    spawn_icon(
                        button,
                        icons.clone(),
                        UiIcon::ChevronDown,
                        12.0,
                        theme.muted_foreground,
                    );
                });
            if open {
                control
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            bottom: px(36),
                            min_width: px(150),
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::all(px(5)),
                            row_gap: px(2),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(7)),
                            ..default()
                        },
                        BackgroundColor(theme.card),
                        BorderColor::all(theme.border.with_alpha(0.9)),
                        ZIndex(70),
                    ))
                    .with_children(|menu| {
                        for (value, option_label) in options {
                            let selected = *value == current_value;
                            menu.spawn((
                                Button,
                                UiAction::from(EditorCommand::SelectEditorValue(
                                    kind,
                                    (*value).to_string(),
                                )),
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
                                    theme.primary.with_alpha(0.12)
                                } else {
                                    Color::NONE
                                }),
                            ))
                            .with_children(|option| {
                                spawn_text(
                                    option,
                                    font.clone(),
                                    *option_label,
                                    9.0,
                                    if selected {
                                        theme.primary
                                    } else {
                                        theme.foreground
                                    },
                                );
                                option.spawn(Node {
                                    flex_grow: 1.0,
                                    ..default()
                                });
                                if selected {
                                    spawn_icon(
                                        option,
                                        icons.clone(),
                                        UiIcon::Check,
                                        13.0,
                                        theme.primary,
                                    );
                                }
                            });
                        }
                    });
            }
        });
}
