use super::*;
use crate::studio::*;

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
    let lane_height =
        (14.0 + visible_lane_count as f32 * 26.0).clamp(46.0, 14.0 + MAX_LYRIC_LANES as f32 * 26.0);
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
                UiPointerApi(&["ui.pointer.editor_lane.primary"]),
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
                        UiAction::from(EditorCommand::SelectEditorWord(
                            lyric.segment,
                            lyric.word,
                            (lyric.start.max(0.0) * 1000.0).round() as u64,
                        )),
                        UiPointerApi(&[
                            "ui.pointer.editor_lyric.primary",
                            "ui.pointer.editor_lyric.secondary",
                            "ui.pointer.editor_lyric_drag",
                        ]),
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
                                    UiPointerApi(&["ui.pointer.editor_lyric_resize"]),
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
                              mut editor_state: ResMut<EditorUiState>,
                              mut invalidated: ResMut<UiInvalidated>| {
                            event.propagate(false);
                            open_lyric_from_click(
                                &event,
                                selection,
                                &mut editor_state,
                                &mut invalidated,
                            );
                        },
                    );
                }
                spawn_editor_alignment_guide(lane, theme, 8);
                spawn_editor_binding_guide(lane, theme, EditorBindingGuidePart::Lane);
            })
            .observe(
                |event: On<Pointer<Click>>,
                 mut editor_state: ResMut<EditorUiState>,
                 mut invalidated: ResMut<UiInvalidated>| {
                    // Individual lyric words stop propagation on their own
                    // click, so only a click on the bare lane reaches here.
                    if event.button != PointerButton::Primary {
                        return;
                    }
                    if let Some(editor) = editor_state.editor.as_mut() {
                        editor.clear_selection();
                        invalidated.invalidate(UiDirtyRegion::Editor);
                    }
                },
            );
        });
}

pub(crate) fn spawn_editor_file_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                top: px(52),
                width: px(230),
                padding: UiRect::all(px(10)),
                row_gap: px(6),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.3),
                px(0),
                px(6),
                px(18),
                px(-4),
            ),
            ZIndex(30),
        ))
        .with_children(|menu| {
            spawn_text(menu, font.clone(), "FILES", 8.0, theme.muted_foreground);
            for (label, action) in [
                (
                    if editor.dirty {
                        "Save changes *"
                    } else {
                        "Save changes"
                    },
                    UiAction::from(EditorCommand::Editor(EditorAction::Save)),
                ),
                (
                    "Save as UTZ…",
                    UiAction::from(EditorCommand::SaveEditorAsUtz),
                ),
                (
                    "Save as UltraStar…",
                    UiAction::from(EditorCommand::SaveEditorAsUltraStar),
                ),
                (
                    "Song information…",
                    UiAction::from(EditorCommand::OpenSongSettings(
                        editor.chart.file_hash.clone(),
                    )),
                ),
            ] {
                spawn_text_button(menu, font.clone(), theme, label, 9.0, action);
            }
            spawn_text_button(
                menu,
                font,
                theme,
                "Done",
                9.0,
                UiAction::from(EditorCommand::DismissEditorFileMenu),
            );
        });
}

