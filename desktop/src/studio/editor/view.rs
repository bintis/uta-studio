//! Editor rendering: chrome, dock, timeline, lyric lane, and playhead.

use crate::studio::*;

pub(crate) fn spawn_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
    theme: &StudioTheme,
    window_size: Vec2,
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
                        height: px(58),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(12)),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.58)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons,
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::Back,
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
                                    "Chart needs attention"
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
    let tracks_visible = session.editor_tracks_open || editor.document.track_count() > 1;
    let lyrics = chart_lyrics(&editor.document);
    // The lyric or note(s) the current selection is bound to, so its match
    // can be highlighted and connected — the format ties a note's pitch and
    // its lyric together, and clicking either one should show the other. A
    // syllable held across a pitch change spans more than one note (see
    // `ChartLyricView::continuation_notes`), so all of them highlight
    // together rather than just the one carrying the lyric text.
    let selected_lyric = if let Some(word) = editor.selected_word {
        lyrics
            .iter()
            .find(|lyric| lyric.segment == word.segment && lyric.word == word.word && lyric.guided)
    } else {
        editor.selected_note.and_then(|note_index| {
            lyrics
                .iter()
                .find(|lyric| lyric.note == note_index || lyric.continuation_notes.contains(&note_index))
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
                        height: px(58),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::horizontal(px(16)),
                        column_gap: px(8),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.58)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                ))
                .with_children(|toolbar| {
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::ArrowLeft,
                        UiAction::Back,
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
                                    .unwrap_or("Uta Studio"),
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
                        UiAction::Editor(EditorAction::Undo),
                        false,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Redo,
                        UiAction::Editor(EditorAction::Redo),
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
                        "Edit lyrics",
                        UiAction::Editor(EditorAction::EditLyricLine),
                        false,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::PanelBottom,
                        UiAction::Editor(EditorAction::ToggleLyrics),
                        !editor.lyrics_hidden,
                        false,
                        34.0,
                    );
                    spawn_duet_icon_button(
                        toolbar,
                        theme,
                        UiAction::Editor(EditorAction::ToggleTracks),
                        tracks_visible,
                        // A multi-track chart always shows which track an edit
                        // would land on, so the toggle has nothing to do.
                        editor.document.track_count() > 1,
                        34.0,
                    );
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
                            UiAction::Editor(EditorAction::ToggleProblemsPanel),
                            problems.blocks_saving(),
                        );
                    }
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::PanelRight,
                        UiAction::Editor(EditorAction::ToggleInspector),
                        editor.inspector_open,
                        false,
                        34.0,
                    );
                    spawn_icon_button(
                        toolbar,
                        icons.clone(),
                        theme,
                        UiIcon::Settings,
                        UiAction::OpenSongSettings(editor.chart.file_hash.clone()),
                        false,
                        false,
                        34.0,
                    );
                    spawn_toolbar_button(
                        toolbar,
                        font.clone(),
                        icons.clone(),
                        theme,
                        UiIcon::Save,
                        if editor.dirty { "Save *" } else { "Save" },
                        UiAction::Editor(EditorAction::Save),
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
                                // A thin strip between the pitch canvas and
                                // the lyric lane, bridging the gap between
                                // the canvas and lane segments spawned below
                                // (see `spawn_editor_timeline` /
                                // `spawn_editor_lyrics`) into one line. The
                                // leading spacer matches the gutter both rows
                                // above and below indent past, so the same
                                // `left` percent lines up with them.
                                timeline_column
                                    .spawn(Node {
                                        width: percent(100),
                                        height: px(10),
                                        flex_shrink: 0.0,
                                        flex_direction: FlexDirection::Row,
                                        ..default()
                                    })
                                    .with_children(|gap_row| {
                                        gap_row.spawn(Node {
                                            width: px(EDITOR_TRACK_GUTTER_WIDTH),
                                            flex_shrink: 0.0,
                                            ..default()
                                        });
                                        gap_row
                                            .spawn(Node {
                                                position_type: PositionType::Relative,
                                                min_width: px(0),
                                                height: percent(100),
                                                flex_grow: 1.0,
                                                ..default()
                                            })
                                            .with_children(|gap| {
                                                spawn_editor_binding_guide(
                                                    gap,
                                                    theme,
                                                    EditorBindingGuidePart::Gap,
                                                );
                                            });
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
                            spawn_editor_dock(
                                timeline_column,
                                font.clone(),
                                icons.clone(),
                                editor,
                                session.open_editor_select,
                                theme,
                            );
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
            if let Some(context) = editor.note_context.as_ref() {
                spawn_note_context_menu(editor_root, font.clone(), theme, editor, context, window_size);
            }
            if let Some(context) = editor.lyric_context.as_ref() {
                spawn_lyric_context_menu(editor_root, font.clone(), theme, editor, context, window_size);
            }
            if let Some(context) = editor.waveform_context.as_ref() {
                spawn_waveform_context_menu(editor_root, font.clone(), theme, editor, context, window_size);
            }

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
                    UiAction::Editor(EditorAction::TogglePlayback),
                    true,
                    false,
                    36.0,
                );
                spawn_icon_button(
                    dock,
                    icons.clone(),
                    theme,
                    UiIcon::ArrowLeft,
                    UiAction::Editor(EditorAction::SeekStart),
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
                let mut audio_options = vec![("instrumental", "Instrumental")];
                if editor.chart.audio.vocals.is_some() {
                    audio_options.insert(0, ("vocals", "Vocals"));
                }
                audio_options.push(("original", "Original"));
                spawn_editor_select(
                    dock,
                    font.clone(),
                    icons.clone(),
                    theme,
                    EditorDockSelectKind::AudioSource,
                    UiIcon::Music,
                    match editor.audio_source.as_str() {
                        "vocals" => "Vocals",
                        "original" => "Original",
                        _ => "Instrumental",
                    },
                    editor.audio_source.as_str(),
                    &audio_options,
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
                        UiAction::Editor(action),
                    );
                }
                if editor.audition_until.is_some() {
                    spawn_text_button(
                        dock,
                        font.clone(),
                        theme,
                        "Stop",
                        9.0,
                        UiAction::Editor(EditorAction::StopAudition),
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
                    UiAction::Editor(EditorAction::ToggleTapMode),
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
                    UiAction::Editor(EditorAction::AddLyric),
                    false,
                ));
                tools.push((
                    UiIcon::Scissors,
                    "Split",
                    UiAction::Editor(EditorAction::SplitSelection),
                    false,
                ));
                if selected_lyrics > 1 {
                    tools.push((
                        UiIcon::Combine,
                        "Merge",
                        UiAction::Editor(EditorAction::MergeSelection),
                        false,
                    ));
                }
                tools.push((
                    UiIcon::Trash,
                    "Delete",
                    UiAction::Editor(EditorAction::DeleteSelection),
                    true,
                ));
                format!("LYRICS · {selected_lyrics}")
            } else if selected_notes > 0 {
                tools.push((
                    UiIcon::Scissors,
                    "Split",
                    UiAction::Editor(EditorAction::SplitSelection),
                    false,
                ));
                if selected_notes > 1 {
                    tools.push((
                        UiIcon::Combine,
                        "Merge",
                        UiAction::Editor(EditorAction::MergeSelection),
                        false,
                    ));
                }
                tools.extend([
                    (
                        UiIcon::Copy,
                        "Copy",
                        UiAction::Editor(EditorAction::CopyNotes),
                        false,
                    ),
                    (
                        UiIcon::Copy,
                        "Duplicate",
                        UiAction::Editor(EditorAction::DuplicateNotes),
                        false,
                    ),
                    (
                        UiIcon::Sparkles,
                        "Type",
                        UiAction::Editor(EditorAction::CycleNoteKind),
                        false,
                    ),
                    (
                        UiIcon::Grid,
                        "Quantize",
                        UiAction::Editor(EditorAction::QuantizeNotes),
                        false,
                    ),
                    (
                        UiIcon::Trash,
                        "Delete",
                        UiAction::Editor(EditorAction::DeleteSelection),
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
                        UiAction::Editor(EditorAction::AddNote),
                        false,
                    ),
                    (
                        UiIcon::Add,
                        "Lyric",
                        UiAction::Editor(EditorAction::AddLyric),
                        false,
                    ),
                ]);
                if !editor.clipboard_notes.is_empty() {
                    tools.push((
                        UiIcon::Clipboard,
                        "Paste",
                        UiAction::Editor(EditorAction::PasteNotes),
                        false,
                    ));
                }
                tools.push((
                    UiIcon::Repair,
                    "Repair",
                    UiAction::Editor(EditorAction::RepairChart),
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
                UiAction::Editor(EditorAction::BindNearest),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Unbind",
                9.0,
                UiAction::Editor(EditorAction::UnbindSelection),
            );
            // Which side's start/end a bind keeps when the lyric and the
            // pitch note it's landing on disagree — a one-time alignment
            // choice, not a lasting link between the two.
            dock.spawn((
                Button,
                UiAction::Editor(EditorAction::ToggleBindAlignment),
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
                UiAction::Editor(EditorAction::ToggleLockMode),
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
                    UiAction::Editor(EditorAction::ToggleBeatGrid),
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
                UiAction::Editor(EditorAction::FitSelection),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Fit song",
                9.0,
                UiAction::Editor(EditorAction::FitSong),
            );
            spawn_icon_button(
                dock,
                icons.clone(),
                theme,
                UiIcon::ZoomOut,
                UiAction::Editor(EditorAction::ZoomOutTime),
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
                UiAction::Editor(EditorAction::ZoomInTime),
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
                UiAction::Editor(EditorAction::PanPitchDown),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Pitch ↑",
                9.0,
                UiAction::Editor(EditorAction::PanPitchUp),
            );
            spawn_text_button(
                dock,
                font.clone(),
                theme,
                "Range +",
                9.0,
                UiAction::Editor(EditorAction::ZoomOutPitch),
            );
            spawn_text_button(
                dock,
                font,
                theme,
                "Range −",
                9.0,
                UiAction::Editor(EditorAction::ZoomInPitch),
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
                    UiAction::OpenEditorSelect(kind),
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
                                UiAction::SelectEditorValue(kind, (*value).to_string()),
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

pub(crate) fn spawn_editor_timeline(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    notes: &[ChartNoteView],
    // Notes belonging to the other tracks, drawn behind and not editable.
    ghosts: &[ChartNoteView],
    // The note(s) the selected lyric is bound to — more than one when it's
    // held across a pitch change — highlighted to match it.
    bound_notes: &BTreeSet<usize>,
    theme: &StudioTheme,
) {
    parent
        .spawn(Node {
            width: percent(100),
            min_height: px(240),
            flex_grow: 1.0,
            flex_direction: FlexDirection::Row,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Node {
                    position_type: PositionType::Relative,
                    width: px(EDITOR_TRACK_GUTTER_WIDTH),
                    height: percent(100),
                    flex_shrink: 0.0,
                    border: UiRect::right(px(1)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme.card.with_alpha(0.44)),
                BorderColor::all(theme.border.with_alpha(0.45)),
            ))
            .with_children(|gutter| {
                gutter.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(7),
                        top: px(6),
                        ..default()
                    },
                    Text::new("AUDIO"),
                    ui_text_font(font.clone(), 7.0),
                    TextColor(theme.muted_foreground.with_alpha(0.65)),
                ));
                let pitch_span = (editor.pitch_max - editor.pitch_min).max(1.0);
                let pitch_step = (pitch_span / 42.0).ceil().max(1.0) as usize;
                for midi in ((editor.pitch_min.floor() as i32).clamp(0, 127)
                    ..=(editor.pitch_max.ceil() as i32).clamp(0, 127))
                    .step_by(pitch_step)
                {
                    let top = pitch_percent(f64::from(midi) + 0.5, editor);
                    let bottom = pitch_percent(f64::from(midi) - 0.5, editor);
                    let black_key = matches!(midi.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
                    gutter
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                right: px(0),
                                top: percent(top),
                                width: if black_key { percent(68) } else { percent(100) },
                                height: percent((bottom - top).max(0.1)),
                                align_items: AlignItems::Center,
                                justify_content: JustifyContent::FlexEnd,
                                padding: UiRect::right(px(3)),
                                border: UiRect::bottom(px(1)),
                                ..default()
                            },
                            // Piano keys read as piano keys — near-black and
                            // near-white, like a real keyboard — rather than
                            // faint tints of the app theme, which used to
                            // leave white keys almost invisible against the
                            // gutter's own background.
                            BackgroundColor(if black_key {
                                Color::srgba(0.05, 0.05, 0.07, 0.94)
                            } else {
                                Color::srgba(0.95, 0.95, 0.93, 0.98)
                            }),
                            BorderColor::all(if black_key {
                                Color::srgba(0.0, 0.0, 0.0, 0.9)
                            } else {
                                Color::srgba(0.35, 0.35, 0.35, 0.55)
                            }),
                        ))
                        .with_children(|key| {
                            if midi.rem_euclid(12) == 0 {
                                key.spawn((
                                    Text::new(midi_note_name(f64::from(midi))),
                                    ui_text_font(font.clone(), 6.5),
                                    TextColor(Color::srgba(0.12, 0.12, 0.12, 0.92)),
                                    TextLayout::no_wrap(),
                                ));
                            }
                        });
                }
            });
            row.spawn((
                Button,
                EditorTimelineSurface,
                Node {
                    position_type: PositionType::Relative,
                    min_width: px(0),
                    height: percent(100),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(theme.background.with_alpha(0.96)),
            ))
            .with_children(|canvas| {
                canvas.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        right: px(0),
                        top: px(0),
                        height: percent(EDITOR_PITCH_TOP_PERCENT),
                        border: UiRect::bottom(px(1)),
                        ..default()
                    },
                    BackgroundColor(theme.card.with_alpha(0.56)),
                    BorderColor::all(theme.border.with_alpha(0.55)),
                    Pickable::IGNORE,
                ));
                let pitch_step = ((editor.pitch_max - editor.pitch_min) / 30.0)
                    .ceil()
                    .max(1.0) as usize;
                for midi in ((editor.pitch_min.floor() as i32).clamp(0, 127)
                    ..=(editor.pitch_max.ceil() as i32).clamp(0, 127))
                    .step_by(pitch_step)
                {
                    let top = pitch_percent(f64::from(midi) + 0.5, editor);
                    let bottom = pitch_percent(f64::from(midi) - 0.5, editor);
                    let black_key = matches!(midi.rem_euclid(12), 1 | 3 | 6 | 8 | 10);
                    if black_key {
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: px(0),
                                right: px(0),
                                top: percent(top),
                                height: percent((bottom - top).max(0.1)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.16)),
                            Pickable::IGNORE,
                        ));
                    }
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(0),
                            right: px(0),
                            top: percent(pitch_percent(f64::from(midi), editor)),
                            height: px(1),
                            ..default()
                        },
                        BackgroundColor(theme.border.with_alpha(if midi.rem_euclid(12) == 0 {
                            0.54
                        } else {
                            0.22
                        })),
                        Pickable::IGNORE,
                    ));
                }
                for step in 0..=12 {
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            top: px(0),
                            bottom: px(0),
                            left: percent(step as f32 / 12.0 * 100.0),
                            width: px(1),
                            ..default()
                        },
                        BackgroundColor(theme.border.with_alpha(if step % 3 == 0 {
                            0.38
                        } else {
                            0.14
                        })),
                        Pickable::IGNORE,
                    ));
                    if step % 2 == 0 && step < 12 {
                        let time = editor.viewport_start
                            + editor.viewport_duration * f64::from(step) / 12.0;
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(step as f32 / 12.0 * 100.0),
                                top: px(4),
                                padding: UiRect::left(px(4)),
                                ..default()
                            },
                            Text::new(format!("{time:.1}s")),
                            ui_text_font(font.clone(), 8.0),
                            TextColor(theme.muted_foreground),
                            Pickable::IGNORE,
                        ));
                    }
                }
                // A purely visual reference layer — never the authoritative
                // absolute-second timeline, and never read back into any
                // note or lyric. Weaker than the time-ruler ticks above so
                // it never competes with them or with note/lyric content
                // for attention; culled to the visible beats, and capped so
                // a fast tempo at maximum zoom-out can't spawn thousands of
                // nodes in one rebuild.
                if editor.beat_grid_visible && !editor.beats.is_empty() {
                    const MAX_VISIBLE_BEATS: usize = 300;
                    let viewport_end = editor.viewport_end();
                    let start_index = editor
                        .beats
                        .partition_point(|&beat| beat < editor.viewport_start);
                    let end_index = editor
                        .beats
                        .partition_point(|&beat| beat <= viewport_end);
                    let visible = &editor.beats[start_index..end_index];
                    let stride = visible.len().div_ceil(MAX_VISIBLE_BEATS).max(1);
                    for &beat in visible.iter().step_by(stride) {
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(time_percent(beat, editor)),
                                top: px(0),
                                bottom: px(0),
                                width: px(1),
                                ..default()
                            },
                            BackgroundColor(theme.border.with_alpha(0.10)),
                            Pickable::IGNORE,
                        ));
                    }
                }
                // A static, program-generated pitch reference drawn once
                // across the whole canvas rather than clipped inside each
                // note's own box — the analyzer's raw evidence, never
                // editable, sitting behind the waveform and notes as a
                // spectrogram-like backdrop to author against.
                {
                    let visible_frames = editor
                        .pitch_frames
                        .iter()
                        .filter(|frame| {
                            frame.time >= editor.viewport_start
                                && frame.time <= editor.viewport_end()
                                && frame.confidence >= 0.12
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let reference = abstract_pitch_contour(&visible_frames, 320);
                    // Buckets more than a couple of widths apart span a gap
                    // in the voicing (a breath, a rest) — draw those as a
                    // break in the trace instead of a false connection.
                    let gap_seconds = (editor.viewport_duration / 320.0) * 2.5;
                    for pair in reference.windows(2) {
                        let [start, end] = pair else { continue };
                        if end.time - start.time > gap_seconds {
                            continue;
                        }
                        let left = time_percent(start.time, editor);
                        let right = time_percent(end.time, editor);
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(left),
                                top: percent(pitch_percent(start.midi, editor)),
                                width: percent((right - left).max(0.3)),
                                height: px(1.2),
                                ..default()
                            },
                            BackgroundColor(
                                theme
                                    .pitch_contour
                                    .with_alpha((0.10 + start.confidence as f32 * 0.16).min(0.32)),
                            ),
                            Pickable::IGNORE,
                        ));
                    }
                }
                let peak_count = editor.waveform.peaks.len();
                if peak_count > 0 && editor.waveform.duration_secs > 0.0 {
                    let visible_peaks = editor
                        .waveform
                        .peaks
                        .iter()
                        .enumerate()
                        .filter_map(|(index, peak)| {
                            let time =
                                index as f64 / peak_count as f64 * editor.waveform.duration_secs;
                            (time >= editor.viewport_start && time <= editor.viewport_end())
                                .then_some((time, *peak))
                        })
                        .collect::<Vec<_>>();
                    // Reduce each bar's whole span to its true min/max instead
                    // of sampling one point from it: a stride that skips
                    // buckets can miss the loudest transient in the gap and
                    // draw a waveform quieter or spikier than the audio.
                    let bars = 360usize;
                    let chunk_size = visible_peaks.len().div_ceil(bars).max(1);
                    let groups = visible_peaks
                        .chunks(chunk_size)
                        .filter_map(|group| {
                            let &(time, _) = group.first()?;
                            let minimum = group
                                .iter()
                                .map(|(_, (minimum, _))| *minimum)
                                .fold(f32::INFINITY, f32::min);
                            let maximum = group
                                .iter()
                                .map(|(_, (_, maximum))| *maximum)
                                .fold(f32::NEG_INFINITY, f32::max);
                            let amplitude = (maximum - minimum).abs().clamp(0.01, 2.0);
                            Some((time_percent(time, editor), amplitude))
                        })
                        .collect::<Vec<_>>();
                    match editor.waveform_style {
                        WaveformStyle::Bars => {
                            for &(left, amplitude) in &groups {
                                canvas.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: percent(left),
                                        top: percent(13.0 - amplitude * 3.0),
                                        width: px(1),
                                        height: percent(amplitude * 6.0),
                                        ..default()
                                    },
                                    BackgroundColor(theme.waveform.with_alpha(0.32)),
                                    Pickable::IGNORE,
                                ));
                            }
                        }
                        WaveformStyle::Filled => {
                            // Contiguous, gapless bars read as a solid mass
                            // rather than individual sticks.
                            let width = (100.0 / groups.len().max(1) as f32).max(0.3);
                            for &(left, amplitude) in &groups {
                                canvas.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: percent(left),
                                        top: percent(13.0 - amplitude * 3.0),
                                        width: percent(width),
                                        height: percent(amplitude * 6.0),
                                        ..default()
                                    },
                                    BackgroundColor(theme.waveform.with_alpha(0.45)),
                                    Pickable::IGNORE,
                                ));
                            }
                        }
                        WaveformStyle::Line => {
                            // A single connected trace along the envelope
                            // peak, the same segment-joining technique the
                            // per-note pitch contour uses.
                            for pair in groups.windows(2) {
                                let [(left, amplitude), (next_left, _)] = pair else {
                                    continue;
                                };
                                canvas.spawn((
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: percent(*left),
                                        top: percent(13.0 - amplitude * 3.0),
                                        width: percent((next_left - left).max(0.2)),
                                        height: px(1.3),
                                        ..default()
                                    },
                                    BackgroundColor(theme.waveform.with_alpha(0.72)),
                                    Pickable::IGNORE,
                                ));
                            }
                        }
                    }
                }
                // Other tracks read as context: visible enough to place a
                // second voice against, never mistakable for what is editable.
                for ghost in ghosts.iter().filter(|note| {
                    note.end >= editor.viewport_start && note.start <= editor.viewport_end()
                }) {
                    let left = time_percent(ghost.start, editor);
                    let right = time_percent(ghost.end, editor);
                    canvas.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            top: percent(pitch_percent(ghost.midi, editor)),
                            width: percent((right - left).max(0.4)),
                            min_width: px(6),
                            height: px(18),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(2)),
                            ..default()
                        },
                        BackgroundColor(
                            editor_note_color(ghost.kind, ghost.placeholder, theme).with_alpha(0.16),
                        ),
                        BorderColor::all(
                            editor_note_color(ghost.kind, ghost.placeholder, theme).with_alpha(0.4),
                        ),
                        UiTransform::from_xy(px(0), px(-9)),
                        ZIndex(0),
                        Pickable::IGNORE,
                    ));
                }
                for note in notes.iter().filter(|note| {
                    note.end >= editor.viewport_start && note.start <= editor.viewport_end()
                }) {
                    let left = time_percent(note.start, editor);
                    let right = time_percent(note.end, editor);
                    let width = (right - left).max(0.4);
                    let top = pitch_percent(note.midi, editor);
                    let selected = editor.selected_notes.contains(&note.index)
                        || editor.selected_note == Some(note.index);
                    // Reads the same as `selected`, dimmer, to show the note
                    // a selected lyric is bound to without implying it was
                    // the thing actually clicked.
                    let bound_highlight = !selected && bound_notes.contains(&note.index);
                    let active =
                        editor.visible_position >= note.start && editor.visible_position < note.end;
                    let note_color = editor_note_color(note.kind, note.placeholder, theme);
                    canvas
                        .spawn((
                            Button,
                            EditorNoteNode(note.index),
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(left),
                                top: percent(top),
                                width: percent(width),
                                min_width: px(6),
                                height: px(18),
                                align_items: AlignItems::Center,
                                padding: UiRect::horizontal(px(6)),
                                border: UiRect::all(px(1)),
                                border_radius: BorderRadius::all(px(2)),
                                ..default()
                            },
                            BackgroundColor(if selected {
                                theme.editor_selection.with_alpha(0.9)
                            } else if bound_highlight {
                                theme.editor_selection.with_alpha(0.55)
                            } else if active {
                                theme.primary.with_alpha(0.86)
                            } else {
                                // A note with no pitch target reads as guidance
                                // rather than something to hit.
                                note_color.with_alpha(if note.pitched { 0.98 } else { 0.72 })
                            }),
                            BorderColor::all(if selected || bound_highlight {
                                theme.editor_selection.with_alpha(1.0)
                            } else if active {
                                theme.primary.with_alpha(1.0)
                            } else {
                                note_color.with_alpha(1.0)
                            }),
                            BoxShadow::new(
                                if selected || bound_highlight || active {
                                    Color::srgba(0.0, 0.0, 0.0, 0.28)
                                } else {
                                    Color::NONE
                                },
                                px(0),
                                px(3),
                                px(7),
                                px(-2),
                            ),
                            UiTransform::from_xy(px(0), px(-9)),
                            ZIndex(if selected || bound_highlight || active { 2 } else { 1 }),
                        ))
                        .with_children(|note_node| {
                            if width >= 2.6 {
                                // A note's own syllable is more useful to
                                // read at a glance than its pitch name; a
                                // continuation shows as a held-note mark, and
                                // a note with neither dims to flag that it's
                                // not singable as-is (the same condition the
                                // "lyric without pitch" chart check watches).
                                let has_lyric = note.continues_lyric || note.lyric.is_some();
                                let label = if note.continues_lyric {
                                    "~".to_string()
                                } else if let Some(lyric) = note.lyric.as_deref() {
                                    lyric.to_string()
                                } else {
                                    midi_note_name(note.midi)
                                };
                                note_node.spawn((
                                    Text::new(label),
                                    ui_text_font(font.clone(), 8.0),
                                    TextColor(if selected {
                                        theme.background
                                    } else if active {
                                        theme.primary_foreground
                                    } else if !has_lyric {
                                        theme.muted_foreground.with_alpha(0.75)
                                    } else if theme.dark {
                                        theme.foreground.with_alpha(0.96)
                                    } else {
                                        theme.primary_foreground.with_alpha(0.96)
                                    }),
                                    TextLayout::no_wrap(),
                                    ZIndex(2),
                                    Pickable::IGNORE,
                                ));
                            }
                            for (edge, left, right) in [
                                (NoteEdge::Start, Some(px(-3)), None),
                                (NoteEdge::End, None, Some(px(-3))),
                            ] {
                                note_node.spawn((
                                    Button,
                                    EditorNoteResizeHandle {
                                        index: note.index,
                                        edge,
                                    },
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: left.unwrap_or_default(),
                                        right: right.unwrap_or_default(),
                                        top: px(-2),
                                        bottom: px(-2),
                                        width: px(8),
                                        border_radius: BorderRadius::all(px(3)),
                                        ..default()
                                    },
                                    BackgroundColor(if selected {
                                        theme.background.with_alpha(0.62)
                                    } else {
                                        Color::NONE
                                    }),
                                ));
                            }
                        })
                        .observe({
                            let note_index = note.index;
                            move |mut event: On<Pointer<Click>>,
                                  mut session: ResMut<StudioSession>,
                                  mut invalidated: ResMut<UiInvalidated>| {
                                event.propagate(false);
                                open_note_from_click(&event, note_index, &mut session, &mut invalidated);
                            }
                        });
                }
                let playhead = time_percent(editor.visible_position, editor);
                spawn_editor_alignment_guide(canvas, theme, 42);
                spawn_editor_binding_guide(canvas, theme, EditorBindingGuidePart::Canvas);
                canvas.spawn((
                    EditorPlayhead,
                    Node {
                        position_type: PositionType::Absolute,
                        left: percent(playhead),
                        top: px(0),
                        bottom: px(0),
                        width: px(1.5),
                        ..default()
                    },
                    BackgroundColor(theme.primary.with_alpha(0.94)),
                    ZIndex(3),
                    Pickable::IGNORE,
                ));
                // Shown and positioned by `handle_editor_pointer_capture`
                // while shift-dragging a marquee selection.
                canvas.spawn((
                    EditorMarqueeBox,
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: px(0),
                        width: px(0),
                        height: px(0),
                        border: UiRect::all(px(1)),
                        display: Display::None,
                        ..default()
                    },
                    BackgroundColor(theme.editor_selection.with_alpha(0.14)),
                    BorderColor::all(theme.editor_selection.with_alpha(0.85)),
                    ZIndex(4),
                    Pickable::IGNORE,
                ));
            })
            .observe(
                move |mut event: On<Pointer<Click>>,
                      mut session: ResMut<StudioSession>,
                      mut invalidated: ResMut<UiInvalidated>,
                      canvas: Query<
                    (&ComputedNode, &UiGlobalTransform),
                    With<EditorTimelineSurface>,
                >| {
                    open_waveform_menu_from_click(&event, &canvas, &mut session, &mut invalidated);
                    event.propagate(false);
                },
            );
        });
}

