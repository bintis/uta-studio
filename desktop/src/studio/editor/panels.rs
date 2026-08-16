//! Editor side panels: inspector, problems, and history.

use crate::studio::*;

pub(crate) fn spawn_editor_inspector(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    notes: &[ChartNoteView],
    theme: &StudioTheme,
) {
    parent
        .spawn((
            Node {
                width: px(260),
                height: percent(100),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(8),
                border: UiRect::left(px(1)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.5)),
            BorderColor::all(theme.border.with_alpha(0.55)),
        ))
        .with_children(|inspector| {
            spawn_text(
                inspector,
                font.clone(),
                "CHART INSPECTOR",
                8.0,
                theme.primary,
            );
            let selected = editor.selected_note_indices();
            if selected.len() > 1 {
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("{} pitch notes", selected.len()),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Drag any selected bar to move and transpose the group. Shift-click adds or removes notes; Shift-drag draws a selection rectangle.",
                    10.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Cycle note type", EditorAction::CycleNoteKind),
                    ("Split selection", EditorAction::SplitSelection),
                    ("Merge selection", EditorAction::MergeSelection),
                    ("Quantize selection", EditorAction::QuantizeNotes),
                    ("Duplicate selection", EditorAction::DuplicateNotes),
                    ("Copy selection", EditorAction::CopyNotes),
                    ("Delete selection", EditorAction::DeleteSelection),
                ] {
                    spawn_editor_action_button(inspector, font.clone(), theme, label, action);
                }
            } else if let Some(note) = editor
                .selected_note
                .and_then(|index| notes.iter().find(|note| note.index == index))
            {
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("MIDI {:.0}", note.midi),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    format!(
                        "{:.3}s – {:.3}s\nType: {}\nDrag to change time and pitch.",
                        note.start, note.end, note.kind.label()
                    ),
                    10.0,
                    theme.muted_foreground,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Cycle note type",
                    EditorAction::CycleNoteKind,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Split at playhead",
                    EditorAction::SplitSelection,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Quantize note",
                    EditorAction::QuantizeNotes,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Duplicate note",
                    EditorAction::DuplicateNotes,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Copy note",
                    EditorAction::CopyNotes,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Delete note",
                    EditorAction::DeleteSelection,
                );
                if !editor.clipboard_notes.is_empty() {
                    spawn_editor_action_button(
                        inspector,
                        font.clone(),
                        theme,
                        "Paste at playhead",
                        EditorAction::PasteNotes,
                    );
                }
            } else if editor.selected_word_indices().len() > 1 {
                let count = editor.selected_word_indices().len();
                spawn_text(
                    inspector,
                    font.clone(),
                    format!("{count} lyric words"),
                    17.0,
                    theme.foreground,
                );
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Shift/Ctrl-click toggles words. Timing moves apply to the whole selection; merge requires words from one phrase.",
                    9.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Move selection −10 ms", UiAction::Editor(EditorAction::ShiftLyricEarlier)),
                    ("Move selection +10 ms", UiAction::Editor(EditorAction::ShiftLyricLater)),
                    (
                        "Split into syllables",
                        UiAction::Editor(EditorAction::SyllabizeLyrics),
                    ),
                    ("Split selected words", UiAction::Editor(EditorAction::SplitLyrics)),
                    ("Merge selected words", UiAction::Editor(EditorAction::MergeLyrics)),
                    ("Delete selected words", UiAction::Editor(EditorAction::DeleteLyrics)),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
            } else if let Some(selection) = editor.selected_word
                && let Some((text, start, end)) = selected_editor_word(&editor.document, selection)
            {
                spawn_text(
                    inspector,
                    font.clone(),
                    "Lyric word",
                    17.0,
                    theme.foreground,
                );
                let guided = chart_lyrics(&editor.document)
                    .iter()
                    .find(|lyric| lyric.segment == selection.segment && lyric.word == selection.word)
                    .map(|lyric| lyric.guided)
                    .unwrap_or(true);
                if !guided {
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        "No pitch note yet, so this word won't be scored. Drag it onto a note, or select a lyric-less note and press Bind.",
                        9.0,
                        theme.editor_warning,
                    );
                }
                let mut input = inspector.spawn((
                    EditorWordInput(selection),
                    EditableText {
                        max_characters: Some(160),
                        visible_width: Some(22.0),
                        ..EditableText::new(text)
                    },
                    Node {
                        width: percent(100),
                        min_height: px(36),
                        padding: UiRect::axes(px(9), px(7)),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(5)),
                        ..default()
                    },
                    ui_text_font(font.clone(), 11.0),
                    TextColor(theme.foreground),
                    TextCursorStyle {
                        color: theme.primary,
                        selected_text_color: Some(theme.primary_foreground),
                        ..default()
                    },
                    BackgroundColor(theme.background.with_alpha(0.4)),
                    BorderColor::all(theme.border.with_alpha(0.6)),
                    TabIndex(0),
                ));
                if editor.word_edit_focus == Some(selection) {
                    input.insert(AutoFocus);
                }
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    format!(
                        "{start:.3}s – {end:.3}s · whole-word and boundary controls use 10 ms steps."
                    ),
                    9.0,
                    theme.muted_foreground,
                );
                for (label, action) in [
                    ("Add word at playhead", UiAction::Editor(EditorAction::AddLyric)),
                    ("Move word −10 ms", UiAction::Editor(EditorAction::ShiftLyricEarlier)),
                    ("Move word +10 ms", UiAction::Editor(EditorAction::ShiftLyricLater)),
                    ("Start −10 ms", UiAction::Editor(EditorAction::LyricStartEarlier)),
                    ("Start +10 ms", UiAction::Editor(EditorAction::LyricStartLater)),
                    ("End −10 ms", UiAction::Editor(EditorAction::LyricEndEarlier)),
                    ("End +10 ms", UiAction::Editor(EditorAction::LyricEndLater)),
                    (
                        "Split into syllables",
                        UiAction::Editor(EditorAction::SyllabizeLyrics),
                    ),
                    ("Split word", UiAction::Editor(EditorAction::SplitLyrics)),
                    ("Merge next word", UiAction::Editor(EditorAction::MergeLyrics)),
                    ("New phrase here", UiAction::Editor(EditorAction::SplitPhrase)),
                    ("Join next phrase", UiAction::Editor(EditorAction::MergePhrase)),
                    ("Delete word", UiAction::Editor(EditorAction::DeleteLyrics)),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
                spawn_phrase_editor(inspector, font.clone(), editor, selection.segment, theme);
            } else {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Select a note or lyric word. Shift-click/drag selects multiple notes.",
                    10.0,
                    theme.muted_foreground,
                );
                spawn_editor_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Add lyric at playhead",
                    EditorAction::AddLyric,
                );
            }

            // Chart checks (errors/warnings) have their own toolbar button
            // and floating panel now — see `spawn_problems_panel` — so a long
            // problem list can't crowd this column out the way it used to.
            inspector.spawn(Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(6)),
                ..default()
            });
            spawn_text(
                inspector,
                font.clone(),
                "GLOBAL TIMING",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                inspector,
                font.clone(),
                "Shift lyrics and pitch together when the whole chart is early or late.",
                9.0,
                theme.muted_foreground,
            );
            spawn_editor_action_button(
                inspector,
                font.clone(),
                theme,
                "Shift all −10 ms",
                EditorAction::ShiftChartEarlier,
            );
            spawn_editor_action_button(
                inspector,
                font.clone(),
                theme,
                "Shift all +10 ms",
                EditorAction::ShiftChartLater,
            );

            let (undoable, redoable) = editor.history();
            inspector.spawn(Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(6)),
                ..default()
            });
            spawn_text(inspector, font.clone(), "HISTORY", 8.0, theme.primary);
            if undoable.is_empty() && redoable.is_empty() {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "No edits yet.",
                    9.0,
                    theme.muted_foreground,
                );
            } else {
                // Newest first, so the next undo is the one at the top.
                for label in undoable.iter().rev().take(EDITOR_HISTORY_ROWS) {
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        format!("· {label}"),
                        9.0,
                        theme.foreground,
                    );
                }
                if undoable.len() > EDITOR_HISTORY_ROWS {
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        format!("+ {} earlier edit(s)", undoable.len() - EDITOR_HISTORY_ROWS),
                        9.0,
                        theme.muted_foreground,
                    );
                }
                if let Some(next) = redoable.last() {
                    spawn_wrapped_text(
                        inspector,
                        font.clone(),
                        format!("Redo: {next}"),
                        9.0,
                        theme.muted_foreground,
                    );
                }
            }
            spawn_editor_action_button(
                inspector,
                font.clone(),
                theme,
                "Undo",
                EditorAction::Undo,
            );
            spawn_action_button(inspector, font, theme, "Redo", UiAction::Editor(EditorAction::Redo));
        });
}