pub(crate) fn spawn_editor_layout_menu(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
) {
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                top: px(52),
                width: px(230),
                padding: UiRect::all(px(10)),
                row_gap: px(4),
                flex_direction: FlexDirection::Column,
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border),
            BoxShadow::new(
                Color::srgba(0.0, 0.0, 0.0, 0.3),
                px(0),
                px(6),
                px(18),
                px(-4),
            ),
            ZIndex(30),
        ))
        .with_children(|menu| {
            spawn_text(
                menu,
                font.clone(),
                "EDITOR AREAS",
                8.0,
                theme.muted_foreground,
            );
            for (label, active, action) in [
                (
                    "Track strip",
                    !editor.tracks_hidden,
                    EditorAction::ToggleTracks,
                ),
                ("Lyrics", !editor.lyrics_hidden, EditorAction::ToggleLyrics),
                (
                    "Spectrum",
                    !editor.spectrum_hidden,
                    EditorAction::ToggleSpectrum,
                ),
                ("Controls", !editor.dock_hidden, EditorAction::ToggleDock),
                (
                    "Status bar",
                    !editor.status_hidden,
                    EditorAction::ToggleStatus,
                ),
                (
                    "Inspector",
                    editor.inspector_open,
                    EditorAction::ToggleInspector,
                ),
                (
                    "Chart checks",
                    editor.problems_panel_open,
                    EditorAction::ToggleProblemsPanel,
                ),
            ] {
                spawn_menu_check_row(
                    menu,
                    font.clone(),
                    theme,
                    label,
                    active,
                    true,
                    UiAction::from(EditorCommand::Editor(action)),
                );
            }
            spawn_text_button(
                menu,
                font,
                theme,
                "Done",
                9.0,
                UiAction::from(EditorCommand::DismissEditorLayoutMenu),
            );
        });
}

/// Right-clicking a note selects it (unless it's already part of the current
/// selection, so a Shift-multi-select survives the click) and opens the
/// context menu at the cursor.
pub(crate) fn open_note_from_click(
    event: &Pointer<Click>,
    note_index: usize,
    state: &mut EditorUiState,
    invalidated: &mut UiInvalidated,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let Some(editor) = state.editor.as_mut() else {
        return;
    };
    // Captured before selecting the note (below) replaces it: a syllable
    // selected beforehand is what this note could extend into a held
    // continuation, offered in the menu when eligible.
    let continue_word = editor
        .selected_word
        .filter(|word| can_extend_editor_lyric(&editor.document, *word, note_index));
    let selection_changed =
        editor.selected_note != Some(note_index) && !editor.selected_notes.contains(&note_index);
    if selection_changed {
        editor.select_only_note(note_index);
    }
    editor.note_context = Some(NoteContextMenu {
        position: event.pointer_location.position,
        continue_word,
    });
    if selection_changed {
        invalidated.invalidate(UiDirtyRegion::Editor);
    }
    invalidated.invalidate(UiDirtyRegion::Dialog);
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
        UiAction::from(EditorCommand::DismissNoteContext),
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
    let (left, top) = clamp_menu_position(context.position, window_size, Vec2::new(230.0, 360.0));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(230),
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
            let selected_note_view = editor.selected_note.and_then(|index| {
                chart_notes(&editor.document)
                    .into_iter()
                    .find(|note| note.index == index)
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
                    UiAction::from(EditorCommand::ExtendLyricOverNote(word, note_index)),
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
            // Only offered for a note with no lyric of its own yet — one
            // that already has text (or continues an earlier one) is edited
            // in place in the lyric lane instead.
            if selected_note_view
                .as_ref()
                .is_some_and(|note| note.lyric.is_none() && !note.continues_lyric)
            {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Edit lyric",
                    10.0,
                    UiAction::from(EditorCommand::Editor(EditorAction::EditNoteLyric)),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Play pitch",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::PlayNotePitch)),
            );
            if editor.chart.audio.vocals.is_some() {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Play vocal",
                    10.0,
                    UiAction::from(EditorCommand::Editor(EditorAction::PlayNoteVocal)),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split note",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::SplitSelection)),
            );
            if count > 1 {
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Merge into one note",
                    10.0,
                    UiAction::from(EditorCommand::Editor(EditorAction::MergeSelection)),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Duplicate",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::DuplicateNotes)),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Copy",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::CopyNotes)),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Quantize",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::QuantizeNotes)),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Unbind from lyric",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::UnbindSelection)),
            );
            if let (Some(candidate), Some(authored)) = (
                app_core::load_active_artifact(
                    &editor.chart.file_hash,
                    app_core::ArtifactKind::CandidateChart,
                ),
                app_core::load_active_artifact(
                    &editor.chart.file_hash,
                    app_core::ArtifactKind::AuthoredChart,
                ),
            ) {
                let candidate_ref = artifact_ref_from_revision(&candidate);
                let authored_ref = artifact_ref_from_revision(&authored);
                menu.spawn((
                    Node {
                        height: px(1),
                        margin: UiRect::vertical(px(3)),
                        ..default()
                    },
                    BackgroundColor(theme.border.with_alpha(0.5)),
                ));
                spawn_text(menu, font.clone(), "CANDIDATE", 8.0, theme.muted_foreground);
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Replace this phrase from candidate",
                    10.0,
                    UiAction::from(AnalysisCommand::MergeSelectedCandidatePhrase(
                        candidate_ref.clone(),
                        authored_ref.clone(),
                    )),
                );
                spawn_text_button(
                    menu,
                    font.clone(),
                    theme,
                    "Replace selected notes from candidate",
                    10.0,
                    UiAction::from(AnalysisCommand::MergeSelectedCandidateRange(
                        candidate_ref,
                        authored_ref,
                    )),
                );
            }
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(3)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text(menu, font.clone(), "TYPE", 8.0, theme.muted_foreground);
            let current_kind = selected_note_view.as_ref().map(|note| note.kind);
            for kind in [
                app_core::NoteKind::Normal,
                app_core::NoteKind::Golden,
                app_core::NoteKind::Freestyle,
                app_core::NoteKind::Rap,
                app_core::NoteKind::GoldenRap,
            ] {
                spawn_menu_check_row(
                    menu,
                    font.clone(),
                    theme,
                    &kind.label().replace('_', " "),
                    current_kind == Some(kind),
                    true,
                    UiAction::from(EditorCommand::SetNoteKind(kind)),
                );
            }
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
                UiAction::from(EditorCommand::Editor(EditorAction::DeleteSelection)),
            );
        });
}