/// Right-clicking inside the waveform header strip (the top
/// `EDITOR_PITCH_TOP_PERCENT` of the pitch canvas) opens a menu to pick which
/// stem it's decoded from and how its peaks are drawn. A right-click lower in
/// the canvas, over the actual note grid, is left alone.
fn open_waveform_menu_from_click(
    event: &Pointer<Click>,
    canvas: &Query<(&ComputedNode, &UiGlobalTransform), With<EditorTimelineSurface>>,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let Ok((computed, transform)) = canvas.single() else {
        return;
    };
    let size = computed.size() * computed.inverse_scale_factor();
    if size.y <= 1.0 {
        return;
    }
    let local = transform
        .affine()
        .inverse()
        .transform_point2(event.pointer_location.position);
    let fraction_y = (local.y / size.y + 0.5).clamp(0.0, 1.0);
    if fraction_y * 100.0 > EDITOR_PITCH_TOP_PERCENT {
        return;
    }
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    editor.waveform_context = Some(WaveformContextMenu {
        position: event.pointer_location.position,
    });
    invalidated.0 = true;
}

/// Anchors a context menu at `pointer` (window coordinates — the editor has
/// no sidebar and its own toolbar is inside `editor_root`, so no offset is
/// needed to reach `editor_root`-local space) without letting it run off the
/// bottom or right edge. `menu_size` is a rough estimate of the menu's own
/// footprint — the exact height depends on how many rows it ends up with —
/// generous is fine, this only needs to keep the menu from being clipped.
fn clamp_menu_position(pointer: Vec2, window_size: Vec2, menu_size: Vec2) -> (f32, f32) {
    let left = pointer
        .x
        .min((window_size.x - menu_size.x - 8.0).max(8.0))
        .max(8.0);
    let top = pointer
        .y
        .min((window_size.y - menu_size.y - 8.0).max(8.0))
        .max(8.0);
    (left, top)
}