/// Every entry the cheat sheet lists: label, then the shortcut or gesture.
/// Registered single-key actions are listed by hand here rather than pulled
/// from `EDITOR_ACTIONS` at runtime — the registry's shortcut list is a test
/// fixture (`EditorAction::ALL` is `#[cfg(test)]`-only), and most of what
/// makes this sheet worth having (drags, wheel modifiers, held keys) has no
/// registry entry to read from at all.
const EDITOR_SHORTCUTS: &[(&str, &str)] = &[
    ("Space", "Play / pause"),
    ("Home / End", "Jump to song start / end"),
    ("F", "Fit the whole song in view"),
    ("Shift + F", "Fit the selection in view"),
    ("Ctrl + Space", "Play the selection"),
    ("Ctrl + S", "Save"),
    ("Ctrl + Z / Ctrl + Y", "Undo / redo"),
    ("Escape", "Close the inspector"),
    ("Tab / Shift + Tab", "Select next / previous note"),
    ("Ctrl + A", "Select all"),
    ("Delete / Backspace", "Delete the selection"),
    ("S / M / Q", "Split / merge / quantize the selection"),
    ("Ctrl + C / Ctrl + D", "Copy / duplicate the selection"),
    ("Ctrl + T, then T", "Arm and use tap-to-time"),
    ("Shift + drag (canvas)", "Marquee-select notes"),
    ("Shift/Ctrl + click", "Add to the selection"),
    ("Double-click canvas", "Add a note at the pointer"),
    ("Hold B, click note then lyric", "Bind them together"),
    ("Hold C, click a bound note", "Unbind it"),
    ("Right-click note / lyric / waveform", "Open its context menu"),
    ("Ctrl + wheel", "Zoom the timeline"),
    ("Alt + wheel", "Zoom the pitch range"),
    ("Shift + wheel", "Pan the pitch range"),
];