/// Right-clicking a lyric selects it (unless it's already part of the current
/// selection, so a Ctrl-multi-select survives the click) and opens the
/// context menu at the cursor.
pub(crate) fn open_lyric_from_click(
    event: &Pointer<Click>,
    selection: WordSelection,
    state: &mut EditorUiState,
    invalidated: &mut UiInvalidated,
) {
    if event.button != PointerButton::Secondary {
        return;
    }
    let Some(editor) = state.editor.as_mut() else {
        return;
    };
    let selection_changed =
        editor.selected_word != Some(selection) && !editor.selected_words.contains(&selection);
    if selection_changed {
        editor.select_only_word(selection);
    }
    editor.lyric_context = Some(LyricContextMenu {
        position: event.pointer_location.position,
    });
    if selection_changed {
        invalidated.invalidate(UiDirtyRegion::Editor);
    }
    invalidated.invalidate(UiDirtyRegion::Dialog);
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
        UiAction::from(EditorCommand::DismissLyricContext),
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
    let (left, top) = clamp_menu_position(context.position, window_size, Vec2::new(200.0, 280.0));
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
                UiAction::from(EditorCommand::Editor(EditorAction::MergeLyrics)),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split word",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::SplitLyrics)),
            );
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Split into syllables",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::SyllabizeLyrics)),
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
                UiAction::from(EditorCommand::Editor(EditorAction::BindNearest)),
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
                    UiAction::from(EditorCommand::ExtendLyricOverNote(word, next_note)),
                );
            }
            spawn_text_button(
                menu,
                font.clone(),
                theme,
                "Unbind from note",
                10.0,
                UiAction::from(EditorCommand::Editor(EditorAction::UnbindSelection)),
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
                UiAction::from(EditorCommand::Editor(EditorAction::DeleteLyrics)),
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

type EditorAlignmentGuides<'w, 's> = Query<
    'w,
    's,
    &'static mut Node,
    (
        With<EditorAlignmentGuide>,
        Without<EditorNoteNode>,
        Without<EditorLyricNode>,
    ),
>;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorGeometrySignature {
    revision: u64,
    viewport_start: u64,
    viewport_duration: u64,
    pitch_min: u64,
    pitch_max: u64,
    alignment_guide: Option<u64>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn update_editor_geometry(
    state: Res<EditorUiState>,
    capture: Res<EditorPointerCapture>,
    mut previous: Local<Option<EditorGeometrySignature>>,
    added_notes: Query<(), Added<EditorNoteNode>>,
    added_lyrics: Query<(), Added<EditorLyricNode>>,
    added_guides: Query<(), Added<EditorAlignmentGuide>>,
    mut note_nodes: Query<(&EditorNoteNode, &mut Node)>,
    mut lyric_nodes: Query<(&EditorLyricNode, &mut Node), Without<EditorNoteNode>>,
    mut alignment_guides: EditorAlignmentGuides,
) {
    let Some(editor) = state.editor.as_ref() else {
        *previous = None;
        return;
    };
    let signature = EditorGeometrySignature {
        revision: editor.document.revision(),
        viewport_start: editor.viewport_start.to_bits(),
        viewport_duration: editor.viewport_duration.to_bits(),
        pitch_min: editor.pitch_min.to_bits(),
        pitch_max: editor.pitch_max.to_bits(),
        alignment_guide: capture.alignment_guide.map(f64::to_bits),
    };
    let has_new_nodes =
        !added_notes.is_empty() || !added_lyrics.is_empty() || !added_guides.is_empty();
    if previous.as_ref() == Some(&signature) && !has_new_nodes {
        return;
    }
    *previous = Some(signature);

    let notes = chart_notes(&editor.document);
    let notes_by_index = notes
        .iter()
        .map(|note| (note.index, note))
        .collect::<HashMap<_, _>>();
    for (marker, mut node) in &mut note_nodes {
        let Some(note) = notes_by_index.get(&marker.0) else {
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
    state: Res<EditorUiState>,
    mut previous: Local<Option<(u64, u64, u64, u64)>>,
    added_playheads: Query<(), Added<EditorPlayhead>>,
    added_clocks: Query<(), Added<EditorClockText>>,
    mut playheads: Query<&mut Node, With<EditorPlayhead>>,
    mut clocks: Query<&mut Text, With<EditorClockText>>,
) {
    let Some(editor) = state.editor.as_ref() else {
        *previous = None;
        return;
    };
    let signature = (
        editor.visible_position.to_bits(),
        editor.audio_status.duration_secs.to_bits(),
        editor.viewport_start.to_bits(),
        editor.viewport_duration.to_bits(),
    );
    if previous.as_ref() == Some(&signature)
        && added_playheads.is_empty()
        && added_clocks.is_empty()
    {
        return;
    }
    *previous = Some(signature);

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
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct EditorBindingGuideSignature {
    revision: u64,
    viewport_start: u64,
    viewport_duration: u64,
    pitch_min: u64,
    pitch_max: u64,
    selected_word: Option<WordSelection>,
    selected_note: Option<usize>,
}

pub(crate) fn update_editor_binding_guides(
    state: Res<EditorUiState>,
    mut previous: Local<Option<EditorBindingGuideSignature>>,
    added_guides: Query<(), Added<EditorBindingGuide>>,
    mut guides: Query<(&EditorBindingGuidePart, &mut Node), With<EditorBindingGuide>>,
) {
    let hide =
        |guides: &mut Query<(&EditorBindingGuidePart, &mut Node), With<EditorBindingGuide>>| {
            for (_, mut node) in guides.iter_mut() {
                node.display = Display::None;
            }
        };
    let Some(editor) = state.editor.as_ref() else {
        *previous = None;
        hide(&mut guides);
        return;
    };
    let signature = EditorBindingGuideSignature {
        revision: editor.document.revision(),
        viewport_start: editor.viewport_start.to_bits(),
        viewport_duration: editor.viewport_duration.to_bits(),
        pitch_min: editor.pitch_min.to_bits(),
        pitch_max: editor.pitch_max.to_bits(),
        selected_word: editor.selected_word,
        selected_note: editor.selected_note,
    };
    if previous.as_ref() == Some(&signature) && added_guides.is_empty() {
        return;
    }
    *previous = Some(signature);

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