pub(crate) fn spawn_waveform_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
    context: &WaveformContextMenu,
    window_size: Vec2,
) {
    parent.spawn((
        Button,
        UiAction::DismissWaveformContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let (left, top) =
        clamp_menu_position(context.position, window_size, Vec2::new(190.0, 230.0));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(190),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(1),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(menu, font.clone(), "WAVEFORM TRACK", 8.0, theme.muted_foreground);
            for (label, source) in [
                ("Instrumental", WaveformSource::Instrumental),
                ("Vocals", WaveformSource::Vocals),
                ("Original", WaveformSource::Original),
            ] {
                let available = source != WaveformSource::Vocals || editor.chart.audio.vocals.is_some();
                let active = editor.waveform_source == source;
                spawn_menu_check_row(
                    menu,
                    font.clone(),
                    theme,
                    label,
                    active,
                    available,
                    UiAction::SelectWaveformSource(source),
                );
            }
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(4)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text(menu, font.clone(), "WAVEFORM STYLE", 8.0, theme.muted_foreground);
            for (label, style) in [
                ("Bars", WaveformStyle::Bars),
                ("Filled", WaveformStyle::Filled),
                ("Line", WaveformStyle::Line),
            ] {
                spawn_menu_check_row(
                    menu,
                    font.clone(),
                    theme,
                    label,
                    editor.waveform_style == style,
                    true,
                    UiAction::SelectWaveformStyle(style),
                );
            }
        });
}

