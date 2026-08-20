//! Chart edit commands: thin, named wrappers over `EditorDocument`
//! mutations so the action layer never touches the document directly.

use std::collections::BTreeSet;

use crate::studio::widgets::format_duration;

use super::state::{NativeEditor, WordSelection};

pub(crate) fn move_chart_note(
    document: &mut app_core::EditorDocument,
    index: usize,
    start: f64,
    end: f64,
    midi: f64,
) -> bool {
    document.move_note(index, start, end, midi)
}

pub(crate) fn resize_chart_note(
    document: &mut app_core::EditorDocument,
    index: usize,
    start: f64,
    end: f64,
) -> bool {
    document.resize_note(index, start, end)
}

pub(crate) fn insert_chart_note(
    document: &mut app_core::EditorDocument,
    start: f64,
    end: f64,
    midi: f64,
) -> Option<usize> {
    document.insert_note(start, end, midi)
}

pub(crate) fn copy_chart_notes(
    document: &app_core::EditorDocument,
    indices: &BTreeSet<usize>,
) -> Vec<app_core::ClipboardNote> {
    document.copy_notes(indices)
}

pub(crate) fn paste_chart_notes(
    document: &mut app_core::EditorDocument,
    clipboard: &[app_core::ClipboardNote],
    at: f64,
) -> BTreeSet<usize> {
    document.paste_notes(clipboard, at)
}

pub(crate) fn remove_chart_notes(
    document: &mut app_core::EditorDocument,
    indices: &BTreeSet<usize>,
) -> usize {
    document.remove_notes(indices)
}

pub(crate) fn split_chart_notes(
    document: &mut app_core::EditorDocument,
    indices: &BTreeSet<usize>,
    playhead: f64,
) -> BTreeSet<usize> {
    document.split_notes(indices, playhead)
}

pub(crate) fn merge_chart_notes(
    document: &mut app_core::EditorDocument,
    indices: &BTreeSet<usize>,
    primary: Option<usize>,
) -> Option<usize> {
    document.merge_notes(indices, primary)
}

pub(crate) fn quantize_chart_notes(
    document: &mut app_core::EditorDocument,
    indices: Option<&BTreeSet<usize>>,
    grid: f64,
) -> usize {
    document.quantize_notes(indices, grid)
}

pub(crate) fn shift_chart_notes(
    document: &mut app_core::EditorDocument,
    indices: &BTreeSet<usize>,
    seconds: f64,
    semitones: f64,
    resize_end: bool,
) -> usize {
    document.shift_notes(indices, seconds, semitones, resize_end)
}

pub(crate) fn cycle_chart_note_kinds(
    document: &mut app_core::EditorDocument,
    indices: &BTreeSet<usize>,
) -> usize {
    document.cycle_note_kinds(indices)
}

pub(crate) fn update_editor_word_text(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
    text: &str,
) -> bool {
    document.set_lyric_text(selection, text)
}

/// Gives a lyric-less pitch note its own lyric text directly, ready to
/// type over — no separate placeholder note or "Bind" step.
pub(crate) fn add_lyric_to_editor_note(
    document: &mut app_core::EditorDocument,
    note_index: usize,
) -> Option<WordSelection> {
    document.add_lyric_to_note(note_index)
}

pub(crate) fn insert_editor_word(
    document: &mut app_core::EditorDocument,
    selection: Option<WordSelection>,
    playhead: f64,
) -> Option<WordSelection> {
    document.insert_lyric(selection, playhead)
}

pub(crate) fn delete_editor_words(
    document: &mut app_core::EditorDocument,
    selections: &BTreeSet<WordSelection>,
) -> usize {
    document.delete_lyrics(selections)
}

pub(crate) fn merge_selected_editor_words(
    document: &mut app_core::EditorDocument,
    selections: &BTreeSet<WordSelection>,
) -> Option<WordSelection> {
    document.merge_lyrics(selections)
}

pub(crate) fn split_selected_editor_words(
    document: &mut app_core::EditorDocument,
    selections: &BTreeSet<WordSelection>,
    playhead: f64,
) -> BTreeSet<WordSelection> {
    document.split_lyrics(selections, playhead)
}

pub(crate) fn shift_editor_word(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
    delta: f64,
) -> bool {
    document.shift_lyric(selection, delta)
}

pub(crate) fn set_editor_word_timing(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
    start: f64,
    end: f64,
) -> bool {
    document.set_lyric_timing(selection, start, end)
}