/// Always spawned (so hovering the status-bar hint can reveal it without a
/// UI rebuild), initially hidden. `update_editor_shortcuts_panel_visibility`
/// shows it while the hint or the panel itself is hovered, or while pinned
/// open with H or a click on the hint. A pinned-open panel also gets a
/// click-away backdrop; a hover-only reveal doesn't, so it can't eat clicks
/// meant for whatever is underneath.
pub(crate) fn spawn_shortcuts_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
) {
    if editor.shortcuts_panel_open {
        parent.spawn((
            Button,
            UiAction::DismissShortcutsPanel,
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
    }
    parent
        .spawn((
            EditorShortcutsPanel,
            Interaction::None,
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                bottom: px(34),
                width: px(340),
                max_height: px(440),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(4),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                overflow: Overflow::scroll_y(),
                display: Display::None,
                ..default()
            },
            ScrollPosition::default(),
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|panel| {
            spawn_text(panel, font.clone(), "SHORTCUTS", 9.0, theme.primary);
            for (keys, description) in EDITOR_SHORTCUTS {
                panel
                    .spawn(Node {
                        column_gap: px(8),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Node {
                                width: px(150),
                                flex_shrink: 0.0,
                                ..default()
                            },
                            Text::new(*keys),
                            ui_text_font(font.clone(), 9.0),
                            TextColor(theme.foreground),
                            TextLayout::default(),
                        ));
                        row.spawn((
                            Text::new(*description),
                            ui_text_font(font.clone(), 9.0),
                            TextColor(theme.muted_foreground),
                            TextLayout::default(),
                        ));
                    });
            }
        });
}

/// Keeps `editor.problems_cache` current. Runs every frame, but the check
/// itself is a cheap integer compare — the expensive `problems()` pass only
/// runs again once the document's revision has actually moved.
pub(crate) fn refresh_editor_problems_cache(mut session: ResMut<StudioSession>) {
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    editor.refresh_problems();
}