/// A menu row with a leading check mark for the active choice, dimmed and
/// inert when `available` is false (e.g. picking vocals on a chart with no
/// separate vocal stem).
pub(crate) fn spawn_menu_check_row(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    active: bool,
    available: bool,
    action: UiAction,
) {
    let color = if !available {
        theme.muted_foreground.with_alpha(0.5)
    } else if active {
        theme.primary
    } else {
        theme.foreground
    };
    let mut row = parent.spawn((
        Node {
            width: percent(100),
            height: px(24),
            align_items: AlignItems::Center,
            padding: UiRect::horizontal(px(6)),
            column_gap: px(6),
            border_radius: BorderRadius::all(px(4)),
            ..default()
        },
        BackgroundColor(Color::NONE),
    ));
    if available {
        row.insert((Button, action));
    }
    row.with_children(|row| {
        spawn_text(row, font.clone(), if active { "✓" } else { " " }, 10.0, color);
        spawn_text(row, font, label, 10.0, color);
    });
}

pub(crate) fn spawn_editor_lyrics(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    lyrics: &[ChartLyricView],
    // The lyric the selected note is bound to, highlighted to match it.
    bound_word: Option<WordSelection>,
    theme: &StudioTheme,
) {
    let visible_lane_count = lyrics
        .iter()
        .filter(|lyric| lyric.end >= editor.viewport_start && lyric.start <= editor.viewport_end())
        .map(|lyric| lyric.lane + 1)
        .max()
        .unwrap_or(1);
    let lane_height = (14.0 + visible_lane_count as f32 * 26.0).clamp(46.0, 14.0 + MAX_LYRIC_LANES as f32 * 26.0);
    parent
        .spawn((
            Node {
                width: percent(100),
                height: px(lane_height),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                border: UiRect::top(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.58)),
            BorderColor::all(theme.border.with_alpha(0.45)),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    width: px(EDITOR_TRACK_GUTTER_WIDTH),
                    height: percent(100),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::right(px(1)),
                    ..default()
                },
                BorderColor::all(theme.border.with_alpha(0.45)),
                children![(
                    Text::new("LYRICS"),
                    ui_text_font(font.clone(), 8.0),
                    TextColor(theme.muted_foreground),
                )],
            ));
            row.spawn((
                EditorLyricsSurface,
                Node {
                    position_type: PositionType::Relative,
                    min_width: px(0),
                    height: percent(100),
                    flex_grow: 1.0,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|lane| {
                for lyric in lyrics.iter().filter(|lyric| {
                    lyric.end >= editor.viewport_start && lyric.start <= editor.viewport_end()
                }) {
                    let left = time_percent(lyric.start, editor);
                    let right = time_percent(lyric.end, editor);
                    let selection = WordSelection {
                        segment: lyric.segment,
                        word: lyric.word,
                    };
                    let selected = editor.selected_words.contains(&selection)
                        || editor.selected_word
                            == Some(WordSelection {
                                segment: lyric.segment,
                                word: lyric.word,
                            });
                    // Reads the same as `selected`, dimmer, to show the
                    // lyric a selected note is bound to without implying it
                    // was the thing actually clicked.
                    let bound_highlight = !selected && bound_word == Some(selection);
                    let active = editor.visible_position >= lyric.start
                        && editor.visible_position < lyric.end;
                    lane.spawn((
                        Button,
                        UiAction::SelectEditorWord(
                            lyric.segment,
                            lyric.word,
                            (lyric.start.max(0.0) * 1000.0).round() as u64,
                        ),
                        EditorLyricNode { selection },
                        Node {
                            position_type: PositionType::Absolute,
                            left: percent(left),
                            top: px(6.0 + lyric.lane as f32 * 26.0),
                            width: percent((right - left).max(1.5)),
                            min_width: px(26),
                            height: px(22),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::FlexStart,
                            padding: UiRect::horizontal(px(7)),
                            margin: UiRect::horizontal(px(1)),
                            overflow: Overflow::clip(),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(4)),
                            ..default()
                        },
                        BackgroundColor(if selected {
                            theme.editor_selection.with_alpha(0.28)
                        } else if bound_highlight {
                            theme.editor_selection.with_alpha(0.16)
                        } else if active {
                            theme.primary.with_alpha(0.22)
                        } else if lyric.guided {
                            theme.muted.with_alpha(if theme.dark { 0.34 } else { 0.74 })
                        } else {
                            theme
                                .editor_warning
                                .with_alpha(if theme.dark { 0.07 } else { 0.045 })
                        }),
                        BorderColor::all(if selected || bound_highlight {
                            theme.editor_selection.with_alpha(0.94)
                        } else if active {
                            theme.primary.with_alpha(0.9)
                        } else if lyric.guided {
                            theme
                                .border
                                .with_alpha(if theme.dark { 0.86 } else { 0.68 })
                        } else {
                            theme.border.with_alpha(0.7)
                        }),
                    ))
                    .with_children(|lyric_node| {
                        if !lyric.guided {
                            lyric_node.spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: px(2),
                                    top: px(4),
                                    bottom: px(4),
                                    width: px(2),
                                    border_radius: BorderRadius::MAX,
                                    ..default()
                                },
                                BackgroundColor(theme.editor_warning.with_alpha(0.9)),
                                Pickable::IGNORE,
                            ));
                        }
                        if editor.word_edit_focus == Some(selection) {
                            lyric_node.spawn((
                                InlineEditorWordInput,
                                EditorWordInput(selection),
                                EditableText {
                                    max_characters: Some(160),
                                    visible_width: Some(18.0),
                                    ..EditableText::new(&lyric.text)
                                },
                                Node {
                                    width: percent(100),
                                    min_width: px(0),
                                    height: percent(100),
                                    align_items: AlignItems::Center,
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                ui_text_font(font.clone(), 9.0),
                                TextColor(theme.foreground),
                                TextCursorStyle {
                                    color: theme.editor_selection,
                                    selected_text_color: Some(theme.primary_foreground),
                                    ..default()
                                },
                                BackgroundColor(theme.background.with_alpha(0.72)),
                                TabIndex(0),
                                AutoFocus,
                            ));
                        } else {
                            lyric_node.spawn((
                                Text::new(lyric.text.clone()),
                                ui_text_font(font.clone(), 10.0),
                                TextColor(if selected || active || lyric.guided {
                                    theme.foreground
                                } else {
                                    theme.foreground.with_alpha(0.84)
                                }),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        }
                        if selected {
                            for (edge, left, right) in [
                                (NoteEdge::Start, Some(px(0)), None),
                                (NoteEdge::End, None, Some(px(0))),
                            ] {
                                lyric_node.spawn((
                                    Button,
                                    EditorLyricResizeHandle { selection, edge },
                                    Node {
                                        position_type: PositionType::Absolute,
                                        left: left.unwrap_or_default(),
                                        right: right.unwrap_or_default(),
                                        top: px(1),
                                        bottom: px(1),
                                        width: px(7),
                                        border_radius: BorderRadius::all(px(2)),
                                        ..default()
                                    },
                                    BackgroundColor(theme.editor_selection.with_alpha(0.78)),
                                ));
                            }
                        }
                    })
                    .observe(
                        move |mut event: On<Pointer<Click>>,
                              mut session: ResMut<StudioSession>,
                              mut invalidated: ResMut<UiInvalidated>| {
                            event.propagate(false);
                            open_lyric_from_click(&event, selection, &mut session, &mut invalidated);
                        },
                    );
                }
                spawn_editor_alignment_guide(lane, theme, 8);
                spawn_editor_binding_guide(lane, theme, EditorBindingGuidePart::Lane);
            })
            .observe(
                |event: On<Pointer<Click>>,
                 mut session: ResMut<StudioSession>,
                 mut invalidated: ResMut<UiInvalidated>| {
                    // Individual lyric words stop propagation on their own
                    // click, so only a click on the bare lane reaches here.
                    if event.button != PointerButton::Primary {
                        return;
                    }
                    if let Some(editor) = session.editor.as_mut() {
                        editor.clear_selection();
                        invalidated.0 = true;
                    }
                },
            );
        });
}

