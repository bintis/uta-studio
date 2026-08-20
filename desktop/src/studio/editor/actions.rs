//! Editor commands, driven by one registry.
//!
//! [`EditorAction`] mirrors `app_core::EDITOR_ACTIONS` one variant per entry, so
//! a toolbar button, a key chord, and the label an edit gets in the undo
//! history all resolve to the same command. Adding a command means adding a
//! registry entry and a variant; the tests below fail if the two drift.
//!
//! Selection plumbing that a key chord cannot express — picking a lyric from
//! the lane, opening a dock menu, jumping to a chart problem — stays on
//! [`UiAction`] and is dispatched here alongside the registry actions.

use std::{collections::BTreeSet, time::Instant};

use app_core::{EditorActionDef, editor_action};
use bevy::prelude::{ChildSpawnerCommands, Font, Handle};

use crate::{
    studio::{
        commands::{EditorCommand, UiAction},
        state::{DialogState, EditorUiState, ShellState},
        ui_invalidation::{UiDirtyRegion, UiInvalidated},
        widgets::{format_duration, spawn_action_button},
    },
    theme::StudioTheme,
};

use super::{
    audition::toggle_editor_playback,
    commands::{
        add_lyric_to_editor_note, adjust_editor_word_boundary, bind_nearest_editor_selection,
        copy_chart_notes, cycle_chart_note_kinds, delete_editor_words, insert_chart_note,
        insert_editor_word, merge_chart_notes, merge_editor_phrase, merge_editor_word,
        merge_selected_editor_words, move_chart_note, paste_chart_notes, quantize_chart_notes,
        remove_chart_notes, repair_editor_chart, resize_chart_note, save_editor_chart,
        shift_all_chart_timings, shift_chart_notes, shift_editor_word, split_chart_notes,
        split_editor_phrase, split_selected_editor_words, unbind_editor_selection,
    },
    state::{
        BindAlignment, NativeEditor, TapSession, WordSelection, all_editor_word_selections,
        chart_lyrics, chart_notes, format_snap_grid, selected_editor_word, set_editor_pitch_span,
    },
};

macro_rules! editor_actions {
    ($($variant:ident => $command:literal,)*) => {
        /// One editor command. The variants are the registry, so exhaustive
        /// matching keeps dispatch complete.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub(crate) enum EditorAction {
            $($variant,)*
        }

        impl EditorAction {
            #[cfg(test)]
            pub(crate) const ALL: &'static [Self] = &[$(Self::$variant,)*];

            /// The stable registry id, shared with the API contract.
            pub(crate) fn command(self) -> &'static str {
                match self {
                    $(Self::$variant => $command,)*
                }
            }

            pub(crate) fn from_command(command: &str) -> Option<Self> {
                match command {
                    $($command => Some(Self::$variant),)*
                    _ => None,
                }
            }
        }
    };
}

editor_actions! {
    Save => "save",
    Undo => "undo",
    Redo => "redo",
    RepairChart => "repair_chart",
    ShiftChartEarlier => "shift_chart_earlier",
    ShiftChartLater => "shift_chart_later",
    AddTrack => "add_track",
    RemoveTrack => "remove_track",
    CycleTrackRole => "cycle_track_role",
    ToggleTrackScoring => "toggle_track_scoring",
    SelectNextTrack => "select_next_track",
    MoveSelectionToNextTrack => "move_selection_to_next_track",
    TogglePlayback => "toggle_playback",
    SeekStart => "seek_start",
    SeekEnd => "seek_end",
    AuditionSelection => "audition_selection",
    AuditionVisible => "audition_visible",
    AuditionBeforeSelection => "audition_before_selection",
    AuditionAfterSelection => "audition_after_selection",
    StopAudition => "stop_audition",
    CycleAuditionMode => "cycle_audition_mode",
    ToggleLyrics => "toggle_lyrics",
    ToggleTracks => "toggle_tracks",
    ToggleInspector => "toggle_inspector",
    CloseInspector => "close_inspector",
    ToggleProblemsPanel => "toggle_problems_panel",
    ToggleShortcutsPanel => "toggle_shortcuts_panel",
    ToggleLockMode => "toggle_lock_mode",
    ToggleBeatGrid => "toggle_beat_grid",
    ZoomInTime => "zoom_in_time",
    ZoomOutTime => "zoom_out_time",
    FitSelection => "fit_selection",
    FitSong => "fit_song",
    ZoomInPitch => "zoom_in_pitch",
    ZoomOutPitch => "zoom_out_pitch",
    PanPitchUp => "pan_pitch_up",
    PanPitchDown => "pan_pitch_down",
    SelectAll => "select_all",
    SelectNextNote => "select_next_note",
    SelectPreviousNote => "select_previous_note",
    AddNote => "add_note",
    EditNoteLyric => "edit_note_lyric",
    PlayNotePitch => "play_note_pitch",
    PlayNoteVocal => "play_note_vocal",
    ToggleTapMode => "toggle_tap_mode",
    TapNote => "tap_note",
    DeleteSelection => "delete_selection",
    SplitSelection => "split_selection",
    MergeSelection => "merge_selection",
    QuantizeNotes => "quantize_notes",
    DuplicateNotes => "duplicate_notes",
    CopyNotes => "copy_notes",
    CutNotes => "cut_notes",
    PasteNotes => "paste_notes",
    CycleNoteKind => "cycle_note_kind",
    NudgeEarlier => "nudge_earlier",
    NudgeLater => "nudge_later",
    ShortenSelection => "shorten_selection",
    LengthenSelection => "lengthen_selection",
    RaisePitch => "raise_pitch",
    LowerPitch => "lower_pitch",
    RaisePitchOctave => "raise_pitch_octave",
    LowerPitchOctave => "lower_pitch_octave",
    EditLyricLine => "edit_lyric_line",
    EditAllLyrics => "edit_all_lyrics",
    AddLyric => "add_lyric",
    DeleteLyrics => "delete_lyrics",
    SplitLyrics => "split_lyrics",
    SyllabizeLyrics => "syllabize_lyrics",
    MergeLyrics => "merge_lyrics",
    ShiftLyricEarlier => "shift_lyric_earlier",
    ShiftLyricLater => "shift_lyric_later",
    LyricStartEarlier => "lyric_start_earlier",
    LyricStartLater => "lyric_start_later",
    LyricEndEarlier => "lyric_end_earlier",
    LyricEndLater => "lyric_end_later",
    RollLyricsLeft => "roll_lyrics_left",
    RollLyricsRight => "roll_lyrics_right",
    SplitPhrase => "split_phrase",
    MergePhrase => "merge_phrase",
    BindNearest => "bind_nearest",
    UnbindSelection => "unbind_selection",
    ToggleBindAlignment => "toggle_bind_alignment",
}

