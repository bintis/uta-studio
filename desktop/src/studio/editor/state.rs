//! Editor state: the `NativeEditor` resource, viewport, selection,
//! history, and the read-only chart views the timeline renders.

use std::{
    collections::BTreeSet,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

use bevy::{
    color::Alpha,
    prelude::{Color, Component, Resource, Timer, Vec2},
};

use crate::{studio::widgets::format_duration, theme::StudioTheme};

pub(crate) const EDITOR_TRACK_GUTTER_WIDTH: f32 = 40.0;

pub(crate) const EDITOR_PITCH_TOP_PERCENT: f32 = 20.0;

pub(crate) const EDITOR_PITCH_HEIGHT_PERCENT: f32 = 76.0;

pub(crate) const EDITOR_LYRIC_NOTE_SNAP_DISTANCE_PX: f32 = 9.0;

pub(crate) const EDITOR_LYRIC_NOTE_SNAP_MAX_SECONDS: f64 = 0.12;

/// A note/lyric move or resize drag only starts changing the chart once the
/// pointer has moved this far from where the mouse went down. Below it, a
/// press-and-release reads as a plain click rather than an edit — without
/// this, an ordinary click can jitter a pixel or two and, worse, land inside
/// the lyric/note snap radius (`EDITOR_LYRIC_NOTE_SNAP_DISTANCE_PX`), which
/// then snaps a bound note to a neighboring boundary the user never meant to
/// touch. Densely packed lyrics (several overlapping rows) are exactly where
/// notes sit close enough for that snap to fire from a near-zero movement.
pub(crate) const EDITOR_DRAG_MIN_PX: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorDockSelectKind {
    AudioSource,
    SnapGrid,
    AuditionMode,
}

/// What an audition plays back. The synthesized pitch is a second stream; the
/// song audio is never altered to produce it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum AuditionMode {
    /// The song as recorded.
    #[default]
    Audio,
    /// Only the note targets, as tones.
    Pitch,
    /// Both at once, for checking the chart against the recording.
    Mixed,
}

/// Which stem the overview waveform is decoded from. Independent of
/// `audio_source` (what plays back) — the waveform is an alignment aid, so
/// it's picked separately, right-click on the waveform itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaveformSource {
    Instrumental,
    Vocals,
    Original,
}

/// How the overview waveform's peaks are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum WaveformStyle {
    #[default]
    Bars,
    Filled,
    Line,
}

#[derive(Clone, Copy)]
pub(crate) struct WaveformContextMenu {
    pub(crate) position: Vec2,
}

impl AuditionMode {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Pitch => "pitch",
            Self::Mixed => "mixed",
        }
    }

    pub(crate) fn from_label(value: &str) -> Self {
        match value {
            "pitch" => Self::Pitch,
            "mixed" => Self::Mixed,
            _ => Self::Audio,
        }
    }

    pub(crate) fn cycle(self) -> Self {
        match self {
            Self::Audio => Self::Pitch,
            Self::Pitch => Self::Mixed,
            Self::Mixed => Self::Audio,
        }
    }

    pub(crate) fn plays_song(self) -> bool {
        matches!(self, Self::Audio | Self::Mixed)
    }

    pub(crate) fn plays_tones(self) -> bool {
        matches!(self, Self::Pitch | Self::Mixed)
    }
}

/// Tap-to-time state.
///
/// Tapping along with playback is how a rap or spoken line gets its timing:
/// hold the tap key for as long as the syllable lasts. With notes selected the
/// taps re-time those notes in order instead of adding new ones, so an
/// existing line can be re-performed rather than rebuilt.
#[derive(Default)]
pub(crate) struct TapSession {
    /// Notes queued for re-timing, in time order.
    pub(crate) retiming: Vec<usize>,
    /// How far through `retiming` the taps have got.
    pub(crate) retimed: usize,
    /// The note under the finger, and the position it started at.
    pub(crate) holding: Option<(usize, f64)>,
}

impl TapSession {
    /// The note the next tap re-times, if any are still queued.
    pub(crate) fn next_retarget(&self) -> Option<usize> {
        self.retiming.get(self.retimed).copied()
    }

    pub(crate) fn remaining(&self) -> usize {
        self.retiming.len().saturating_sub(self.retimed)
    }
}