/// The chart-checks panel: every timing/lyric-coverage problem, filterable by
/// severity and scrollable, so a long list no longer has to squeeze into the
/// fixed-height inspector column the way it used to.
pub(crate) fn spawn_problems_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
) {
    parent.spawn((
        Button,
        UiAction::DismissProblemsPanel,
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
    let report = &editor.problems_cache.1;
    let multi_track = editor.document.track_count() > 1;
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                right: px(16),
                top: px(64),
                width: px(320),
                max_height: px(420),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(10)),
                row_gap: px(6),
                border: UiRect::all(px(1)),
                border_radius: BorderRadius::all(px(8)),
                ..default()
            },
            BackgroundColor(theme.card.with_alpha(0.98)),
            BorderColor::all(theme.border.with_alpha(0.72)),
            ZIndex(41),
        ))
        .with_children(|panel| {
            spawn_text(panel, font.clone(), "CHART CHECKS", 9.0, theme.primary);
            spawn_wrapped_text(
                panel,
                font.clone(),
                if report.total() == 0 {
                    "No timing or lyric coverage issues found.".to_string()
                } else {
                    format!(
                        "{} error(s) · {} warning(s){}",
                        report.errors(),
                        report.warnings(),
                        if report.blocks_saving() {
                            " · saving is blocked"
                        } else {
                            ""
                        }
                    )
                },
                9.0,
                if report.blocks_saving() {
                    theme.destructive
                } else {
                    theme.muted_foreground
                },
            );
            panel
                .spawn(Node {
                    column_gap: px(4),
                    ..default()
                })
                .with_children(|filters| {
                    for filter in [
                        ProblemsFilter::All,
                        ProblemsFilter::Errors,
                        ProblemsFilter::Warnings,
                    ] {
                        spawn_menu_check_row(
                            filters,
                            font.clone(),
                            theme,
                            filter.label(),
                            editor.problems_filter == filter,
                            true,
                            UiAction::SetProblemsFilter(filter),
                        );
                    }
                });
            let matching = report
                .problems
                .iter()
                .filter(|problem| editor.problems_filter.matches(problem.severity()))
                .collect::<Vec<_>>();
            if matching.is_empty() {
                spawn_wrapped_text(
                    panel,
                    font.clone(),
                    "Nothing matches this filter.",
                    9.0,
                    theme.muted_foreground,
                );
            } else {
                panel
                    .spawn((
                        EditorProblemsList,
                        ScrollPosition::default(),
                        Node {
                            min_height: px(0),
                            flex_direction: FlexDirection::Column,
                            row_gap: px(2),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                    ))
                    .with_children(|list| {
                        for problem in matching {
                            let color = if problem.severity() == app_core::Severity::Error {
                                theme.destructive
                            } else {
                                theme.foreground
                            };
                            list.spawn((
                                Button,
                                // Jumping to a problem also switches to the
                                // track it is on, so the note it points at is
                                // the editable one.
                                UiAction::FocusChartProblem(
                                    problem.track,
                                    (problem.time * 1000.0).max(0.0) as u64,
                                ),
                                Node {
                                    width: percent(100),
                                    padding: UiRect::all(px(4)),
                                    border_radius: BorderRadius::all(px(3)),
                                    ..default()
                                },
                                BackgroundColor(Color::NONE),
                                children![(
                                    Text::new(format!(
                                        "{} {}{} · {}",
                                        if problem.severity() == app_core::Severity::Error {
                                            "!"
                                        } else {
                                            "·"
                                        },
                                        format_duration(problem.time),
                                        if multi_track {
                                            format!(" · T{}", problem.track + 1)
                                        } else {
                                            String::new()
                                        },
                                        problem.message
                                    )),
                                    ui_text_font(font.clone(), 9.0),
                                    TextColor(color),
                                    TextLayout::default(),
                                )],
                            ));
                        }
                    });
            }
            if report.auto_fixable() {
                spawn_editor_action_button(
                    panel,
                    font,
                    theme,
                    "Apply safe repairs",
                    EditorAction::RepairChart,
                );
            }
        });
}

