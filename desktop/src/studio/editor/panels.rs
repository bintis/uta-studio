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
                    ("Cycle note type", UiAction::CycleEditorNoteKind),
                    ("Split selection", UiAction::SplitEditorNote),
                    ("Merge selection", UiAction::MergeEditorNotes),
                    ("Quantize selection", UiAction::QuantizeEditorNotes),
                    ("Duplicate selection", UiAction::DuplicateEditorNotes),
                    ("Copy selection", UiAction::CopyEditorNote),
                    ("Delete selection", UiAction::DeleteEditorNote),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
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
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Cycle note type",
                    UiAction::CycleEditorNoteKind,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Split at playhead",
                    UiAction::SplitEditorNote,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Quantize note",
                    UiAction::QuantizeEditorNotes,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Duplicate note",
                    UiAction::DuplicateEditorNotes,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Copy note",
                    UiAction::CopyEditorNote,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Delete note",
                    UiAction::DeleteEditorNote,
                );
                if !editor.clipboard_notes.is_empty() {
                    spawn_action_button(
                        inspector,
                        font.clone(),
                        theme,
                        "Paste at playhead",
                        UiAction::PasteEditorNote,
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
                    ("Move selection −10 ms", UiAction::ShiftEditorWord(-1)),
                    ("Move selection +10 ms", UiAction::ShiftEditorWord(1)),
                    ("Split selected words", UiAction::SplitEditorWord),
                    ("Merge selected words", UiAction::MergeEditorWord),
                    ("Delete selected words", UiAction::DeleteEditorWord),
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
                    ("Add word at playhead", UiAction::AddEditorWord),
                    ("Move word −10 ms", UiAction::ShiftEditorWord(-1)),
                    ("Move word +10 ms", UiAction::ShiftEditorWord(1)),
                    ("Start −10 ms", UiAction::AdjustEditorWordStart(-1)),
                    ("Start +10 ms", UiAction::AdjustEditorWordStart(1)),
                    ("End −10 ms", UiAction::AdjustEditorWordEnd(-1)),
                    ("End +10 ms", UiAction::AdjustEditorWordEnd(1)),
                    ("Split word", UiAction::SplitEditorWord),
                    ("Merge next word", UiAction::MergeEditorWord),
                    ("New phrase here", UiAction::SplitEditorPhrase),
                    ("Join next phrase", UiAction::MergeEditorPhrase),
                    ("Delete word", UiAction::DeleteEditorWord),
                ] {
                    spawn_action_button(inspector, font.clone(), theme, label, action);
                }
            } else {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Select a note or lyric word. Shift-click/drag selects multiple notes.",
                    10.0,
                    theme.muted_foreground,
                );
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Add lyric at playhead",
                    UiAction::AddEditorWord,
                );
            }

            let report = editor.document.problems();
            inspector.spawn(Node {
                width: percent(100),
                height: px(1),
                margin: UiRect::vertical(px(6)),
                ..default()
            });
            spawn_text(
                inspector,
                font.clone(),
                "CHART CHECKS",
                8.0,
                theme.primary,
            );
            spawn_wrapped_text(
                inspector,
                font.clone(),
                if report.total() == 0 {
                    "No timing or lyric coverage issues found.".to_string()
                } else {
                    format!(
                        "{} error(s) · {} warning(s)",
                        report.errors(),
                        report.warnings()
                    )
                },
                9.0,
                if report.blocks_saving() {
                    theme.destructive
                } else {
                    theme.muted_foreground
                },
            );
            if report.blocks_saving() {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    "Saving is blocked until the errors are resolved.",
                    9.0,
                    theme.destructive,
                );
            }
            for problem in report.problems.iter().take(EDITOR_PROBLEM_ROWS) {
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    format!(
                        "{} {} · {}",
                        if problem.severity() == app_core::Severity::Error {
                            "!"
                        } else {
                            "·"
                        },
                        format_duration(problem.time),
                        problem.message
                    ),
                    UiAction::FocusChartProblem((problem.time * 1000.0).max(0.0) as u64),
                );
            }
            if report.total() > EDITOR_PROBLEM_ROWS {
                spawn_wrapped_text(
                    inspector,
                    font.clone(),
                    format!("+ {} more", report.total() - EDITOR_PROBLEM_ROWS),
                    9.0,
                    theme.muted_foreground,
                );
            }
            if report.auto_fixable() {
                spawn_action_button(
                    inspector,
                    font.clone(),
                    theme,
                    "Apply safe repairs",
                    UiAction::RepairEditorChart,
                );
            }
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
            spawn_action_button(
                inspector,
                font.clone(),
                theme,
                "Shift all −10 ms",
                UiAction::ShiftWholeChart(-1),
            );
            spawn_action_button(
                inspector,
                font.clone(),
                theme,
                "Shift all +10 ms",
                UiAction::ShiftWholeChart(1),
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
            spawn_action_button(
                inspector,
                font.clone(),
                theme,
                "Undo",
                UiAction::EditorUndo,
            );
            spawn_action_button(inspector, font, theme, "Redo", UiAction::EditorRedo);
        });
}
