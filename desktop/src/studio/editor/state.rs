//! Editor state: the `NativeEditor` resource, viewport, selection,
//! history, and the read-only chart views the timeline renders.

use crate::studio::*;

pub(crate) const EDITOR_TRACK_GUTTER_WIDTH: f32 = 40.0;

pub(crate) const EDITOR_PITCH_TOP_PERCENT: f32 = 20.0;

pub(crate) const EDITOR_PITCH_HEIGHT_PERCENT: f32 = 76.0;

pub(crate) const EDITOR_LYRIC_NOTE_SNAP_DISTANCE_PX: f32 = 9.0;

pub(crate) const EDITOR_LYRIC_NOTE_SNAP_MAX_SECONDS: f64 = 0.12;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EditorDockSelectKind {
    AudioSource,
    SnapGrid,
}

pub(crate) struct NativeEditor {
    pub(crate) chart: app_core::ChartDocument,
    /// The authoritative UTZ 0.2 chart under edit. Every note and lyric change
    /// goes through it; nothing is re-derived from analyzer JSON on save.
    pub(crate) document: app_core::EditorDocument,
    pub(crate) waveform: app_core::ChartWaveform,
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
    pub(crate) dirty: bool,
    pub(crate) manual_scroll_until: Instant,
    pub(crate) undo: Vec<ChartSnapshot>,
    pub(crate) redo: Vec<ChartSnapshot>,
    pub(crate) clipboard_notes: Vec<app_core::ClipboardNote>,
}

/// A lyric token address: (phrase, syllable). The editor still calls these
/// words because that is what the lyric lane shows.
pub(crate) type WordSelection = app_core::LyricAddress;

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
        audio_source: impl Into<String>,
    ) -> Self {
        let document = app_core::EditorDocument::new(chart.vocal_chart.clone());
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
            document,
            waveform,
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
            dirty: false,
            manual_scroll_until: Instant::now(),
            undo: Vec::new(),
            redo: Vec::new(),
            clipboard_notes: Vec::new(),
        }
    }

    pub(crate) fn viewport_end(&self) -> f64 {
        self.viewport_start + self.viewport_duration
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
    pub(crate) kind: app_core::NoteKind,
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

#[derive(Component)]
pub(crate) struct EditorWordInput(pub(crate) WordSelection);

#[derive(Component)]
pub(crate) struct InlineEditorWordInput;

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

/// How many chart problems the inspector lists before collapsing the rest.
pub(crate) const EDITOR_PROBLEM_ROWS: usize = 6;

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
            kind: note.kind,
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

pub(crate) fn chart_lyrics(document: &app_core::EditorDocument) -> Vec<ChartLyricView> {
    let mut lyrics = document.lyrics();
    lyrics.retain(|lyric| !lyric.text.trim().is_empty());
    lyrics.sort_by(|left, right| left.start.total_cmp(&right.start));
    let mut lane_ends = [f64::NEG_INFINITY; 3];
    lyrics
        .into_iter()
        .map(|lyric| {
            let lane = lane_ends
                .iter()
                .position(|lane_end| *lane_end <= lyric.start)
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

pub(crate) fn editor_note_color(kind: app_core::NoteKind, theme: &StudioTheme) -> Color {
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