pub(crate) struct NativeEditor {
    pub(crate) chart: app_core::ChartDocument,
    /// Immutable Candidate/Authored revision this working copy was opened
    /// from. Ordinary current-chart loads leave this unset.
    pub(crate) artifact_source: Option<app_core::ArtifactRef>,
    /// The authoritative UTZ 0.2 chart under edit. Every note and lyric change
    /// goes through it; nothing is re-derived from analyzer JSON on save.
    pub(crate) document: app_core::EditorDocument,
    /// The chart's analyzer pitch evidence, decoded from `chart.pitch_track`
    /// once at load. That JSON is read-only analyzer output — editing never
    /// touches it — so re-parsing it on every UI rebuild (as `view.rs` used
    /// to, once per visible note) was pure waste.
    pub(crate) pitch_frames: Vec<ChartPitchFrame>,
    /// Absolute-second beat timestamps from `{file_hash}_music_analysis.json`,
    /// decoded once at load, same as `pitch_frames`. Empty when analysis
    /// never ran, found no confident tempo, or produced no individually
    /// detected beats — the beat grid then draws nothing rather than a
    /// fabricated or misleading one.
    pub(crate) beats: Vec<f64>,
    /// Whether the beat grid draws at all, independent of whether `beats`
    /// has data. User-toggleable; on by default when there is data to show.
    pub(crate) beat_grid_visible: bool,
    /// The last-computed chart-checks report, alongside the document
    /// revision it was computed from. `refresh_editor_problems_cache`
    /// recomputes it only when the revision has moved, since a full
    /// problems() pass over every note and lyric on every UI rebuild is
    /// wasted work when nothing changed.
    pub(crate) problems_cache: (u64, app_core::ProblemReport),
    pub(crate) waveform: app_core::ChartWaveform,
    pub(crate) waveform_source: WaveformSource,
    pub(crate) waveform_style: WaveformStyle,
    /// The right-click waveform menu, open at a screen position.
    pub(crate) waveform_context: Option<WaveformContextMenu>,
    pub(crate) audio_source: String,
    pub(crate) visible_position: f64,
    pub(crate) audio_status: uta_studio_audio::EditorAudioStatus,
    pub(crate) last_audio_sync: Instant,
    pub(crate) viewport_start: f64,
    pub(crate) viewport_duration: f64,
    pub(crate) pitch_min: f64,
    pub(crate) pitch_max: f64,
    pub(crate) lyrics_hidden: bool,
    pub(crate) inspector_open: bool,
    pub(crate) selected_note: Option<usize>,
    pub(crate) selected_notes: BTreeSet<usize>,
    pub(crate) selected_word: Option<WordSelection>,
    pub(crate) selected_words: BTreeSet<WordSelection>,
    pub(crate) word_edit_focus: Option<WordSelection>,
    pub(crate) snap_seconds: f64,
    pub(crate) audition_mode: AuditionMode,
    /// Whether the tap key times notes instead of doing nothing.
    pub(crate) tap_mode: bool,
    pub(crate) tap: TapSession,
    /// When a ranged audition should stop, in seconds. `None` while the
    /// transport is under plain manual control.
    pub(crate) audition_until: Option<f64>,
    pub(crate) dirty: bool,
    pub(crate) manual_scroll_until: Instant,
    pub(crate) undo: Vec<ChartSnapshot>,
    pub(crate) redo: Vec<ChartSnapshot>,
    pub(crate) clipboard_notes: Vec<app_core::ClipboardNote>,
    /// The right-click lyric menu, open at a screen position. Its actions run
    /// against whatever is selected at the time it's opened.
    pub(crate) lyric_context: Option<LyricContextMenu>,
    /// The right-click note (pitch) menu, open at a screen position.
    pub(crate) note_context: Option<NoteContextMenu>,
    /// Whether the chart-checks panel (opened from its own toolbar button)
    /// is showing. Kept out of the inspector column so a long problem list
    /// can't crowd out the rest of it.
    pub(crate) problems_panel_open: bool,
    pub(crate) problems_filter: ProblemsFilter,
    /// The keyboard/mouse shortcut cheat sheet, opened from its own toolbar
    /// button. Covers the gestures with no single-key registry entry to
    /// read a shortcut from: marquee-select, the wheel modifiers, tap mode.
    pub(crate) shortcuts_panel_open: bool,
    /// The whole-song lyrics editor: every phrase's text, one per line, so a
    /// pass over the whole song doesn't mean clicking into each line one at
    /// a time. Unlike the song-detail lyrics dialog, this writes straight
    /// into the existing phrases/notes rather than re-running alignment, so
    /// pitch and timing already authored survive it.
    pub(crate) all_lyrics_editor_open: bool,
    /// Set by the "Add note" toolbar button: the next press-and-drag on the
    /// timeline canvas places a note and sizes it to the drag instead of
    /// selecting or panning. Cleared once that drag starts.
    pub(crate) note_insert_armed: bool,
    /// While locked, the mouse cannot move or resize a note or lyric — only
    /// select, pan, zoom, and the keyboard nudge/pitch chords still work.
    /// Guards against an accidental drag once timing is dialed in.
    pub(crate) lock_mode: bool,
    /// Which side's timing wins when "Bind" merges a lyric-only note onto a
    /// pitch-only one — the MIDI note's own start/end (the historical
    /// behavior) or the lyric's.
    pub(crate) bind_alignment: BindAlignment,
    /// The audio source `PlayNoteVocal` temporarily switched to (always
    /// "vocals"), to be restored once the ranged audition it started ends.
    /// `None` when no such restore is pending.
    pub(crate) audition_restore_source: Option<String>,
}

