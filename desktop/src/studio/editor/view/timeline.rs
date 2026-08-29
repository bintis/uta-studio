use super::*;
use crate::studio::*;

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
                UiPointerApi(&[
                    "ui.pointer.editor_timeline.primary",
                    "ui.pointer.editor_waveform.secondary",
                    "ui.pointer.editor_viewport_pan",
                ]),
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
                        height: percent(editor_pitch_top_percent(editor)),
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
                    let end_index = editor.beats.partition_point(|&beat| beat <= viewport_end);
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
                if !editor.spectrum_hidden {
                    let viewport_end = editor.viewport_end();
                    let frame_start = editor
                        .pitch_frames
                        .partition_point(|frame| frame.time < editor.viewport_start);
                    let frame_end = editor
                        .pitch_frames
                        .partition_point(|frame| frame.time <= viewport_end);
                    let visible_frames = editor.pitch_frames[frame_start..frame_end]
                        .iter()
                        .filter(|frame| frame.confidence >= 0.12)
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
                if !editor.spectrum_hidden && peak_count > 0 && editor.waveform.duration_secs > 0.0
                {
                    let peak_start = ((editor.viewport_start / editor.waveform.duration_secs)
                        * peak_count as f64)
                        .floor()
                        .clamp(0.0, peak_count as f64)
                        as usize;
                    let peak_end = ((editor.viewport_end() / editor.waveform.duration_secs)
                        * peak_count as f64)
                        .ceil()
                        .clamp(0.0, peak_count as f64) as usize;
                    let visible_peaks = editor.waveform.peaks[peak_start..peak_end]
                        .iter()
                        .enumerate()
                        .map(|(offset, peak)| {
                            let index = peak_start + offset;
                            let time =
                                index as f64 / peak_count as f64 * editor.waveform.duration_secs;
                            (time, *peak)
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
                // STARS technique output is a read-only evidence strip. The
                // value is explicitly source-local and uncalibrated; it never
                // creates, splits, or moves a MIDI note.
                if editor
                    .visible_evidence
                    .contains(&app_core::EvidenceKind::StarsTechnique)
                    && let Some(track) = editor
                        .evidence
                        .tracks
                        .iter()
                        .find(|track| track.kind == app_core::EvidenceKind::StarsTechnique)
                {
                    for (chunk_index, group) in track.points.chunks(9).enumerate() {
                        let Some((position, point)) = group
                            .iter()
                            .enumerate()
                            .filter(|(_, point)| {
                                point.time >= editor.viewport_start
                                    && point.time <= editor.viewport_end()
                            })
                            .max_by(|(_, left), (_, right)| left.value.total_cmp(&right.value))
                        else {
                            continue;
                        };
                        let flat_index = chunk_index * 9 + position;
                        let class = point
                            .label
                            .as_deref()
                            .and_then(|label| label.split(" · ").next())
                            .unwrap_or("technique");
                        canvas
                            .spawn((
                                Node {
                                    position_type: PositionType::Absolute,
                                    left: percent(time_percent(point.time, editor)),
                                    top: px(18),
                                    max_width: px(108),
                                    padding: UiRect::axes(px(4), px(2)),
                                    border: UiRect::all(px(1)),
                                    border_radius: BorderRadius::all(px(4)),
                                    ..default()
                                },
                                BackgroundColor(theme.card.with_alpha(0.82)),
                                BorderColor::all(theme.pitch_contour.with_alpha(0.52)),
                                Text::new(format!("{class} · local {:.3} · uncal.", point.value)),
                                ui_text_font(font.clone(), 7.0),
                                TextColor(theme.muted_foreground),
                            ))
                            .observe(
                                move |mut event: On<Pointer<Click>>,
                                      mut editor_state: ResMut<EditorUiState>,
                                      mut invalidated: ResMut<UiInvalidated>| {
                                    event.propagate(false);
                                    if let Some(editor) = editor_state.editor.as_mut() {
                                        editor.selected_technique_point = Some(flat_index);
                                        invalidated.invalidate(UiDirtyRegion::Editor);
                                    }
                                },
                            );
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
                            editor_note_color(ghost.kind, ghost.placeholder, theme)
                                .with_alpha(0.16),
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
                            UiPointerApi(&[
                                "ui.pointer.editor_note.secondary",
                                "ui.pointer.editor_note_drag",
                            ]),
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
                            ZIndex(if selected || bound_highlight || active {
                                2
                            } else {
                                1
                            }),
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
                                    UiPointerApi(&["ui.pointer.editor_note_resize"]),
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
                                  mut editor_state: ResMut<EditorUiState>,
                                  mut invalidated: ResMut<UiInvalidated>| {
                                event.propagate(false);
                                open_note_from_click(
                                    &event,
                                    note_index,
                                    &mut editor_state,
                                    &mut invalidated,
                                );
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
                      mut editor_state: ResMut<EditorUiState>,
                      mut invalidated: ResMut<UiInvalidated>,
                      canvas: Query<
                    (&ComputedNode, &UiGlobalTransform),
                    With<EditorTimelineSurface>,
                >| {
                    open_waveform_menu_from_click(
                        &event,
                        &canvas,
                        &mut editor_state,
                        &mut invalidated,
                    );
                    event.propagate(false);
                },
            );
        });
}

/// Right-clicking inside the waveform header strip (the top
/// `EDITOR_PITCH_TOP_PERCENT` of the pitch canvas) opens a menu to pick which
/// stem it's decoded from and how its peaks are drawn. A right-click lower in
/// the canvas, over the actual note grid, is left alone.
pub(crate) fn open_waveform_menu_from_click(
    event: &Pointer<Click>,
    canvas: &Query<(&ComputedNode, &UiGlobalTransform), With<EditorTimelineSurface>>,
    state: &mut EditorUiState,
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
    let Some(editor) = state.editor.as_mut() else {
        return;
    };
    if editor.spectrum_hidden {
        return;
    }
    editor.waveform_context = Some(WaveformContextMenu {
        position: event.pointer_location.position,
    });
    invalidated.invalidate(UiDirtyRegion::Dialog);
}

/// Anchors a context menu at `pointer` (window coordinates — the editor has
/// no sidebar and its own toolbar is inside `editor_root`, so no offset is
/// needed to reach `editor_root`-local space) without letting it run off the
/// bottom or right edge. `menu_size` is a rough estimate of the menu's own
/// footprint — the exact height depends on how many rows it ends up with —
/// generous is fine, this only needs to keep the menu from being clipped.
pub(crate) fn clamp_menu_position(pointer: Vec2, window_size: Vec2, menu_size: Vec2) -> (f32, f32) {
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
        UiAction::from(EditorCommand::DismissWaveformContext),
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
    let (left, top) = clamp_menu_position(context.position, window_size, Vec2::new(280.0, 420.0));
    parent
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                width: px(280),
                max_height: px(420),
                flex_direction: FlexDirection::Column,
                overflow: Overflow::scroll_y(),
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
                "WAVEFORM TRACK",
                8.0,
                theme.muted_foreground,
            );
            for (label, source) in [
                ("Instrumental", WaveformSource::Instrumental),
                ("Vocals", WaveformSource::Vocals),
                ("Original", WaveformSource::Original),
            ] {
                let available =
                    source != WaveformSource::Vocals || editor.chart.audio.vocals.is_some();
                let active = editor.waveform_source == source;
                spawn_menu_check_row(
                    menu,
                    font.clone(),
                    theme,
                    label,
                    active,
                    available,
                    UiAction::from(EditorCommand::SelectWaveformSource(source)),
                );
            }
            if let Some(context) = editor.source_context.as_ref()
                && !context.audio_artifacts.is_empty()
            {
                menu.spawn((
                    Node {
                        height: px(1),
                        margin: UiRect::vertical(px(4)),
                        ..default()
                    },
                    BackgroundColor(theme.border.with_alpha(0.5)),
                ));
                spawn_text(
                    menu,
                    font.clone(),
                    "ARTIFACT A/B & WAVEFORM",
                    8.0,
                    theme.muted_foreground,
                );
                for (slot, label, available) in [
                    (
                        ArtifactAuditionSlot::A,
                        "Switch to A",
                        editor.artifact_audition.a.is_some(),
                    ),
                    (
                        ArtifactAuditionSlot::B,
                        "Switch to B",
                        editor.artifact_audition.b.is_some(),
                    ),
                ] {
                    spawn_menu_check_row(
                        menu,
                        font.clone(),
                        theme,
                        label,
                        editor.artifact_audition.active == Some(slot),
                        available,
                        UiAction::from(EditorCommand::ActivateArtifactAudition(slot)),
                    );
                }
                for artifact in &context.audio_artifacts {
                    for (slot, prefix, selected) in [
                        (
                            ArtifactAuditionSlot::A,
                            "A",
                            editor.artifact_audition.a.as_ref() == Some(&artifact.revision),
                        ),
                        (
                            ArtifactAuditionSlot::B,
                            "B",
                            editor.artifact_audition.b.as_ref() == Some(&artifact.revision),
                        ),
                    ] {
                        spawn_menu_check_row(
                            menu,
                            font.clone(),
                            theme,
                            &format!("{prefix} · {}", artifact.label),
                            selected,
                            true,
                            UiAction::from(EditorCommand::SelectArtifactAudition(
                                slot,
                                artifact.revision.clone(),
                            )),
                        );
                    }
                    spawn_menu_check_row(
                        menu,
                        font.clone(),
                        theme,
                        &format!("Waveform · {}", artifact.label),
                        editor.artifact_audition.waveform.as_ref() == Some(&artifact.revision),
                        true,
                        UiAction::from(EditorCommand::SelectArtifactWaveform(
                            artifact.revision.clone(),
                        )),
                    );
                }
            }
            menu.spawn((
                Node {
                    height: px(1),
                    margin: UiRect::vertical(px(4)),
                    ..default()
                },
                BackgroundColor(theme.border.with_alpha(0.5)),
            ));
            spawn_text(
                menu,
                font.clone(),
                "WAVEFORM STYLE",
                8.0,
                theme.muted_foreground,
            );
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
                    UiAction::from(EditorCommand::SelectWaveformStyle(style)),
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
        spawn_text(
            row,
            font.clone(),
            if active { "✓" } else { " " },
            10.0,
            color,
        );
        spawn_text(row, font, label, 10.0, color);
    });
}