/// Right-clicking a note selects it (unless it's already part of the current
/// selection, so a Shift-multi-select survives the click) and opens the
/// context menu at the cursor.
fn open_note_from_click(
    event: &Pointer<Click>,
    note_index: usize,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    // Captured before selecting the note (below) replaces it: a syllable
    // selected beforehand is what this note could extend into a held
    // continuation, offered in the menu when eligible.
    let continue_word = editor
        .selected_word
        .filter(|word| can_extend_editor_lyric(&editor.document, *word, note_index));
    if editor.selected_note != Some(note_index) && !editor.selected_notes.contains(&note_index) {
        editor.select_only_note(note_index);
    }
    editor.note_context = Some(NoteContextMenu {
        position: event.pointer_location.position,
        continue_word,
    });
    invalidated.0 = true;
}

pub(crate) fn spawn_note_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
    context: &NoteContextMenu,
    window_size: Vec2,
) {
    parent.spawn((
        Button,
        UiAction::DismissNoteContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let count = editor.selected_note_indices().len().max(1);
    let (left, top) =
        clamp_menu_position(context.position, window_size, Vec2::new(190.0, 280.0));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(190),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(1),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                if count > 1 {
                    format!("{count} notes selected")
                } else {
                    "Pitch note".to_string()
                },
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(3),
                ..default()
            });
            if let Some(word) = context.continue_word
                && let Some(note_index) = editor.selected_note
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Continue syllable here",
                    10.0,
                    UiAction::ExtendLyricOverNote(word, note_index),
                );
                menu.spawn((
                    Node {
                        height: px(1),
                        margin: UiRect::vertical(px(3)),
                        ..default()
                    },
                    BackgroundColor(theme.border.with_alpha(0.5)),
                ));
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Play pitch",
                10.0,
                UiAction::Editor(EditorAction::PlayNotePitch),
            );
            if editor.chart.audio.vocals.is_some() {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Play vocal",
                    10.0,
                    UiAction::Editor(EditorAction::PlayNoteVocal),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split note",
                10.0,
                UiAction::Editor(EditorAction::SplitSelection),
            );
            if count > 1 {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Merge into one note",
                    10.0,
                    UiAction::Editor(EditorAction::MergeSelection),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Duplicate",
                10.0,
                UiAction::Editor(EditorAction::DuplicateNotes),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Copy",
                10.0,
                UiAction::Editor(EditorAction::CopyNotes),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Quantize",
                10.0,
                UiAction::Editor(EditorAction::QuantizeNotes),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Cycle note type",
                10.0,
                UiAction::Editor(EditorAction::CycleNoteKind),
            );
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text_button(
                menu,
                font,
                theme,
                "Delete",
                10.0,
                UiAction::Editor(EditorAction::DeleteSelection),
            );
        });
}