/// Which timing a "Bind" merge keeps when the lyric and the pitch note it is
/// bound to disagree — see [`NativeEditor::bind_alignment`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum BindAlignment {
    #[default]
    Pitch,
    Lyric,
}

impl BindAlignment {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Pitch => "MIDI",
            Self::Lyric => "Lyric",
        }
    }

    pub(crate) fn toggled(self) -> Self {
        match self {
            Self::Pitch => Self::Lyric,
            Self::Lyric => Self::Pitch,
        }
    }
}

/// A lyric token address: (phrase, syllable). The editor still calls these
/// words because that is what the lyric lane shows.
pub(crate) type WordSelection = app_core::LyricAddress;

#[derive(Clone, Copy)]
pub(crate) struct LyricContextMenu {
    pub(crate) position: Vec2,
}

#[derive(Clone, Copy)]
pub(crate) struct NoteContextMenu {
    pub(crate) position: Vec2,
    /// The selected syllable that this note could hold as a pitch-glide
    /// continuation, captured before right-clicking the note replaces the
    /// selection with the note itself.
    pub(crate) continue_word: Option<WordSelection>,
}

/// Which severities the chart-checks panel lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ProblemsFilter {
    #[default]
    All,
    Errors,
    Warnings,
}

impl ProblemsFilter {
    pub(crate) fn matches(self, severity: app_core::Severity) -> bool {
        match self {
            Self::All => true,
            Self::Errors => severity == app_core::Severity::Error,
            Self::Warnings => severity == app_core::Severity::Warning,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Errors => "Errors",
            Self::Warnings => "Warnings",
        }
    }
}

/// One undoable step: what the chart looked like before an edit, and what to
/// call that edit in the history.
#[derive(Clone)]
pub(crate) struct ChartSnapshot {
    pub(crate) label: &'static str,
    pub(crate) chart: app_core::VocalChartV1,
}

impl NativeEditor {
    pub(crate) fn new(
        chart: app_core::ChartDocument,
        audio_status: uta_studio_audio::EditorAudioStatus,
        waveform: app_core::ChartWaveform,
        waveform_source: WaveformSource,
        audio_source: impl Into<String>,
    ) -> Self {
        let document = app_core::EditorDocument::new(chart.vocal_chart.clone());
        let pitch_frames = chart_pitch_frames(&chart);
        let beats = load_editor_beats(&chart.file_hash);
        let problems_cache = (document.revision(), document.problems());
        let notes = document.notes();
        let pitch_min = notes
            .iter()
            .filter(|note| note.pitched)
            .map(|note| note.midi)
            .reduce(f64::min)
            .unwrap_or(48.0)
            .floor()
            - 2.0;
        let pitch_max = notes
            .iter()
            .filter(|note| note.pitched)
            .map(|note| note.midi)
            .reduce(f64::max)
            .unwrap_or(72.0)
            .ceil()
            + 2.0;
        Self {
            chart,
            artifact_source: None,
            document,
            pitch_frames,
            beat_grid_visible: !beats.is_empty(),
            beats,
            problems_cache,
            waveform,
            waveform_source,
            waveform_style: WaveformStyle::default(),
            waveform_context: None,
            audio_source: audio_source.into(),
            visible_position: audio_status.position_secs,
            audio_status,
            last_audio_sync: Instant::now(),
            viewport_start: 0.0,
            viewport_duration: 12.0,
            pitch_min,
            pitch_max: pitch_max.max(pitch_min + 12.0),
            lyrics_hidden: false,
            inspector_open: false,
            selected_note: None,
            selected_notes: BTreeSet::new(),
            selected_word: None,
            selected_words: BTreeSet::new(),
            word_edit_focus: None,
            snap_seconds: 0.05,
            audition_mode: AuditionMode::default(),
            tap_mode: false,
            tap: TapSession::default(),
            audition_until: None,
            dirty: false,
            manual_scroll_until: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard_notes: Vec::new(),
            lyric_context: None,
            note_context: None,
            problems_panel_open: false,
            problems_filter: ProblemsFilter::default(),
            shortcuts_panel_open: false,
            all_lyrics_editor_open: false,
            note_insert_armed: false,
            lock_mode: false,
            bind_alignment: BindAlignment::default(),
            audition_restore_source: None,
        }
    }