pub(crate) fn adjust_editor_word_boundary(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
    start_delta: f64,
    end_delta: f64,
) -> bool {
    document.adjust_lyric_boundary(selection, start_delta, end_delta)
}

pub(crate) fn merge_editor_word(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
) -> bool {
    document.merge_lyric_with_next(selection)
}

pub(crate) fn split_editor_phrase(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
) -> Option<WordSelection> {
    document.split_phrase(selection)
}

pub(crate) fn merge_editor_phrase(
    document: &mut app_core::EditorDocument,
    selection: WordSelection,
) -> Option<WordSelection> {
    document.merge_phrase_with_next(selection)
}

/// Binds an unpitched lyric onto a lyric-less pitch note, wherever the two
/// were authored. Returns the bound note's index.
pub(crate) fn bind_editor_lyric(
    document: &mut app_core::EditorDocument,
    word: WordSelection,
    note_index: usize,
) -> Option<usize> {
    document.bind_lyric_to_note(word, note_index)
}

/// Splits a bound note's pitch and lyric apart. Returns the freed lyric's
/// new address.
pub(crate) fn unbind_editor_note(
    document: &mut app_core::EditorDocument,
    note_index: usize,
) -> Option<WordSelection> {
    document.unbind_note(note_index)
}

/// The note a lyric currently belongs to, for resolving a click on the lyric
/// lane into the note an unbind should act on.
pub(crate) fn editor_note_for_word(
    document: &app_core::EditorDocument,
    selection: WordSelection,
) -> Option<usize> {
    document.note_for_word(selection)
}

/// Whether `note_index` could become a held continuation of the syllable at
/// `word` — the note right-click menu uses this to decide whether to offer
/// it, without mutating anything.
pub(crate) fn can_extend_editor_lyric(
    document: &app_core::EditorDocument,
    word: WordSelection,
    note_index: usize,
) -> bool {
    document.can_extend_lyric_over_note(word, note_index)
}

/// Makes `note_index` a held continuation of the syllable at `word`, for a
/// pitch that glides partway through one sung syllable.
pub(crate) fn extend_editor_lyric(
    document: &mut app_core::EditorDocument,
    word: WordSelection,
    note_index: usize,
) -> bool {
    document.extend_lyric_over_note(word, note_index)
}

/// The note `word` could extend onto next, if any — lets the lyric's own
/// right-click menu offer it directly instead of requiring a separate
/// right-click on that note.
pub(crate) fn next_extendable_editor_note(
    document: &app_core::EditorDocument,
    word: WordSelection,
) -> Option<usize> {
    document.next_extendable_note(word)
}

/// Binds the current selection to its nearest eligible counterpart, for the
/// dock button and the bare `B` shortcut.
pub(crate) fn bind_nearest_editor_selection(
    document: &mut app_core::EditorDocument,
    word: Option<WordSelection>,
    note: Option<usize>,
    align_to_lyric: bool,
) -> Option<usize> {
    document.bind_nearest(word, note, align_to_lyric)
}

/// Unbinds whichever note the current selection names.
pub(crate) fn unbind_editor_selection(
    document: &mut app_core::EditorDocument,
    word: Option<WordSelection>,
    note: Option<usize>,
) -> Option<WordSelection> {
    document.unbind_selected(word, note)
}

/// Saves the chart, or explains the first error standing in the way. The
/// format rejects an invalid chart outright, so the editor names the problem
/// and where it is instead of surfacing a validation message with no location.
pub(crate) fn save_editor_chart(editor: &mut NativeEditor) -> String {
    let report = editor.refresh_problems();
    if let Some(problem) = report
        .problems
        .iter()
        .find(|problem| problem.severity() == app_core::Severity::Error)
    {
        return format!(
            "Cannot save: {} on track {} at {} ({} error(s)). Open the inspector to jump to it.",
            problem.message,
            problem.track + 1,
            format_duration(problem.time),
            report.errors()
        );
    }
    match app_core::save_vocal_chart_from_revision(
        &editor.chart.file_hash,
        editor.document.to_chart(),
        editor.artifact_source.as_ref(),
    ) {
        Ok(()) => {
            editor.dirty = false;
            "Chart saved atomically.".to_string()
        }
        Err(error) => format!("Could not save chart: {error}"),
    }
}

pub(crate) fn repair_editor_chart(document: &mut app_core::EditorDocument) -> bool {
    document.repair();
    true
}

pub(crate) fn shift_all_chart_timings(document: &mut app_core::EditorDocument, seconds: f64) {
    document.shift_all(seconds);
}