/// Right-clicking a lyric selects it (unless it's already part of the current
/// selection, so a Ctrl-multi-select survives the click) and opens the
/// context menu at the cursor.
fn open_lyric_from_click(
    event: &Pointer<Click>,
    selection: WordSelection,
    session: &mut StudioSession,
    invalidated: &mut UiInvalidated,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    if editor.selected_word != Some(selection) && !editor.selected_words.contains(&selection) {
        editor.select_only_word(selection);
    }
    editor.lyric_context = Some(LyricContextMenu {
        position: event.pointer_location.position,
    });
    invalidated.0 = true;
}

pub(crate) fn spawn_lyric_context_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
    context: &LyricContextMenu,
    window_size: Vec2,
) {
    parent.spawn((
        Button,
        UiAction::DismissLyricContext,
        Node {
            position_type: PositionType::Absolute,
            left: px(0),
            right: px(0),
            top: px(0),
            bottom: px(0),
            ..default()
        },
        BackgroundColor(Color::NONE),
        ZIndex(40),
    ));
    let word_count = editor.selected_word_indices().len().max(1);
    let (left, top) =
        clamp_menu_position(context.position, window_size, Vec2::new(200.0, 280.0));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(200),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(6)),
                row_gap: px(1),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(6)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                if word_count > 1 {
                    format!("{word_count} words selected")
                } else {
                    "Lyric word".to_string()
                },
                8.0,
                theme.muted_foreground,
            );
            menu.spawn(Node {
                height: px(3),
                ..default()
            });
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                if word_count > 1 {
                    "Merge into one word"
                } else {
                    "Merge with next word"
                },
                10.0,
                UiAction::Editor(EditorAction::MergeLyrics),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split word",
                10.0,
                UiAction::Editor(EditorAction::SplitLyrics),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split into syllables",
                10.0,
                UiAction::Editor(EditorAction::SyllabizeLyrics),
            );
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Bind to nearest note",
                10.0,
                UiAction::Editor(EditorAction::BindNearest),
            );
            if word_count == 1
                && let Some(word) = editor.selected_word
                && let Some(next_note) = next_extendable_editor_note(&editor.document, word)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Extend onto next note",
                    10.0,
                    UiAction::ExtendLyricOverNote(word, next_note),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Unbind from note",
                10.0,
                UiAction::Editor(EditorAction::UnbindSelection),
            );
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text_button(
                menu,
                font,
                theme,
                "Delete",
                10.0,
                UiAction::Editor(EditorAction::DeleteLyrics),
            );
        });
}