    pub(crate) fn viewport_end(&self) -> f64 {
        self.viewport_start + self.viewport_duration
    }

    /// The chart-checks report, recomputed only when the document has
    /// actually changed since the last call.
    pub(crate) fn refresh_problems(&mut self) -> &app_core::ProblemReport {
        let revision = self.document.revision();
        if self.problems_cache.0 != revision {
            self.problems_cache = (revision, self.document.problems());
        }
        &self.problems_cache.1
    }

    pub(crate) fn snapshot(&self, label: &'static str) -> ChartSnapshot {
        ChartSnapshot {
            label,
            chart: self.document.chart().clone(),
        }
    }

    pub(crate) fn checkpoint(&mut self, label: &'static str) {
        self.undo.push(self.snapshot(label));
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub(crate) fn restore(&mut self, snapshot: ChartSnapshot) {
        self.document = app_core::EditorDocument::new(snapshot.chart);
        self.clear_selection();
        self.dirty = true;
    }

    /// Drops every note and lyric selection. Used whenever indices stop
    /// pointing at what the user picked, such as after an undo or a repair.
    pub(crate) fn clear_selection(&mut self) {
        self.selected_note = None;
        self.selected_notes.clear();
        self.selected_word = None;
        self.selected_words.clear();
        self.word_edit_focus = None;
    }

    /// Keeps the playhead from stealing the viewport back right after the user
    /// moved it, as the interaction rules require.
    pub(crate) fn hold_manual_scroll(&mut self) {
        self.manual_scroll_until = Instant::now() + Duration::from_secs(2);
    }

    /// Undoes the most recent edit and reports what it was called.
    pub(crate) fn undo(&mut self) -> Option<&'static str> {
        let snapshot = self.undo.pop()?;
        let label = snapshot.label;
        self.redo.push(self.snapshot(label));
        self.restore(snapshot);
        Some(label)
    }