impl EditorAction {
    fn def(self) -> &'static EditorActionDef {
        // The registry round trip is proven by `every_action_has_a_registry_entry`.
        editor_action(self.command()).expect("registered editor action")
    }

    /// What the command is called in menus and in the undo history.
    pub(crate) fn label(self) -> &'static str {
        self.def().label
    }

    /// The canonical key chord, for a tooltip or menu hint.
    pub(crate) fn shortcut(self) -> Option<String> {
        self.def().shortcuts.first().map(|chord| chord.describe())
    }
}

/// Spawns a button for a registered command, teaching its key chord where the
/// registry binds one.
pub(crate) fn spawn_editor_action_button(
    parent: &mut ChildSpawnerCommands,
    font: Handle<Font>,
    theme: &StudioTheme,
    label: &str,
    action: EditorAction,
) {
    let label = match action.shortcut() {
        Some(chord) => format!("{label}  ·  {chord}"),
        None => label.to_string(),
    };
    spawn_action_button(
        parent,
        font,
        theme,
        label,
        UiAction::from(EditorCommand::Editor(action)),
    );
}

/// Everything a command needs from the app: the chart under edit, the audio
/// transport it may seek, and the redraw flag.
pub(crate) struct EditorActionContext<'a> {
    pub(crate) audio: &'a uta_studio_audio::EditorAudioPlayer,
    /// The independent tone stream used by pitch audition. It never touches
    /// the song audio.
    pub(crate) tones: &'a uta_studio_audio::PitchAudition,
    pub(crate) shell: &'a mut ShellState,
    pub(crate) editor: &'a mut EditorUiState,
    pub(crate) dialogs: &'a mut DialogState,
    pub(crate) invalidated: &'a mut UiInvalidated,
}

impl EditorActionContext<'_> {
    /// Seeks without changing whether the user was listening, as the editor
    /// interaction rules require.
    pub(crate) fn seek(&mut self, target: f64) {
        let Some(editor) = self.editor.editor.as_mut() else {
            return;
        };
        let was_playing = editor.audio_status.playing;
        match self.audio.seek(target.max(0.0)) {
            Ok(mut status) => {
                if was_playing && let Ok(playing) = self.audio.play() {
                    status = playing;
                }
                editor.visible_position = status.position_secs;
                editor.audio_status = status;
                editor.last_audio_sync = Instant::now();
                self.shell.notice = None;
            }
            Err(error) => self.shell.notice = Some(error),
        }
    }
}

/// Runs one editor command. Every entry point — key chord, toolbar button,
/// inspector button — arrives here.
pub(crate) fn run_editor_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    if let Some(editor) = ctx.editor.editor.as_mut() {
        editor.lyric_context = None;
        editor.note_context = None;
    }
    match action {
        Save | Undo | Redo | RepairChart | ShiftChartEarlier | ShiftChartLater => {
            run_document_action(action, ctx)
        }
        AddTrack
        | RemoveTrack
        | CycleTrackRole
        | ToggleTrackScoring
        | SelectNextTrack
        | MoveSelectionToNextTrack => run_track_action(action, ctx),
        TogglePlayback
        | SeekStart
        | SeekEnd
        | AuditionSelection
        | AuditionVisible
        | AuditionBeforeSelection
        | AuditionAfterSelection
        | StopAudition
        | CycleAuditionMode => run_transport_action(action, ctx),
        ToggleLyrics | ToggleTracks | ToggleInspector | CloseInspector | ToggleProblemsPanel
        | ToggleShortcutsPanel | ToggleLockMode | ToggleBeatGrid | ZoomInTime | ZoomOutTime
        | FitSelection | FitSong | ZoomInPitch | ZoomOutPitch | PanPitchUp | PanPitchDown => {
            run_view_action(action, ctx)
        }
        SelectAll | SelectNextNote | SelectPreviousNote => run_selection_action(action, ctx),
        ToggleTapMode | TapNote => run_tap_action(action, ctx),
        AddNote | EditNoteLyric | PlayNotePitch | PlayNoteVocal | DeleteSelection
        | SplitSelection | MergeSelection | QuantizeNotes | DuplicateNotes | CopyNotes
        | CutNotes | PasteNotes | CycleNoteKind | NudgeEarlier | NudgeLater | ShortenSelection
        | LengthenSelection | RaisePitch | LowerPitch | RaisePitchOctave | LowerPitchOctave => {
            run_note_action(action, ctx)
        }
        EditLyricLine | EditAllLyrics | AddLyric | DeleteLyrics | SplitLyrics | SyllabizeLyrics
        | MergeLyrics | ShiftLyricEarlier | ShiftLyricLater | LyricStartEarlier
        | LyricStartLater | LyricEndEarlier | LyricEndLater | RollLyricsLeft | RollLyricsRight
        | SplitPhrase | MergePhrase => run_lyric_action(action, ctx),
        BindNearest | UnbindSelection | ToggleBindAlignment => run_bind_action(action, ctx),
    }
}