pub(crate) fn spawn_editor_alignment_guide(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    dash_count: usize,
) {
    parent
        .spawn((
            EditorAlignmentGuide,
            Node {
                position_type: PositionType::Absolute,
                left: percent(0),
                top: px(0),
                bottom: px(0),
                width: px(2),
                display: Display::None,
                ..default()
            },
            ZIndex(5),
            Pickable::IGNORE,
        ))
        .with_children(|guide| {
            for index in 0..dash_count {
                guide.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(0),
                        top: percent(index as f32 / dash_count as f32 * 100.0),
                        width: px(1.5),
                        height: px(4),
                        ..default()
                    },
                    BackgroundColor(theme.editor_selection.with_alpha(0.92)),
                    Pickable::IGNORE,
                ));
            }
        });
}

/// A short vertical mark at the shared start time of a bound note and lyric,
/// placed in one of three containers — see `EditorBindingGuidePart`.
/// `update_editor_binding_guides` positions and sizes each part every frame
/// so together they read as one line from the bound note's own pitch height,
/// through the gap, down to the bound word's own lane.
pub(crate) fn spawn_editor_binding_guide(
    parent: &mut ChildSpawnerCommands,
    theme: &StudioTheme,
    part: EditorBindingGuidePart,
) {
    parent.spawn((
        EditorBindingGuide,
        part,
        Node {
            position_type: PositionType::Absolute,
            left: percent(0),
            top: px(0),
            height: px(0),
            width: px(3),
            display: Display::None,
            ..default()
        },
        BackgroundColor(theme.editor_selection),
        ZIndex(6),
        Pickable::IGNORE,
    ));
}