    pub(crate) fn redo(&mut self) -> Option<&'static str> {
        let snapshot = self.redo.pop()?;
        let label = snapshot.label;
        self.undo.push(self.snapshot(label));
        self.restore(snapshot);
        Some(label)
    }

    /// The edits waiting to be undone and redone, newest last.
    pub(crate) fn history(&self) -> (Vec<&'static str>, Vec<&'static str>) {
        (
            self.undo.iter().map(|entry| entry.label).collect(),
            self.redo.iter().map(|entry| entry.label).collect(),
        )
    }

    pub(crate) fn select_only_note(&mut self, index: usize) {
        self.selected_note = Some(index);
        self.selected_notes.clear();
        self.selected_notes.insert(index);
        self.selected_word = None;
        self.selected_words.clear();
        self.word_edit_focus = None;
    }

    pub(crate) fn selected_note_indices(&self) -> BTreeSet<usize> {
        if self.selected_notes.is_empty() {
            self.selected_note.into_iter().collect()
        } else {
            self.selected_notes.clone()
        }
    }

    pub(crate) fn select_only_word(&mut self, selection: WordSelection) {
        if self.selected_word != Some(selection) {
            self.word_edit_focus = None;
        }
        self.selected_word = Some(selection);
        self.selected_words.clear();
        self.selected_words.insert(selection);
        self.selected_note = None;
        self.selected_notes.clear();
    }

    pub(crate) fn selected_word_indices(&self) -> BTreeSet<WordSelection> {
        if self.selected_words.is_empty() {
            self.selected_word.into_iter().collect()
        } else {
            self.selected_words.clone()
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ChartNoteView {
    pub(crate) index: usize,
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) midi: f64,
    /// Rhythm, spoken, and freestyle notes carry no pitch target to hit.
    pub(crate) pitched: bool,
    /// An unclassified placeholder nobody has confirmed yet — reads as white
    /// so it's never mistaken for an intentionally authored Rap note.
    pub(crate) placeholder: bool,
    pub(crate) kind: app_core::NoteKind,
    /// This note's own syllable, when it carries one directly (not a held
    /// continuation of an earlier syllable).
    pub(crate) lyric: Option<String>,
    /// Whether this note holds a syllable started on an earlier note, so the
    /// timeline can mark it as a continuation rather than a separate word.
    pub(crate) continues_lyric: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ChartPitchFrame {
    pub(crate) time: f64,
    pub(crate) midi: f64,
    pub(crate) confidence: f64,
}

#[derive(Clone, Debug)]
pub(crate) struct ChartLyricView {
    pub(crate) segment: usize,
    pub(crate) word: usize,
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) text: String,
    pub(crate) lane: usize,
    pub(crate) guided: bool,
    /// Flattened index of the note this lyric is bound to.
    pub(crate) note: usize,
    /// Flattened indices of any notes that hold this syllable through a
    /// pitch change past `note` — see `app_core::ChartLyric::continuation_notes`.
    pub(crate) continuation_notes: Vec<usize>,
}

#[derive(Resource)]
pub(crate) struct EditorAudioSyncTimer(pub(crate) Timer);

#[derive(Default)]
pub(crate) struct NativeEditorLoadJob {
    pub(crate) receiver: Option<Mutex<mpsc::Receiver<Result<NativeEditor, String>>>>,
}

#[derive(Component)]
pub(crate) struct EditorPlayhead;

#[derive(Component)]
pub(crate) struct EditorClockText;

#[derive(Component)]
pub(crate) struct EditorTimelineSurface;

#[derive(Component)]
pub(crate) struct EditorLyricsSurface;

#[derive(Component)]
pub(crate) struct EditorAlignmentGuide;

#[derive(Component)]
pub(crate) struct EditorNoteNode(pub(crate) usize);

#[derive(Clone, Copy)]
pub(crate) enum NoteEdge {
    Start,
    End,
}

#[derive(Component)]
pub(crate) struct EditorNoteResizeHandle {
    pub(crate) index: usize,
    pub(crate) edge: NoteEdge,
}

#[derive(Component)]
pub(crate) struct EditorLyricNode {
    pub(crate) selection: WordSelection,
}

#[derive(Component)]
pub(crate) struct EditorLyricResizeHandle {
    pub(crate) selection: WordSelection,
    pub(crate) edge: NoteEdge,
}

/// A vertical mark at the shared time of a bound note and lyric. Three
/// instances — one in the pitch canvas, one in the gap strip between canvas
/// and lyric lane, one in the lyric lane — are kept at the same `left` each
/// frame and each sized to its `EditorBindingGuidePart`, so together they
/// read as one line running from the note's own pitch height down to the
/// lyric's own lane, through the gap in between.
#[derive(Component)]
pub(crate) struct EditorBindingGuide;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorBindingGuidePart {
    /// From the bound note's pitch height down to the bottom of the canvas.
    Canvas,
    /// The full height of the thin strip between canvas and lyric lane.
    Gap,
    /// From the top of the lyric lane down to the bound word's own lane.
    Lane,
}

/// The rectangle drawn while shift-dragging a marquee selection over the
/// note canvas.
#[derive(Component)]
pub(crate) struct EditorMarqueeBox;

/// The scrollable problem list inside the chart-checks panel.
#[derive(Component)]
pub(crate) struct EditorProblemsList;

/// The scrollable shortcuts cheat sheet panel.
#[derive(Component)]
pub(crate) struct EditorShortcutsPanel;

/// The status-bar hint text that reveals the shortcuts panel on hover.
#[derive(Component)]
pub(crate) struct EditorShortcutsHoverTrigger;

/// The whole-song lyrics textarea: one line per phrase, in phrase order.
#[derive(Component)]
pub(crate) struct EditorAllLyricsInput;

#[derive(Component)]
pub(crate) struct EditorWordInput(pub(crate) WordSelection);

#[derive(Component)]
pub(crate) struct InlineEditorWordInput;

/// The whole-line lyric field: the phrase it edits.
#[derive(Component)]
pub(crate) struct EditorPhraseInput(pub(crate) usize);

/// The singer-name field of one track in the track strip.
#[derive(Component)]
pub(crate) struct EditorSingerInput(pub(crate) usize);

#[derive(Resource, Default)]
pub(crate) struct EditorPointerCapture {
    pub(crate) drag: Option<EditorDrag>,
    pub(crate) alignment_guide: Option<f64>,
    pub(crate) last_surface_click: Option<(Instant, Vec2)>,
    pub(crate) last_lyric_click: Option<(Instant, WordSelection)>,
}

#[derive(Clone)]
pub(crate) enum EditorDrag {
    Note {
        pointer_start: Vec2,
        originals: Vec<EditorNoteOriginal>,
        viewport_duration: f64,
        pitch_span: f64,
    },
    ResizeNote {
        index: usize,
        edge: NoteEdge,
        pointer_start: Vec2,
        original_start: f64,
        original_end: f64,
        viewport_duration: f64,
    },
    Lyric {
        pointer_start: Vec2,
        originals: Vec<EditorWordOriginal>,
        viewport_duration: f64,
    },
    ResizeLyric {
        selection: WordSelection,
        edge: NoteEdge,
        pointer_start: Vec2,
        original_start: f64,
        original_end: f64,
        viewport_duration: f64,
    },
    Pan {
        pointer_start: Vec2,
        viewport_start: f64,
        pitch_min: f64,
        pitch_max: f64,
    },
    Marquee {
        pointer_start: Vec2,
        base: BTreeSet<usize>,
        viewport_start: f64,
        viewport_duration: f64,
        pitch_min: f64,
        pitch_max: f64,
    },
    /// A note placed by the armed "Add note" tool: `note_index` was created
    /// at press time with a minimal length, and every frame of the drag
    /// resizes it to span from `anchor_time` to the pointer, in either
    /// direction.
    InsertNote {
        note_index: usize,
        anchor_time: f64,
        pointer_start: Vec2,
        viewport_duration: f64,
    },
}

#[derive(Clone)]
pub(crate) struct EditorNoteOriginal {
    pub(crate) index: usize,
    pub(crate) start: f64,
    pub(crate) end: f64,
    pub(crate) midi: f64,
}

#[derive(Clone)]
pub(crate) struct EditorWordOriginal {
    pub(crate) selection: WordSelection,
    pub(crate) start: f64,
    pub(crate) end: f64,
}

/// How many recent edits the inspector lists before collapsing the rest.
pub(crate) const EDITOR_HISTORY_ROWS: usize = 8;

pub(crate) fn selected_editor_word(
    document: &app_core::EditorDocument,
    selection: WordSelection,
) -> Option<(String, f64, f64)> {
    document.lyric(selection)
}

pub(crate) fn all_editor_word_selections(
    document: &app_core::EditorDocument,
) -> BTreeSet<WordSelection> {
    document.lyric_addresses()
}

/// Notes of every track except the one being edited, so the timeline can show
/// where the other voices sing. They are drawn as ghosts and never picked.
pub(crate) fn other_track_notes(document: &app_core::EditorDocument) -> Vec<ChartNoteView> {
    (0..document.track_count())
        .filter(|index| *index != document.active_track_index())
        .flat_map(|index| document.track_notes(index))
        .map(|note| ChartNoteView {
            index: note.index,
            start: note.start,
            end: note.end,
            midi: note.midi,
            pitched: note.pitched,
            placeholder: note.placeholder,
            kind: note.kind,
            lyric: note.lyric,
            continues_lyric: note.continues_lyric,
        })
        .collect()
}

pub(crate) fn chart_notes(document: &app_core::EditorDocument) -> Vec<ChartNoteView> {
    document
        .notes()
        .into_iter()
        .map(|note| ChartNoteView {
            index: note.index,
            start: note.start,
            end: note.end,
            midi: note.midi,
            pitched: note.pitched,
            placeholder: note.placeholder,
            kind: note.kind,
            lyric: note.lyric,
            continues_lyric: note.continues_lyric,
        })
        .collect()
}

pub(crate) fn chart_pitch_frames(chart: &app_core::ChartDocument) -> Vec<ChartPitchFrame> {
    chart
        .pitch_track
        .get("frames")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|frame| {
            let hz = frame.get("hz")?.as_f64()?;
            let time = frame.get("time")?.as_f64()?;
            (hz.is_finite() && hz > 0.0).then(|| ChartPitchFrame {
                time,
                midi: 69.0 + 12.0 * (hz / 440.0).log2(),
                confidence: frame
                    .get("confidence")
                    .and_then(serde_json::Value::as_f64)
                    .unwrap_or(1.0)
                    .clamp(0.0, 1.0),
            })
        })
        .collect()
}