/// Binds or unbinds the current selection to its nearest counterpart. A
/// held-`B`/`C` click (handled in `handle_editor_pointer_capture`) names the
/// counterpart explicitly instead of searching for the nearest one.
fn run_bind_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    match action {
        BindNearest => {
            if editor.selected_word.is_none() && editor.selected_note.is_none() {
                ctx.shell.notice =
                    Some("Select an unpitched lyric or a lyric-less note to bind.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            editor.checkpoint(action.label());
            let align_to_lyric = editor.bind_alignment == BindAlignment::Lyric;
            match bind_nearest_editor_selection(
                &mut editor.document,
                editor.selected_word,
                editor.selected_note,
                align_to_lyric,
            ) {
                Some(bound) => {
                    editor.select_only_note(bound);
                    editor.dirty = true;
                    ctx.shell.notice = Some(format!(
                        "Bound lyric to note, keeping {} timing.",
                        editor.bind_alignment.label()
                    ));
                }
                None => {
                    editor.undo.pop();
                    ctx.shell.notice =
                        Some("No unpitched lyric and lyric-less note nearby to bind.".to_string());
                }
            }
        }
        UnbindSelection => {
            if editor.selected_word.is_none() && editor.selected_note.is_none() {
                ctx.shell.notice = Some("Select a bound note or lyric to unbind.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            editor.checkpoint(action.label());
            match unbind_editor_selection(
                &mut editor.document,
                editor.selected_word,
                editor.selected_note,
            ) {
                Some(freed) => {
                    editor.select_only_word(freed);
                    editor.dirty = true;
                    ctx.shell.notice = Some("Unbound lyric from note.".to_string());
                }
                None => {
                    editor.undo.pop();
                    ctx.shell.notice =
                        Some("This note has no separable pitch and lyric to unbind.".to_string());
                }
            }
        }
        ToggleBindAlignment => {
            editor.bind_alignment = editor.bind_alignment.toggled();
            ctx.shell.notice = Some(format!(
                "Bind will now keep {} timing.",
                editor.bind_alignment.label()
            ));
        }
        _ => unreachable!(),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

// -- chart ----------------------------------------------------------------

fn run_document_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    match action {
        Save => {
            if let Some(editor) = ctx.editor.editor.as_mut() {
                ctx.shell.notice = Some(save_editor_chart(editor));
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        Undo => {
            if let Some(label) = ctx.editor.editor.as_mut().and_then(NativeEditor::undo) {
                ctx.shell.notice = Some(format!("Undid: {label}."));
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        Redo => {
            if let Some(label) = ctx.editor.editor.as_mut().and_then(NativeEditor::redo) {
                ctx.shell.notice = Some(format!("Redid: {label}."));
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        RepairChart => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            editor.checkpoint(action.label());
            let repaired = repair_editor_chart(&mut editor.document);
            if repaired {
                editor.clear_selection();
                editor.dirty = true;
                ctx.shell.notice = Some("Applied safe timing repairs.".to_string());
            } else {
                editor.undo.pop();
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        ShiftChartEarlier | ShiftChartLater => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let earlier = action == ShiftChartEarlier;
            editor.checkpoint(action.label());
            shift_all_chart_timings(&mut editor.document, if earlier { -0.01 } else { 0.01 });
            editor.dirty = true;
            ctx.shell.notice = Some(format!(
                "Shifted the whole chart by {}10 ms.",
                if earlier { "−" } else { "+" }
            ));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        _ => unreachable!("not a chart action"),
    }
}

// -- tracks ---------------------------------------------------------------

fn run_track_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    let active = editor.document.active_track_index();
    let count = editor.document.track_count();
    match action {
        AddTrack => {
            editor.checkpoint(action.label());
            let index = editor.document.add_track(app_core::TrackRole::Lead);
            editor.clear_selection();
            editor.dirty = true;
            ctx.shell.notice = Some(format!(
                "Added track {}. It is saved once it has notes.",
                index + 1
            ));
        }
        RemoveTrack => {
            editor.checkpoint(action.label());
            if editor.document.remove_track(active) {
                editor.clear_selection();
                editor.dirty = true;
                ctx.shell.notice = Some("Removed the active track.".to_string());
            } else {
                editor.undo.pop();
                ctx.shell.notice = Some("A chart needs at least one track.".to_string());
            }
        }
        CycleTrackRole => {
            let Some(role) = editor
                .document
                .tracks()
                .get(active)
                .map(|track| track.role.cycle())
            else {
                return;
            };
            editor.checkpoint(action.label());
            if editor.document.set_track_role(active, role) {
                editor.dirty = true;
                ctx.shell.notice = Some(format!("Track {} is now {}.", active + 1, role.label()));
            } else {
                editor.undo.pop();
            }
        }
        ToggleTrackScoring => {
            let Some(enabled) = editor
                .document
                .tracks()
                .get(active)
                .map(|track| !track.scoring_enabled)
            else {
                return;
            };
            editor.checkpoint(action.label());
            if editor.document.set_track_scoring(active, enabled) {
                editor.dirty = true;
                ctx.shell.notice = Some(if enabled {
                    "This track is scored.".to_string()
                } else {
                    "This track is sung but not scored.".to_string()
                });
            } else {
                editor.undo.pop();
            }
        }
        SelectNextTrack => {
            if count < 2 {
                return;
            }
            let next = (active + 1) % count;
            editor.document.set_active_track(next);
            editor.clear_selection();
            ctx.shell.notice = Some(format!("Editing track {}.", next + 1));
        }
        MoveSelectionToNextTrack => {
            if count < 2 {
                ctx.shell.notice =
                    Some("Add a second track before moving notes to one.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            move_selection_to_track(action, (active + 1) % count, ctx);
            return;
        }
        _ => unreachable!("not a track action"),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

/// Moves the note selection onto another track. This is the path the format
/// recommends for two voices that would otherwise overlap.
pub(crate) fn move_selection_to_track(
    action: EditorAction,
    target: usize,
    ctx: &mut EditorActionContext,
) {
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    let selected = editor.selected_note_indices();
    if selected.is_empty() {
        ctx.shell.notice = Some("Select notes to move to another track.".to_string());
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    }
    editor.checkpoint(action.label());
    let moved = editor.document.move_notes_to_track(&selected, target);
    if moved > 0 {
        editor.clear_selection();
        editor.dirty = true;
        ctx.shell.notice = Some(format!("Moved {moved} note(s) to track {}.", target + 1));
    } else {
        editor.undo.pop();
        ctx.shell.notice = Some("Those notes could not be moved.".to_string());
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

// -- transport ------------------------------------------------------------

fn run_transport_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    match action {
        TogglePlayback => {
            // Plain transport leaves ranged audition behind.
            stop_audition(ctx);
            ctx.shell.notice = toggle_editor_playback(ctx.audio, ctx.editor.editor.as_mut()).err();
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        SeekStart => {
            if ctx.editor.editor.is_some() {
                stop_audition(ctx);
                ctx.seek(0.0);
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        SeekEnd => {
            if let Some(editor) = ctx.editor.editor.as_ref() {
                let end = editor
                    .audio_status
                    .duration_secs
                    .max(editor.waveform.duration_secs);
                stop_audition(ctx);
                ctx.seek(end);
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
            }
        }
        StopAudition => {
            stop_audition(ctx);
            let _ = ctx.audio.pause();
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        CycleAuditionMode => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            editor.audition_mode = editor.audition_mode.cycle();
            let mode = editor.audition_mode;
            ctx.shell.notice = Some(format!("Auditioning {} .", mode.label()));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        AuditionSelection | AuditionVisible | AuditionBeforeSelection | AuditionAfterSelection => {
            run_range_audition(action, ctx)
        }
        _ => unreachable!("not a transport action"),
    }
}

/// How much room a lead-in or lead-out audition gives around the selection.
const AUDITION_APPROACH_SECONDS: f64 = 2.5;

/// The stretch of timeline an audition action covers.
pub(crate) fn audition_range(action: EditorAction, editor: &NativeEditor) -> Option<(f64, f64)> {
    use EditorAction::*;
    if action == AuditionVisible {
        return Some((editor.viewport_start, editor.viewport_end()));
    }
    let selected = editor.selected_note_indices();
    let notes = chart_notes(&editor.document);
    let span = selected.iter().filter_map(|index| notes.get(*index)).fold(
        None::<(f64, f64)>,
        |span, note| {
            Some(match span {
                Some((start, end)) => (start.min(note.start), end.max(note.end)),
                None => (note.start, note.end),
            })
        },
    )?;
    Some(match action {
        AuditionSelection => span,
        // Checking a transition means hearing the run-up, then stopping where
        // the selection begins, and the reverse on the way out.
        AuditionBeforeSelection => ((span.0 - AUDITION_APPROACH_SECONDS).max(0.0), span.0),
        _ => (span.1, span.1 + AUDITION_APPROACH_SECONDS),
    })
}

fn run_range_audition(action: EditorAction, ctx: &mut EditorActionContext) {
    let Some(editor) = ctx.editor.editor.as_ref() else {
        return;
    };
    let Some((start, end)) = audition_range(action, editor) else {
        ctx.shell.notice = Some("Select notes to audition them.".to_string());
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    };
    if end <= start {
        ctx.shell.notice = Some("That range is empty.".to_string());
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    }
    let mode = editor.audition_mode;
    let tones = if mode.plays_tones() {
        pitch_tones(&editor.document, start, end)
    } else {
        Default::default()
    };
    let mut notice = None;

    ctx.tones.stop();
    if mode.plays_tones()
        && let Err(error) = ctx.tones.start(&tones, end - start, 0.9)
    {
        notice = Some(format!("Pitch audition is unavailable: {error}"));
    }
    if mode.plays_song() {
        ctx.seek(start);
        if let Err(error) = ctx.audio.play() {
            notice = Some(error);
        }
    } else {
        // Pitch-only audition keeps the recording silent but still moves the
        // playhead so the timeline follows what is sounding.
        let _ = ctx.audio.pause();
        ctx.seek(start);
    }
    if let Some(editor) = ctx.editor.editor.as_mut() {
        editor.audition_until = Some(end);
        editor.hold_manual_scroll();
    }
    ctx.shell.notice = notice.or_else(|| {
        Some(format!(
            "Auditioning {} from {} to {} ({} mode).",
            if action == EditorAction::AuditionVisible {
                "the visible range"
            } else {
                "the selection"
            },
            format_duration(start),
            format_duration(end),
            mode.label()
        ))
    });
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

/// Note targets in a range, positioned relative to its start.
pub(crate) fn pitch_tones(
    document: &app_core::EditorDocument,
    start: f64,
    end: f64,
) -> Vec<uta_studio_audio::PitchTone> {
    document
        .notes()
        .into_iter()
        // A note with no pitch target has nothing to sound.
        .filter(|note| note.pitched && note.end > start && note.start < end)
        .map(|note| uta_studio_audio::PitchTone {
            start_secs: note.start.max(start) - start,
            duration_secs: note.end.min(end) - note.start.max(start),
            midi: note.midi,
        })
        .collect()
}

pub(crate) fn stop_audition(ctx: &mut EditorActionContext) {
    ctx.tones.stop();
    if let Some(editor) = ctx.editor.editor.as_mut() {
        editor.audition_until = None;
    }
    restore_audition_source(ctx);
}

/// Switches playback back to whatever source was active before `PlayNoteVocal`
/// temporarily loaded the vocal stem, if it did. A no-op the rest of the time.
fn restore_audition_source(ctx: &mut EditorActionContext) {
    let Some(editor) = ctx.editor.editor.as_ref() else {
        return;
    };
    let Some(source) = editor.audition_restore_source.clone() else {
        return;
    };
    let file_hash = editor.chart.file_hash.clone();
    if let Ok(status) = ctx.audio.load(&file_hash, &source)
        && let Some(editor) = ctx.editor.editor.as_mut()
    {
        editor.audio_source = source;
        editor.audio_status = status;
        editor.last_audio_sync = Instant::now();
        editor.audition_restore_source = None;
    }
}

// -- view -----------------------------------------------------------------

fn run_view_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    if action == ToggleTracks {
        ctx.editor.editor_tracks_open = !ctx.editor.editor_tracks_open;
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    }
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    match action {
        ToggleLyrics => editor.lyrics_hidden = !editor.lyrics_hidden,
        ToggleInspector => editor.inspector_open = !editor.inspector_open,
        CloseInspector => {
            if !editor.inspector_open {
                return;
            }
            editor.inspector_open = false;
        }
        ToggleProblemsPanel => editor.problems_panel_open = !editor.problems_panel_open,
        ToggleShortcutsPanel => editor.shortcuts_panel_open = !editor.shortcuts_panel_open,
        ToggleLockMode => {
            editor.lock_mode = !editor.lock_mode;
            ctx.shell.notice = Some(if editor.lock_mode {
                "Locked: notes and lyrics can no longer be dragged. Arrow keys still nudge them."
                    .to_string()
            } else {
                "Unlocked.".to_string()
            });
        }
        ToggleBeatGrid => {
            if editor.beats.is_empty() {
                ctx.shell.notice = Some(
                    "No beat data for this song yet — re-analyze it (Essentia must be installed) to generate a beat grid."
                        .to_string(),
                );
            } else {
                editor.beat_grid_visible = !editor.beat_grid_visible;
            }
        }
        ZoomInTime | ZoomOutTime => {
            let center = editor.viewport_start + editor.viewport_duration / 2.0;
            let factor = if action == ZoomInTime { 0.8 } else { 1.25 };
            editor.viewport_duration = (editor.viewport_duration * factor).clamp(2.0, 180.0);
            editor.viewport_start = (center - editor.viewport_duration / 2.0).max(0.0);
            editor.hold_manual_scroll();
        }
        FitSelection => {
            let notes = chart_notes(&editor.document);
            let mut span: Option<(f64, f64)> = None;
            for index in editor.selected_note_indices() {
                if let Some(note) = notes.iter().find(|note| note.index == index) {
                    span = Some(match span {
                        Some((start, end)) => (start.min(note.start), end.max(note.end)),
                        None => (note.start, note.end),
                    });
                }
            }
            for selection in editor.selected_word_indices() {
                if let Some((_, start, end)) = selected_editor_word(&editor.document, selection) {
                    span = Some(match span {
                        Some((current_start, current_end)) => {
                            (current_start.min(start), current_end.max(end))
                        }
                        None => (start, end),
                    });
                }
            }
            let Some((start, end)) = span else {
                ctx.shell.notice = Some("Select notes or lyrics to fit them in view.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            };
            let padding = ((end - start) * 0.15).max(0.5);
            editor.viewport_start = (start - padding).max(0.0);
            editor.viewport_duration = ((end - start) + padding * 2.0).clamp(2.0, 180.0);
            editor.hold_manual_scroll();
        }
        FitSong => {
            let duration = editor
                .audio_status
                .duration_secs
                .max(editor.waveform.duration_secs)
                .max(2.0);
            editor.viewport_start = 0.0;
            editor.viewport_duration = duration.clamp(2.0, 180.0);
            editor.hold_manual_scroll();
        }
        ZoomInPitch | ZoomOutPitch => {
            let factor = if action == ZoomInPitch { 0.8 } else { 1.25 };
            let span = (editor.pitch_max - editor.pitch_min) * factor;
            set_editor_pitch_span(editor, span);
            editor.hold_manual_scroll();
        }
        PanPitchUp | PanPitchDown => {
            let span = editor.pitch_max - editor.pitch_min;
            let offset = if action == PanPitchUp { 4.0 } else { -4.0 };
            editor.pitch_min = (editor.pitch_min + offset).clamp(0.0, 127.0 - span);
            editor.pitch_max = editor.pitch_min + span;
            editor.hold_manual_scroll();
        }
        _ => unreachable!("not a view action"),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

// -- selection ------------------------------------------------------------

fn run_selection_action(action: EditorAction, ctx: &mut EditorActionContext) {
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    match action {
        EditorAction::SelectAll => {
            // Selecting all follows whichever lane the user is working in.
            if editor.selected_word.is_some() {
                let words = all_editor_word_selections(&editor.document);
                let count = words.len();
                editor.selected_word = words.iter().next().copied();
                editor.selected_words = words;
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.word_edit_focus = None;
                ctx.shell.notice = Some(format!("Selected {count} lyric word(s)."));
            } else {
                let count = editor.document.note_count();
                editor.selected_notes = (0..count).collect();
                editor.selected_note = (count > 0).then_some(0);
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                ctx.shell.notice = Some(format!("Selected {count} note(s)."));
            }
        }
        EditorAction::SelectNextNote | EditorAction::SelectPreviousNote => {
            let count = editor.document.note_count();
            if count == 0 {
                return;
            }
            let backwards = action == EditorAction::SelectPreviousNote;
            let next = editor.selected_note.map_or(0, |index| {
                if backwards {
                    (index + count - 1) % count
                } else {
                    (index + 1) % count
                }
            });
            editor.select_only_note(next);
        }
        _ => unreachable!("not a selection action"),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

// -- notes ----------------------------------------------------------------

/// The note selection, or `None` when no chart is open.
fn selected_notes(ctx: &EditorActionContext) -> Option<BTreeSet<usize>> {
    ctx.editor
        .editor
        .as_ref()
        .map(NativeEditor::selected_note_indices)
}

fn run_note_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    match action {
        AddNote => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            // Arms the tool instead of inserting immediately: the next
            // press-and-drag on the canvas (handled in
            // `handle_editor_pointer_capture`) places the note where the
            // pointer goes down and sizes it to the drag.
            editor.note_insert_armed = true;
            ctx.shell.notice = Some("Click and drag on the canvas to place a note.".to_string());
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        EditNoteLyric => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let Some(note_index) = editor.selected_note else {
                ctx.shell.notice = Some("Select a note to give it a lyric.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            };
            editor.checkpoint(action.label());
            match add_lyric_to_editor_note(&mut editor.document, note_index) {
                Some(word) => {
                    editor.select_only_word(word);
                    editor.word_edit_focus = Some(word);
                    editor.dirty = true;
                    ctx.shell.notice = Some("Type the syllable, then press Enter.".to_string());
                }
                None => {
                    editor.undo.pop();
                    ctx.shell.notice = Some(
                        "This note already has a lyric — edit it in the lyric lane.".to_string(),
                    );
                }
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        PlayNotePitch => {
            let Some(editor) = ctx.editor.editor.as_ref() else {
                return;
            };
            let Some((start, end)) = audition_range(EditorAction::AuditionSelection, editor) else {
                ctx.shell.notice = Some("Select a note to play its pitch.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            };
            let tones = pitch_tones(&editor.document, start, end);
            ctx.tones.stop();
            let notice = match ctx.tones.start(&tones, (end - start).max(0.05), 0.9) {
                Ok(()) => "Playing pitch.".to_string(),
                Err(error) => format!("Pitch audition is unavailable: {error}"),
            };
            ctx.shell.notice = Some(notice);
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        PlayNoteVocal => {
            let Some(editor) = ctx.editor.editor.as_ref() else {
                return;
            };
            let Some((start, end)) = audition_range(EditorAction::AuditionSelection, editor) else {
                ctx.shell.notice = Some("Select a note to play its vocal.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            };
            if editor.chart.audio.vocals.is_none() {
                ctx.shell.notice = Some("This chart has no separate vocal source.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            let file_hash = editor.chart.file_hash.clone();
            let previous_source = editor.audio_source.clone();
            let restore = (previous_source != "vocals").then_some(previous_source);
            ctx.tones.stop();
            let mut notice = None;
            if restore.is_some()
                && let Err(error) = ctx.audio.load(&file_hash, "vocals")
            {
                notice = Some(error);
            }
            if notice.is_none() {
                ctx.seek(start);
                if let Err(error) = ctx.audio.play() {
                    notice = Some(error);
                }
            }
            if notice.is_none()
                && let Some(editor) = ctx.editor.editor.as_mut()
            {
                editor.audio_source = "vocals".to_string();
                editor.audition_until = Some(end);
                editor.audition_restore_source = restore;
                editor.hold_manual_scroll();
            }
            ctx.shell.notice = notice.or_else(|| Some("Playing the vocal.".to_string()));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        DeleteSelection => {
            let Some(selected) = selected_notes(ctx) else {
                return;
            };
            if selected.is_empty() {
                // With no note selected the same key clears the lyric selection.
                run_lyric_action(DeleteLyrics, ctx);
                return;
            }
            let editor = ctx.editor.editor.as_mut().expect("editor");
            editor.checkpoint(action.label());
            let removed = remove_chart_notes(&mut editor.document, &selected);
            if removed > 0 {
                editor.selected_note = None;
                editor.selected_notes.clear();
                editor.dirty = true;
                ctx.shell.notice = Some(format!("Deleted {removed} note(s)."));
            } else {
                editor.undo.pop();
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        SplitSelection => {
            let Some(selected) = selected_notes(ctx) else {
                return;
            };
            if selected.is_empty() {
                run_lyric_action(SplitLyrics, ctx);
                return;
            }
            let editor = ctx.editor.editor.as_mut().expect("editor");
            editor.checkpoint(action.label());
            let next = split_chart_notes(&mut editor.document, &selected, editor.visible_position);
            if next.is_empty() {
                editor.undo.pop();
                ctx.shell.notice = Some(
                    "Move the playhead inside the selected note before splitting.".to_string(),
                );
            } else {
                editor.selected_note = next.iter().next().copied();
                editor.selected_notes = next;
                editor.dirty = true;
                ctx.shell.notice = Some("Split selected note(s).".to_string());
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        MergeSelection => {
            let Some(selected) = selected_notes(ctx) else {
                return;
            };
            if selected.len() < 2 {
                let editor = ctx.editor.editor.as_ref().expect("editor");
                if editor.selected_word_indices().is_empty() {
                    ctx.shell.notice = Some("Select at least two notes to merge.".to_string());
                    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                    return;
                }
                run_lyric_action(MergeLyrics, ctx);
                return;
            }
            let editor = ctx.editor.editor.as_mut().expect("editor");
            editor.checkpoint(action.label());
            if let Some(index) =
                merge_chart_notes(&mut editor.document, &selected, editor.selected_note)
            {
                editor.select_only_note(index);
                editor.dirty = true;
                ctx.shell.notice = Some("Merged selected notes.".to_string());
            } else {
                editor.undo.pop();
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        QuantizeNotes => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            if selected.is_empty() || editor.snap_seconds <= 0.0 {
                ctx.shell.notice = Some("Select notes and enable a timing grid first.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            editor.checkpoint(action.label());
            let changed =
                quantize_chart_notes(&mut editor.document, Some(&selected), editor.snap_seconds);
            editor.dirty |= changed > 0;
            ctx.shell.notice = Some(format!("Quantized {changed} note(s)."));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        DuplicateNotes => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            let clipboard = copy_chart_notes(&editor.document, &selected);
            if clipboard.is_empty() {
                return;
            }
            // Drop the copy clear of the material it came from.
            let notes = chart_notes(&editor.document);
            let selected_end = selected
                .iter()
                .filter_map(|index| notes.get(*index).map(|note| note.end))
                .reduce(f64::max)
                .unwrap_or(editor.visible_position);
            editor.checkpoint(action.label());
            let inserted = paste_chart_notes(
                &mut editor.document,
                &clipboard,
                selected_end + editor.snap_seconds.max(0.02),
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            ctx.shell.notice = Some("Duplicated selected note(s).".to_string());
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        CopyNotes => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            editor.clipboard_notes = copy_chart_notes(&editor.document, &selected);
            let copied = editor.clipboard_notes.len();
            ctx.shell.notice = Some(format!("Copied {copied} note(s)."));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        CutNotes => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                return;
            }
            editor.clipboard_notes = copy_chart_notes(&editor.document, &selected);
            editor.checkpoint(action.label());
            let removed = remove_chart_notes(&mut editor.document, &selected);
            editor.selected_note = None;
            editor.selected_notes.clear();
            editor.dirty |= removed > 0;
            ctx.shell.notice = Some(format!("Cut {removed} note(s)."));
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        PasteNotes => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            if editor.clipboard_notes.is_empty() {
                return;
            }
            editor.checkpoint(action.label());
            let inserted = paste_chart_notes(
                &mut editor.document,
                &editor.clipboard_notes,
                editor.visible_position,
            );
            editor.selected_note = inserted.iter().next().copied();
            editor.selected_notes = inserted;
            editor.dirty = true;
            ctx.shell.notice = Some("Pasted note(s) at the playhead.".to_string());
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        CycleNoteKind => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                return;
            }
            editor.checkpoint(action.label());
            let changed = cycle_chart_note_kinds(&mut editor.document, &selected);
            if changed > 0 {
                editor.dirty = true;
                ctx.shell.notice = Some("Changed selected note type(s).".to_string());
            } else {
                editor.undo.pop();
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        NudgeEarlier | NudgeLater | ShortenSelection | LengthenSelection => {
            run_nudge_action(action, ctx)
        }
        RaisePitch | LowerPitch | RaisePitchOctave | LowerPitchOctave => {
            let Some(editor) = ctx.editor.editor.as_mut() else {
                return;
            };
            let selected = editor.selected_note_indices();
            if selected.is_empty() {
                return;
            }
            let semitones = match action {
                RaisePitch => 1.0,
                LowerPitch => -1.0,
                RaisePitchOctave => 12.0,
                _ => -12.0,
            };
            editor.checkpoint(action.label());
            shift_chart_notes(&mut editor.document, &selected, 0.0, semitones, false);
            editor.dirty = true;
            ctx.shell.notice = None;
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        _ => unreachable!("not a note action"),
    }
}

// -- tap to time ----------------------------------------------------------

fn run_tap_action(action: EditorAction, ctx: &mut EditorActionContext) {
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    match action {
        EditorAction::ToggleTapMode => {
            editor.tap_mode = !editor.tap_mode;
            if editor.tap_mode {
                // Entering with a selection means "re-perform these"; entering
                // with none means "lay down new ones".
                editor.tap.retiming = editor.selected_note_indices().into_iter().collect();
                editor.tap.retimed = 0;
                editor.tap.holding = None;
                let queued = editor.tap.remaining();
                ctx.shell.notice = Some(if queued > 0 {
                    format!(
                        "Tap-to-time on. Hold {} while playing to re-time {queued} selected note(s).",
                        tap_key_hint()
                    )
                } else {
                    format!(
                        "Tap-to-time on. Hold {} while playing to lay down notes.",
                        tap_key_hint()
                    )
                });
            } else {
                finish_tap(editor);
                editor.tap = TapSession::default();
                ctx.shell.notice = Some("Tap-to-time off.".to_string());
            }
        }
        EditorAction::TapNote => {
            if !editor.tap_mode {
                return;
            }
            // A tap that arrives while one is still held closes it first, so a
            // stuck key never swallows the next syllable.
            finish_tap(editor);
            let at = editor.visible_position.max(0.0);
            match editor.tap.next_retarget() {
                Some(index) => {
                    let Some(note) = chart_notes(&editor.document).get(index).cloned() else {
                        editor.tap.retimed += 1;
                        return;
                    };
                    editor.checkpoint(action.label());
                    let length = (note.end - note.start).max(app_core::MIN_NOTE_SECONDS);
                    move_chart_note(&mut editor.document, index, at, at + length, note.midi);
                    editor.select_only_note(index);
                    editor.tap.holding = Some((index, at));
                    editor.dirty = true;
                }
                None => {
                    // A new tap inherits the kind of the note before it, which
                    // is what keeps a tapped rap line scoring on rhythm.
                    let kind = editor
                        .selected_note
                        .and_then(|index| chart_notes(&editor.document).get(index).map(|n| n.kind));
                    let midi = editor
                        .selected_note
                        .and_then(|index| chart_notes(&editor.document).get(index).map(|n| n.midi))
                        .unwrap_or(((editor.pitch_min + editor.pitch_max) / 2.0).round());
                    editor.checkpoint(action.label());
                    let Some(index) = insert_chart_note(
                        &mut editor.document,
                        at,
                        at + app_core::MIN_NOTE_SECONDS,
                        midi.clamp(0.0, 127.0),
                    ) else {
                        editor.undo.pop();
                        return;
                    };
                    if let Some(kind) = kind {
                        editor
                            .document
                            .set_note_kind(&BTreeSet::from([index]), kind);
                    }
                    editor.select_only_note(index);
                    editor.tap.holding = Some((index, at));
                    editor.dirty = true;
                }
            }
        }
        _ => unreachable!("not a tap action"),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

/// Closes the note under the finger at the current playhead.
pub(crate) fn finish_tap(editor: &mut NativeEditor) -> bool {
    let Some((index, start)) = editor.tap.holding.take() else {
        return false;
    };
    let end = editor
        .visible_position
        .max(start + app_core::MIN_NOTE_SECONDS);
    resize_chart_note(&mut editor.document, index, start, end);
    if editor.tap.next_retarget() == Some(index) {
        editor.tap.retimed += 1;
    }
    editor.dirty = true;
    true
}

/// How the tap key reads in a hint, taken from the registry rather than
/// spelled out a second time.
fn tap_key_hint() -> String {
    EditorAction::TapNote
        .shortcut()
        .unwrap_or_else(|| "the tap key".to_string())
}

/// The arrow keys mean "move what is selected"; with nothing selected they move
/// the playhead instead, which is what a plain arrow press should do.
fn run_nudge_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    let earlier = matches!(action, NudgeEarlier | ShortenSelection);
    let resize = matches!(action, ShortenSelection | LengthenSelection);
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    let step = if editor.snap_seconds > 0.0 {
        editor.snap_seconds
    } else {
        0.01
    };
    let seconds = if earlier { -step } else { step };
    let selected = editor.selected_note_indices();
    if !selected.is_empty() {
        editor.checkpoint(action.label());
        shift_chart_notes(&mut editor.document, &selected, seconds, 0.0, resize);
        editor.dirty = true;
        ctx.shell.notice = None;
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    }
    let words = editor.selected_word_indices();
    if !words.is_empty() {
        editor.checkpoint(action.label());
        let moved = words
            .iter()
            .filter(|selection| shift_editor_word(&mut editor.document, **selection, seconds))
            .count();
        if moved > 0 {
            editor.dirty = true;
            ctx.shell.notice = Some(format!(
                "Moved {moved} lyric word(s) {} by {}.",
                if earlier { "earlier" } else { "later" },
                format_snap_grid(step)
            ));
        } else {
            editor.undo.pop();
        }
        ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        return;
    }
    let target = editor.visible_position + if earlier { -2.0 } else { 2.0 };
    ctx.seek(target);
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}

// -- lyrics ---------------------------------------------------------------

fn run_lyric_action(action: EditorAction, ctx: &mut EditorActionContext) {
    use EditorAction::*;
    let Some(editor) = ctx.editor.editor.as_mut() else {
        return;
    };
    match action {
        EditLyricLine => {
            let lyrics = chart_lyrics(&editor.document);
            let selection = lyrics
                .iter()
                .find(|lyric| {
                    editor.visible_position >= lyric.start && editor.visible_position < lyric.end
                })
                .or_else(|| {
                    lyrics
                        .iter()
                        .find(|lyric| lyric.start >= editor.visible_position)
                })
                .or_else(|| lyrics.last())
                .map(|lyric| WordSelection {
                    segment: lyric.segment,
                    word: lyric.word,
                });
            if let Some(selection) = selection {
                editor.select_only_word(selection);
                editor.inspector_open = true;
            } else {
                ctx.shell.notice = Some("This chart has no lyrics yet.".to_string());
            }
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        EditAllLyrics => {
            editor.all_lyrics_editor_open = !editor.all_lyrics_editor_open;
            ctx.invalidated.invalidate(UiDirtyRegion::Editor);
        }
        AddLyric => {
            editor.checkpoint(action.label());
            if let Some(selection) = insert_editor_word(
                &mut editor.document,
                editor.selected_word,
                editor.visible_position,
            ) {
                editor.select_only_word(selection);
                editor.word_edit_focus = Some(selection);
                editor.inspector_open = true;
                editor.dirty = true;
                ctx.shell.notice = Some(
                    "Added a lyric word at the playhead. Type in the inspector to replace its text."
                        .to_string(),
                );
            } else {
                editor.undo.pop();
                ctx.shell.notice = Some("Could not add a lyric word here.".to_string());
            }
        }
        DeleteLyrics => {
            let words = editor.selected_word_indices();
            if words.is_empty() {
                ctx.shell.notice = Some("Select lyric words to delete.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            editor.checkpoint(action.label());
            let deleted = delete_editor_words(&mut editor.document, &words);
            if deleted > 0 {
                editor.selected_word = None;
                editor.selected_words.clear();
                editor.word_edit_focus = None;
                editor.dirty = true;
                ctx.shell.notice = Some(format!("Deleted {deleted} lyric word(s)."));
            } else {
                editor.undo.pop();
                ctx.shell.notice = Some("Could not delete the lyric selection.".to_string());
            }
        }
        SyllabizeLyrics => {
            let words = editor.selected_word_indices();
            if words.is_empty() {
                ctx.shell.notice = Some("Select lyric words to split into syllables.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            }
            editor.checkpoint(action.label());
            let produced = editor.document.syllabize_lyrics(&words);
            if produced.is_empty() {
                editor.undo.pop();
                ctx.shell.notice = Some(
                    "Those words are already one syllable, or their notes are too short to divide."
                        .to_string(),
                );
            } else {
                editor.selected_word = produced.first().copied();
                editor.selected_words = produced.iter().copied().collect();
                editor.word_edit_focus = None;
                editor.dirty = true;
                ctx.shell.notice = Some(format!("Split into {} syllable(s).", produced.len()));
            }
        }
        SplitLyrics => {
            let words = editor.selected_word_indices();
            if words.is_empty() {
                return;
            }
            editor.checkpoint(action.label());
            let next =
                split_selected_editor_words(&mut editor.document, &words, editor.visible_position);
            if next.is_empty() {
                editor.undo.pop();
                ctx.shell.notice =
                    Some("The selected lyric words are too short to split.".to_string());
            } else {
                editor.selected_word = next.iter().next().copied();
                editor.selected_words = next;
                editor.word_edit_focus = None;
                editor.dirty = true;
                ctx.shell.notice = Some("Split selected lyric word(s).".to_string());
            }
        }
        MergeLyrics => {
            let words = editor.selected_word_indices();
            if words.is_empty() {
                return;
            }
            editor.checkpoint(action.label());
            let merged = if words.len() == 1 {
                words
                    .first()
                    .copied()
                    .filter(|selection| merge_editor_word(&mut editor.document, *selection))
            } else {
                merge_selected_editor_words(&mut editor.document, &words)
            };
            if let Some(selection) = merged {
                editor.select_only_word(selection);
                editor.dirty = true;
                ctx.shell.notice = Some("Merged selected lyric words.".to_string());
            } else {
                editor.undo.pop();
                ctx.shell.notice =
                    Some("Select at least two words from the same phrase to merge.".to_string());
            }
        }
        ShiftLyricEarlier | ShiftLyricLater => {
            let words = editor.selected_word_indices();
            if words.is_empty() {
                return;
            }
            let earlier = action == ShiftLyricEarlier;
            editor.checkpoint(action.label());
            let moved = words
                .iter()
                .filter(|selection| {
                    shift_editor_word(
                        &mut editor.document,
                        **selection,
                        if earlier { -0.01 } else { 0.01 },
                    )
                })
                .count();
            if moved > 0 {
                editor.dirty = true;
                ctx.shell.notice = Some(format!(
                    "Moved {moved} lyric word(s) {} 10 ms.",
                    if earlier { "earlier" } else { "later" }
                ));
            } else {
                editor.undo.pop();
            }
        }
        LyricStartEarlier | LyricStartLater | LyricEndEarlier | LyricEndLater => {
            let Some(selection) = editor.selected_word else {
                return;
            };
            let (start, end) = match action {
                LyricStartEarlier => (-0.01, 0.0),
                LyricStartLater => (0.01, 0.0),
                LyricEndEarlier => (0.0, -0.01),
                _ => (0.0, 0.01),
            };
            editor.checkpoint(action.label());
            if adjust_editor_word_boundary(&mut editor.document, selection, start, end) {
                editor.dirty = true;
            } else {
                editor.undo.pop();
            }
        }
        RollLyricsLeft | RollLyricsRight => {
            let Some(selection) = editor.selected_word else {
                ctx.shell.notice = Some("Select a lyric word in the line to roll it.".to_string());
                ctx.invalidated.invalidate(UiDirtyRegion::Editor);
                return;
            };
            editor.checkpoint(action.label());
            if editor
                .document
                .roll_lyrics(selection.segment, action == RollLyricsRight)
            {
                editor.dirty = true;
                ctx.shell.notice = Some(format!(
                    "Rolled the line one note {}.",
                    if action == RollLyricsRight {
                        "later"
                    } else {
                        "earlier"
                    }
                ));
            } else {
                editor.undo.pop();
                ctx.shell.notice =
                    Some("This line has nothing to roll onto another note.".to_string());
            }
        }
        SplitPhrase => {
            let Some(selection) = editor.selected_word else {
                return;
            };
            editor.checkpoint(action.label());
            if let Some(next) = split_editor_phrase(&mut editor.document, selection) {
                editor.select_only_word(next);
                editor.dirty = true;
                ctx.shell.notice = Some("Started a new lyric phrase.".to_string());
            } else {
                editor.undo.pop();
                ctx.shell.notice = Some("Select a word before the end of its phrase.".to_string());
            }
        }
        MergePhrase => {
            let Some(selection) = editor.selected_word else {
                return;
            };
            editor.checkpoint(action.label());
            if let Some(next) = merge_editor_phrase(&mut editor.document, selection) {
                editor.select_only_word(next);
                editor.dirty = true;
                ctx.shell.notice = Some("Joined the following lyric phrase.".to_string());
            } else {
                editor.undo.pop();
                ctx.shell.notice = Some("There is no following phrase to join.".to_string());
            }
        }
        _ => unreachable!("not a lyric action"),
    }
    ctx.invalidated.invalidate(UiDirtyRegion::Editor);
}
