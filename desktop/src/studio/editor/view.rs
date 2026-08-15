//! Editor rendering: chrome, dock, timeline, lyric lane, and playhead.

use crate::studio::*;

pub(crate) fn spawn_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    icons: Handle<Image>,
    session: &StudioSession,
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
    let lyrics = chart_lyrics(&editor.document);

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
                        padding: UiRect {
                            left: px(12),
                            right: px(44),
                            ..default()
                        },
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
                            spawn_editor_timeline(
                                timeline_column,
                                font.clone(),
                                editor,
                                &notes,
                                theme,
                            );
                            if !editor.lyrics_hidden {
                                spawn_editor_lyrics(
                                    timeline_column,
                                    font.clone(),
                                    editor,
                                    &lyrics,
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
                                    spawn_text(
                                        status,
                                        font.clone(),
                                        "Double-click lyric to edit · drag edges to resize · wheel / Shift / Ctrl / Alt to navigate",
                                        8.0,
                                        theme.muted_foreground,
                                    );
                                });
                        });
                    if editor.inspector_open {
                        spawn_editor_inspector(workspace, font.clone(), editor, &notes, theme);
                    }
                });

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
                        "Note",
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
                            BackgroundColor(if black_key {
                                theme.background.with_alpha(0.96)
                            } else {
                                theme.foreground.with_alpha(0.055)
                            }),
                            BorderColor::all(theme.border.with_alpha(if black_key {
                                0.5
                            } else {
                                0.32
                            })),
                        ))
                        .with_children(|key| {
                            if midi.rem_euclid(12) == 0 {
                                key.spawn((
                                    Text::new(midi_note_name(f64::from(midi))),
                                    ui_text_font(font.clone(), 6.5),
                                    TextColor(theme.muted_foreground.with_alpha(0.82)),
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
                            BackgroundColor(theme.card.with_alpha(0.24)),
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
                    let stride = visible_peaks.len().div_ceil(360).max(1);
                    for (time, (minimum, maximum)) in visible_peaks.into_iter().step_by(stride) {
                        let amplitude = (maximum - minimum).abs().clamp(0.01, 2.0);
                        canvas.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                left: percent(time_percent(time, editor)),
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
                let pitch_frames = chart_pitch_frames(&editor.chart);
                for note in notes.iter().filter(|note| {
                    note.end >= editor.viewport_start && note.start <= editor.viewport_end()
                }) {
                    let left = time_percent(note.start, editor);
                    let right = time_percent(note.end, editor);
                    let width = (right - left).max(0.4);
                    let top = pitch_percent(note.midi, editor);
                    let selected = editor.selected_notes.contains(&note.index)
                        || editor.selected_note == Some(note.index);
                    let active =
                        editor.visible_position >= note.start && editor.visible_position < note.end;
                    let note_frames = pitch_frames
                        .iter()
                        .filter(|frame| {
                            frame.time >= note.start
                                && frame.time <= note.end
                                && (frame.midi - note.midi).abs() <= 4.0
                                && frame.confidence >= 0.18
                        })
                        .cloned()
                        .collect::<Vec<_>>();
                    let max_points =
                        (((note.end - note.start) * 16.0).ceil() as usize).clamp(2, 14);
                    let contour = abstract_pitch_contour(&note_frames, max_points);
                    let note_color = editor_note_color(note.kind, theme);
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
                            } else if active {
                                theme.primary.with_alpha(0.86)
                            } else {
                                // A note with no pitch target reads as guidance
                                // rather than something to hit.
                                note_color.with_alpha(if note.pitched { 0.98 } else { 0.72 })
                            }),
                            BorderColor::all(if selected {
                                theme.editor_selection.with_alpha(1.0)
                            } else if active {
                                theme.primary.with_alpha(1.0)
                            } else {
                                note_color.with_alpha(1.0)
                            }),
                            BoxShadow::new(
                                if selected || active {
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
                            ZIndex(if selected || active { 2 } else { 1 }),
                        ))
                        .with_children(|note_node| {
                            if width >= 2.6 {
                                note_node.spawn((
                                    Text::new(midi_note_name(note.midi)),
                                    ui_text_font(font.clone(), 8.0),
                                    TextColor(if selected {
                                        theme.background
                                    } else if active {
                                        theme.primary_foreground
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
                            let duration = (note.end - note.start).max(0.001);
                            if width >= 2.2 && contour.len() > 1 {
                                for (index, point) in contour.iter().enumerate() {
                                    let point_left = (((point.time - note.start) / duration)
                                        * 100.0)
                                        .clamp(0.0, 100.0)
                                        as f32;
                                    let point_right = contour
                                        .get(index + 1)
                                        .map(|next| {
                                            (((next.time - note.start) / duration) * 100.0)
                                                .clamp(0.0, 100.0)
                                                as f32
                                        })
                                        .unwrap_or(100.0);
                                    let point_top = (50.0 - (point.midi - note.midi) as f32 * 10.0)
                                        .clamp(12.0, 88.0);
                                    let color = if selected {
                                        theme.background.with_alpha(0.72)
                                    } else if active {
                                        theme.primary_foreground.with_alpha(0.8)
                                    } else {
                                        theme
                                            .pitch_contour
                                            .with_alpha((0.46 + point.confidence * 0.34) as f32)
                                    };
                                    note_node.spawn((
                                        Node {
                                            position_type: PositionType::Absolute,
                                            left: percent(point_left),
                                            top: percent(point_top),
                                            width: percent((point_right - point_left).max(0.8)),
                                            height: px(1.2),
                                            border_radius: BorderRadius::MAX,
                                            ..default()
                                        },
                                        BackgroundColor(color),
                                        ZIndex(1),
                                        Pickable::IGNORE,
                                    ));
                                }
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
                        });
                }
                let playhead = time_percent(editor.visible_position, editor);
                spawn_editor_alignment_guide(canvas, theme, 42);
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
            });
        });
}

pub(crate) fn spawn_editor_lyrics(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    lyrics: &[ChartLyricView],
    theme: &StudioTheme,
) {
    let visible_lane_count = lyrics
        .iter()
        .filter(|lyric| lyric.end >= editor.viewport_start && lyric.start <= editor.viewport_end())
        .map(|lyric| lyric.lane + 1)
        .max()
        .unwrap_or(1);
    let lane_height = (14.0 + visible_lane_count as f32 * 26.0).clamp(46.0, 92.0);
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
                        } else if active {
                            theme.primary.with_alpha(0.22)
                        } else if lyric.guided {
                            theme.muted.with_alpha(if theme.dark { 0.34 } else { 0.74 })
                        } else {
                            theme
                                .editor_warning
                                .with_alpha(if theme.dark { 0.07 } else { 0.045 })
                        }),
                        BorderColor::all(if selected {
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
                    });
                }
                spawn_editor_alignment_guide(lane, theme, 8);
            });
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