/// Beat timestamps for the background beat grid. Deliberately conservative:
/// a missing analysis, an unconfident tempo, or a backend that could only
/// guess a global BPM without individual beats (see `rhythm.py`) all read as
/// "nothing to draw" here rather than a grid that might be wrong.
const EDITOR_BEAT_GRID_MIN_CONFIDENCE: f64 = 0.15;

fn load_editor_beats(file_hash: &str) -> Vec<f64> {
    // `try_new` (not `new`, which panics if the cache directory can't be
    // created) -- same "missing data reads as nothing to draw" philosophy
    // this function already documents above applies here too: a cache
    // directory that can't be created is just another reason to skip the
    // beat grid, not a reason to crash the whole editor.
    let Some(cache) = app_core::CacheDir::try_new() else {
        return Vec::new();
    };
    app_core::load_music_analysis(&cache, file_hash)
        .filter(|analysis| {
            analysis.rhythm.bpm.is_some()
                && analysis.rhythm.confidence >= EDITOR_BEAT_GRID_MIN_CONFIDENCE
        })
        .map(|analysis| analysis.rhythm.beats)
        .unwrap_or_default()
}

pub(crate) fn abstract_pitch_contour(
    frames: &[ChartPitchFrame],
    max_points: usize,
) -> Vec<ChartPitchFrame> {
    if frames.is_empty() || max_points == 0 {
        return Vec::new();
    }
    let stride = frames.len().div_ceil(max_points).max(1);
    frames
        .chunks(stride)
        .filter_map(|chunk| {
            let weight = chunk
                .iter()
                .map(|frame| frame.confidence.clamp(0.05, 1.0))
                .sum::<f64>();
            (weight > 0.0).then(|| ChartPitchFrame {
                time: chunk
                    .iter()
                    .map(|frame| frame.time * frame.confidence.clamp(0.05, 1.0))
                    .sum::<f64>()
                    / weight,
                midi: chunk
                    .iter()
                    .map(|frame| frame.midi * frame.confidence.clamp(0.05, 1.0))
                    .sum::<f64>()
                    / weight,
                confidence: chunk.iter().map(|frame| frame.confidence).sum::<f64>()
                    / chunk.len() as f64,
            })
        })
        .collect()
}