/// Scrolls the chart-checks panel's problem list with the mouse wheel, the
/// same way `handle_folder_scroll` drives the folder browser's list.
pub(crate) fn handle_problems_panel_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut lists: Query<(&ComputedNode, &mut ScrollPosition), With<EditorProblemsList>>,
) {
    let open = session
        .editor
        .as_ref()
        .is_some_and(|editor| editor.problems_panel_open);
    if !open {
        return;
    }
    let Ok((computed, mut position)) = lists.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
}

/// Shows the shortcuts panel while the status-bar hint or the panel itself
/// is hovered, or while pinned open (`editor.shortcuts_panel_open`, toggled
/// by the H key or a click on the hint).
pub(crate) fn update_editor_shortcuts_panel_visibility(
    session: Res<StudioSession>,
    triggers: Query<&Interaction, With<EditorShortcutsHoverTrigger>>,
    mut panels: Query<(&Interaction, &mut Node), With<EditorShortcutsPanel>>,
) {
    let pinned = session
        .editor
        .as_ref()
        .is_some_and(|editor| editor.shortcuts_panel_open);
    let trigger_hovered = triggers
        .iter()
        .any(|interaction| *interaction != Interaction::None);
    for (interaction, mut node) in &mut panels {
        let visible = pinned || trigger_hovered || *interaction != Interaction::None;
        node.display = if visible { Display::Flex } else { Display::None };
    }
}

/// Scrolls the shortcuts panel with the mouse wheel.
pub(crate) fn handle_shortcuts_panel_scroll(
    mut wheel: MessageReader<bevy::input::mouse::MouseWheel>,
    session: Res<StudioSession>,
    mut panels: Query<(&ComputedNode, &mut ScrollPosition), With<EditorShortcutsPanel>>,
) {
    let open = session
        .editor
        .as_ref()
        .is_some_and(|editor| editor.shortcuts_panel_open);
    if !open {
        return;
    }
    let Ok((computed, mut position)) = panels.single_mut() else {
        return;
    };
    let mut delta = 0.0;
    for event in wheel.read() {
        let scale = match event.unit {
            bevy::input::mouse::MouseScrollUnit::Line => 22.0,
            bevy::input::mouse::MouseScrollUnit::Pixel => 1.0,
        };
        delta -= event.y * scale;
    }
    if delta.abs() < f32::EPSILON {
        return;
    }
    let size = computed.size() * computed.inverse_scale_factor();
    let content = computed.content_size() * computed.inverse_scale_factor();
    let max = (content.y - size.y).max(0.0);
    position.y = (position.y + delta).clamp(0.0, max);
}

/// The whole-line lyric editor.
///
/// Retyping a line is faster than clicking through its syllables, but the
/// syllable boundaries have to survive the round trip — so the field shows
/// them: a space starts a new word, a slash divides syllables inside one.
fn spawn_phrase_editor(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    editor: &NativeEditor,
    phrase: usize,
    theme: &StudioTheme,
) {
    parent.spawn(Node {
        width: percent(100),
        height: px(1),
        margin: UiRect::vertical(px(6)),
        ..default()
    });
    spawn_text(parent, font.clone(), "WHOLE LINE", 8.0, theme.primary);
    parent.spawn((
        EditorPhraseInput(phrase),
        EditableText {
            max_characters: Some(600),
            // A single visible line (the default) was cutting long lines off
            // instead of showing them — this wraps and grows up to 6 lines,
            // scrolling if a line is somehow longer than that.
            visible_lines: Some(6.0),
            ..EditableText::new(&editor.document.phrase_token_text(phrase))
        },
        Node {
            width: percent(100),
            min_height: px(28),
            max_height: px(130),
            min_width: px(0),
            padding: UiRect::axes(px(8), px(6)),
            border: UiRect::all(px(1)),
            border_radius: BorderRadius::all(px(6)),
            overflow: Overflow::scroll_y(),
            ..default()
        },
        ScrollPosition::default(),
        ui_text_font(font.clone(), 10.0),
        TextColor(theme.foreground),
        TextCursorStyle {
            color: theme.editor_selection,
            selected_text_color: Some(theme.primary_foreground),
            ..default()
        },
        BackgroundColor(theme.background.with_alpha(0.4)),
        BorderColor::all(theme.border.with_alpha(0.6)),
        TabIndex(0),
    ));
    spawn_wrapped_text(
        parent,
        font.clone(),
        "Space starts a word, slash divides syllables inside it. Extra syllables land on the last note.",
        9.0,
        theme.muted_foreground,
    );
    // Alignment that is right except for being one note off is the common
    // failure of automatic transcription, so it gets its own control.
    for (label, action) in [
        ("Roll line earlier", EditorAction::RollLyricsLeft),
        ("Roll line later", EditorAction::RollLyricsRight),
    ] {
        spawn_editor_action_button(parent, font.clone(), theme, label, action);
    }
    spawn_editor_action_button(
        parent,
        font,
        theme,
        "Edit all lyrics",
        EditorAction::EditAllLyrics,
    );
}