pub(crate) fn update_editor_geometry(
    session: Res<StudioSession>,
    capture: Res<EditorPointerCapture>,
    mut note_nodes: Query<(&EditorNoteNode, &mut Node)>,
    mut lyric_nodes: Query<(&EditorLyricNode, &mut Node), Without<EditorNoteNode>>,
    mut alignment_guides: Query<
        &mut Node,
        (
            With<EditorAlignmentGuide>,
            Without<EditorNoteNode>,
            Without<EditorLyricNode>,
        ),
    >,
) {
    let Some(editor) = session.editor.as_ref() else {
        return;
    };
    let notes = chart_notes(&editor.document);
    for (marker, mut node) in &mut note_nodes {
        let Some(note) = notes.iter().find(|note| note.index == marker.0) else {
            node.display = Display::None;
            continue;
        };
        if note.end < editor.viewport_start || note.start > editor.viewport_end() {
            node.display = Display::None;
            continue;
        }
        let left = time_percent(note.start, editor);
        let right = time_percent(note.end, editor);
        node.display = Display::Flex;
        node.left = percent(left);
        node.top = percent(pitch_percent(note.midi, editor));
        node.width = percent((right - left).max(0.4));
    }
    for (marker, mut node) in &mut lyric_nodes {
        let Some((_, start, end)) = selected_editor_word(&editor.document, marker.selection) else {
            node.display = Display::None;
            continue;
        };
        if end < editor.viewport_start || start > editor.viewport_end() {
            node.display = Display::None;
            continue;
        }
        let left = time_percent(start, editor);
        let right = time_percent(end, editor);
        node.display = Display::Flex;
        node.left = percent(left);
        node.width = percent((right - left).max(1.8));
    }
    for mut node in &mut alignment_guides {
        if let Some(time) = capture.alignment_guide
            && time >= editor.viewport_start
            && time <= editor.viewport_end()
        {
            node.display = Display::Flex;
            node.left = percent(time_percent(time, editor));
        } else {
            node.display = Display::None;
        }
    }
}

pub(crate) fn update_editor_playhead(
    session: Res<StudioSession>,
    mut playheads: Query<&mut Node, With<EditorPlayhead>>,
    mut clocks: Query<&mut Text, With<EditorClockText>>,
) {
    let Some(editor) = session.editor.as_ref() else {
        return;
    };
    let position = time_percent(editor.visible_position, editor);
    for mut node in &mut playheads {
        node.left = percent(position);
    }
    let label = format_editor_clock(editor.visible_position, editor.audio_status.duration_secs);
    for mut text in &mut clocks {
        **text = label.clone();
    }
}

/// Highlights the shared time of the current selection's bound note and
/// lyric by sizing the three `EditorBindingGuide` parts (see
/// `EditorBindingGuidePart`) so the line runs from the note's own pitch
/// height down to the lyric's own lane — plain percent/px positioning within
/// each part's own container, the same approach `update_editor_geometry`
/// uses for notes, lyrics, and the alignment guide. No world-space
/// transform math.
pub(crate) fn update_editor_binding_guides(
    session: Res<StudioSession>,
    mut guides: Query<(&EditorBindingGuidePart, &mut Node), With<EditorBindingGuide>>,
) {
    let hide = |guides: &mut Query<(&EditorBindingGuidePart, &mut Node), With<EditorBindingGuide>>| {
        for (_, mut node) in guides.iter_mut() {
            node.display = Display::None;
        }
    };
    let Some(editor) = session.editor.as_ref() else {
        hide(&mut guides);
        return;
    };

    let lyrics = chart_lyrics(&editor.document);
    let lyric = if let Some(word) = editor.selected_word {
        lyrics
            .iter()
            .find(|lyric| lyric.segment == word.segment && lyric.word == word.word && lyric.guided)
    } else {
        editor
            .selected_note
            .and_then(|note_index| lyrics.iter().find(|lyric| lyric.note == note_index))
    };
    let Some(lyric) = lyric.filter(|lyric| {
        lyric.start >= editor.viewport_start && lyric.start <= editor.viewport_end()
    }) else {
        hide(&mut guides);
        return;
    };

    let left = percent(time_percent(lyric.start, editor));
    let note_top = chart_notes(&editor.document)
        .iter()
        .find(|note| note.index == lyric.note)
        .map(|note| pitch_percent(note.midi, editor))
        .unwrap_or(50.0);
    // Lane rows start 6px down and are spaced 26px apart (see
    // `spawn_editor_lyrics`); the lyric's own vertical center sits 11px
    // (half its 22px height) into its row.
    let lane_center = 6.0 + lyric.lane as f32 * 26.0 + 11.0;
    for (part, mut node) in &mut guides {
        node.display = Display::Flex;
        node.left = left;
        match part {
            EditorBindingGuidePart::Canvas => {
                node.top = percent(note_top);
                node.height = percent((100.0 - note_top).max(0.0));
            }
            EditorBindingGuidePart::Gap => {
                node.top = px(0);
                node.height = percent(100);
            }
            EditorBindingGuidePart::Lane => {
                node.top = px(0);
                node.height = px(lane_center);
            }
        }
    }
}