/// Lyric lanes grow on demand as overlapping words need them, rather than
/// hard-capping at a fixed count and cramming the overflow into whichever
/// lane frees up soonest. A dense run of overlapping lyrics still degrades
/// past this many lanes, but 3 was cramping far more often than it needed to.
pub(crate) const MAX_LYRIC_LANES: usize = 5;

pub(crate) fn chart_lyrics(document: &app_core::EditorDocument) -> Vec<ChartLyricView> {
    let mut lyrics = document.lyrics();
    lyrics.retain(|lyric| !lyric.text.trim().is_empty());
    lyrics.sort_by(|left, right| left.start.total_cmp(&right.start));
    let mut lane_ends: Vec<f64> = Vec::new();
    lyrics
        .into_iter()
        .map(|lyric| {
            let lane = lane_ends
                .iter()
                .position(|lane_end| *lane_end <= lyric.start)
                .or_else(|| {
                    (lane_ends.len() < MAX_LYRIC_LANES).then(|| {
                        lane_ends.push(f64::NEG_INFINITY);
                        lane_ends.len() - 1
                    })
                })
                .unwrap_or_else(|| {
                    lane_ends
                        .iter()
                        .enumerate()
                        .min_by(|left, right| left.1.total_cmp(right.1))
                        .map(|(index, _)| index)
                        .unwrap_or(0)
                });
            lane_ends[lane] = lyric.end.max(lyric.start + 0.04);
            ChartLyricView {
                segment: lyric.address.segment,
                word: lyric.address.word,
                start: lyric.start,
                end: lyric.end,
                text: lyric.text,
                lane,
                guided: lyric.guided,
                note: lyric.note,
                continuation_notes: lyric.continuation_notes,
            }
        })
        .collect()
}

pub(crate) fn time_percent(time: f64, editor: &NativeEditor) -> f32 {
    (((time - editor.viewport_start) / editor.viewport_duration) * 100.0).clamp(0.0, 100.0) as f32
}