/// A whole-song lyrics textarea, one line per phrase in order. Unlike the
/// song-detail lyrics dialog (which re-runs alignment from scratch), this
/// writes each retyped line straight into its existing phrase, so pitch and
/// timing already authored survive it — only the text changes.
pub(crate) fn spawn_all_lyrics_panel(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    editor: &NativeEditor,
) {
    if !editor.all_lyrics_editor_open {
        return;
    }
    let text = (0..editor.document.phrase_count())
        .map(|phrase| editor.document.phrase_token_text(phrase))
        .collect::<Vec<_>>()
        .join("\n");
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
            ZIndex(80),
        ))
        .with_children(|overlay| {
            overlay
                .spawn((
                    Node {
                        width: percent(72),
                        max_width: px(760),
                        height: percent(78),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(22)),
                        row_gap: px(10),
                        border: UiRect::all(px(1)),
                        border_radius: BorderRadius::all(px(8)),
                        ..default()
                    },
                    BackgroundColor(theme.card),
                    BorderColor::all(theme.border),
                ))
                .with_children(|dialog| {
                    spawn_text(dialog, font.clone(), "EDIT ALL LYRICS", 8.0, theme.primary);
                    spawn_wrapped_text(
                        dialog,
                        font.clone(),
                        "One line per phrase, in order — blank lines are fine, but the line count has to match the phrase count to apply. This edits text only; every note's pitch and timing stays exactly as authored.",
                        10.0,
                        theme.muted_foreground,
                    );
                    dialog.spawn((
                        EditorAllLyricsInput,
                        EditableText {
                            max_characters: Some(20_000),
                            visible_lines: Some(100.0),
                            allow_newlines: true,
                            ..EditableText::new(text)
                        },
                        Node {
                            width: percent(100),
                            min_height: px(0),
                            flex_grow: 1.0,
                            padding: UiRect::all(px(10)),
                            border: UiRect::all(px(1)),
                            border_radius: BorderRadius::all(px(6)),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition::default(),
                        ui_text_font(font.clone(), 10.0),
                        TextColor(theme.foreground),
                        TextCursorStyle {
                            color: theme.editor_selection,
                            selected_text_color: Some(theme.primary_foreground),
                            ..default()
                        },
                        BackgroundColor(theme.background.with_alpha(0.4)),
                        BorderColor::all(theme.border.with_alpha(0.6)),
                        TabIndex(0),
                        AutoFocus,
                    ));
                    dialog
                        .spawn(Node {
                            width: percent(100),
                            column_gap: px(8),
                            justify_content: JustifyContent::FlexEnd,
                            ..default()
                        })
                        .with_children(|row| {
                            spawn_text_button(
                                row,
                                font.clone(),
                                theme,
                                "Cancel",
                                10.0,
                                UiAction::Editor(EditorAction::EditAllLyrics),
                            );
                            spawn_text_button(
                                row,
                                font,
                                theme,
                                "Apply",
                                10.0,
                                UiAction::ApplyAllLyricsEdit,
                            );
                        });
                });
        });
}

/// Applies a retyped line back onto the notes it belongs to.
pub(crate) fn sync_editor_phrase_input(
    inputs: Query<(&EditableText, &EditorPhraseInput), Changed<EditableText>>,
    mut session: ResMut<StudioSession>,
) {
    let Some(editor) = session.editor.as_mut() else {
        return;
    };
    for (input, marker) in &inputs {
        let text = input.value().to_string();
        if text == editor.document.phrase_token_text(marker.0) {
            continue;
        }
        editor.checkpoint("Retype line");
        if editor.document.set_phrase_token_text(marker.0, &text) {
            editor.dirty = true;
        } else {
            editor.undo.pop();
        }
    }
}