pub(crate) fn pitch_percent(midi: f64, editor: &NativeEditor) -> f32 {
    let span = (editor.pitch_max - editor.pitch_min).max(1.0);
    (EDITOR_PITCH_TOP_PERCENT
        + (((editor.pitch_max - midi) / span) as f32 * EDITOR_PITCH_HEIGHT_PERCENT))
        .clamp(
            EDITOR_PITCH_TOP_PERCENT,
            EDITOR_PITCH_TOP_PERCENT + EDITOR_PITCH_HEIGHT_PERCENT,
        )
}

pub(crate) fn midi_note_name(midi: f64) -> String {
    const NAMES: [&str; 12] = [
        "C", "C♯", "D", "D♯", "E", "F", "F♯", "G", "G♯", "A", "A♯", "B",
    ];
    let midi = midi.round().clamp(0.0, 127.0) as i32;
    format!("{}{}", NAMES[midi.rem_euclid(12) as usize], midi / 12 - 1)
}

pub(crate) fn editor_note_color(
    kind: app_core::NoteKind,
    placeholder: bool,
    theme: &StudioTheme,
) -> Color {
    // An unconfirmed placeholder reads as neutral white regardless of what
    // it would otherwise render as (usually Rap) — it hasn't been triaged,
    // so it shouldn't look like an intentional choice.
    if placeholder {
        return Color::srgb(0.86, 0.86, 0.88);
    }
    match kind {
        app_core::NoteKind::Golden => Color::srgb(0.94, 0.67, 0.2),
        app_core::NoteKind::GoldenRap => Color::srgb(0.94, 0.45, 0.18),
        app_core::NoteKind::Rap => Color::srgb(0.71, 0.43, 0.92),
        app_core::NoteKind::Freestyle => theme.muted_foreground.with_alpha(0.48),
        app_core::NoteKind::Normal => theme.note_normal,
    }
}

pub(crate) fn set_editor_pitch_span(editor: &mut NativeEditor, span: f64) {
    let span = span.clamp(8.0, 127.0);
    let center = (editor.pitch_min + editor.pitch_max) / 2.0;
    editor.pitch_min = (center - span / 2.0).clamp(0.0, 127.0 - span);
    editor.pitch_max = editor.pitch_min + span;
}

pub(crate) fn format_editor_clock(position: f64, duration: f64) -> String {
    format!(
        "{} / {}",
        format_duration(position),
        format_duration(duration)
    )
}

pub(crate) fn format_snap_grid(seconds: f64) -> String {
    if seconds <= 0.0 {
        "off".to_string()
    } else {
        format!("{}ms", (seconds * 1000.0).round() as u32)
    }
}

pub(crate) fn surface_pitch_fraction(surface_fraction: f32) -> f32 {
    ((surface_fraction * 100.0 - EDITOR_PITCH_TOP_PERCENT) / EDITOR_PITCH_HEIGHT_PERCENT)
        .clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct EditorTimeSnap {
    pub(crate) delta: f64,
    pub(crate) target: f64,
}

pub(crate) fn snap_lyric_move_to_notes(
    originals: &[EditorWordOriginal],
    proposed_delta: f64,
    notes: &[ChartNoteView],
    tolerance: f64,
) -> Option<EditorTimeSnap> {
    let earliest = originals.iter().map(|word| word.start).reduce(f64::min)?;
    let mut closest: Option<(f64, EditorTimeSnap)> = None;
    for moving_edge in originals.iter().flat_map(|word| [word.start, word.end]) {
        let proposed_edge = moving_edge + proposed_delta;
        let Some(target) = nearest_note_boundary(proposed_edge, notes, tolerance) else {
            continue;
        };
        let correction = target - proposed_edge;
        let snapped_delta = proposed_delta + correction;
        if snapped_delta < -earliest {
            continue;
        }
        let distance = correction.abs();
        if closest
            .as_ref()
            .is_none_or(|(closest_distance, _)| distance < *closest_distance)
        {
            closest = Some((
                distance,
                EditorTimeSnap {
                    delta: snapped_delta,
                    target,
                },
            ));
        }
    }
    closest.map(|(_, snap)| snap)
}

pub(crate) fn nearest_note_boundary(
    time: f64,
    notes: &[ChartNoteView],
    tolerance: f64,
) -> Option<f64> {
    notes
        .iter()
        .flat_map(|note| [note.start, note.end])
        .filter_map(|boundary| {
            let distance = (boundary - time).abs();
            (distance <= tolerance).then_some((distance, boundary))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, boundary)| boundary)
}
